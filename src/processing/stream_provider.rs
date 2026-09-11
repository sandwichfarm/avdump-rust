//! Sources of streams to process: files discovered on disk, or synthetic null streams.

use super::CancelToken;
use std::collections::VecDeque;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// A readable + seekable source.
pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// A stream handed to the consumer collection. Dropping it releases its concurrency slot.
pub struct ProvidedStream {
    pub tag: String,
    pub length: u64,
    pub stream: Box<dyn ReadSeek>,
    release: Option<Box<dyn FnOnce() + Send>>,
}

impl ProvidedStream {
    pub fn new(tag: String, length: u64, stream: Box<dyn ReadSeek>, release: Option<Box<dyn FnOnce() + Send>>) -> Self {
        Self { tag, length, stream, release }
    }
}

impl Drop for ProvidedStream {
    fn drop(&mut self) {
        if let Some(r) = self.release.take() {
            r();
        }
    }
}

pub trait StreamProvider: Send {
    /// Blocks until the next stream may be processed; `None` when there are no more streams.
    fn next(&mut self, ct: &CancelToken) -> Option<ProvidedStream>;
}

// ----------------------------------------------------------------------------- semaphore

/// Counting semaphore with cancellation-aware waiting.
pub struct Semaphore {
    count: Mutex<usize>,
    cv: Condvar,
}

impl Semaphore {
    pub fn new(count: usize) -> Arc<Self> {
        Arc::new(Self { count: Mutex::new(count), cv: Condvar::new() })
    }

    pub fn available(&self) -> usize {
        *self.count.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn try_acquire(&self) -> bool {
        let mut c = self.count.lock().unwrap_or_else(|e| e.into_inner());
        if *c > 0 {
            *c -= 1;
            true
        } else {
            false
        }
    }

    /// Wait for a slot; returns false when cancelled.
    pub fn acquire(&self, ct: &CancelToken) -> bool {
        let mut c = self.count.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if *c > 0 {
                *c -= 1;
                return true;
            }
            if ct.is_cancelled() {
                return false;
            }
            let (g, _) = self.cv.wait_timeout(c, Duration::from_millis(50)).unwrap_or_else(|e| e.into_inner());
            c = g;
        }
    }

    pub fn release(&self) {
        let mut c = self.count.lock().unwrap_or_else(|e| e.into_inner());
        *c += 1;
        self.cv.notify_all();
    }
}

// ----------------------------------------------------------------------------- null streams

/// Zero-filled in-memory stream of a fixed length (throughput testing).
pub struct NullStream {
    length: u64,
    position: u64,
}

impl NullStream {
    pub fn new(length: u64) -> Self {
        Self { length, position: 0 }
    }
}

impl Read for NullStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = (buf.len() as u64).min(self.length - self.position) as usize;
        // Buffer contents are irrelevant for the null stream; skip the memset for speed.
        self.position += n as u64;
        Ok(n)
    }
}

impl Seek for NullStream {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.position = match pos {
            SeekFrom::Start(p) => p,
            SeekFrom::End(o) => (self.length as i64 + o).max(0) as u64,
            SeekFrom::Current(o) => (self.position as i64 + o).max(0) as u64,
        }
        .min(self.length);
        Ok(self.position)
    }
}

pub struct NullStreamProvider {
    pub stream_length: u64,
    pub stream_count: usize,
    pub parallel_stream_count: usize,
    limiter: Arc<Semaphore>,
    next_index: usize,
}

impl NullStreamProvider {
    pub fn new(stream_count: usize, stream_length: u64, parallel_stream_count: usize) -> Self {
        Self { stream_length, stream_count, parallel_stream_count, limiter: Semaphore::new(parallel_stream_count.max(1)), next_index: 0 }
    }
}

impl StreamProvider for NullStreamProvider {
    fn next(&mut self, ct: &CancelToken) -> Option<ProvidedStream> {
        if self.next_index >= self.stream_count {
            return None;
        }
        if !self.limiter.acquire(ct) {
            return None;
        }
        let i = self.next_index;
        self.next_index += 1;
        let limiter = Arc::clone(&self.limiter);
        Some(ProvidedStream::new(
            format!("NULL{i}"),
            self.stream_length,
            Box::new(NullStream::new(self.stream_length)),
            Some(Box::new(move || limiter.release())),
        ))
    }
}

// ----------------------------------------------------------------------------- files

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathPartition {
    pub path: String,
    pub concurrent_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathPartitions {
    pub concurrent_count: usize,
    pub partitions: Vec<PathPartition>,
}

impl PathPartitions {
    pub fn new(concurrent_count: usize, partitions: Vec<PathPartition>) -> Self {
        Self { concurrent_count, partitions }
    }
}

struct LocalConcurrency {
    path: String,
    limit: Arc<Semaphore>,
    files: VecDeque<String>,
}

/// Provides file streams while limiting the number of files processed concurrently, globally
/// and per path prefix.
pub struct StreamFromPathsProvider {
    partitions: Vec<LocalConcurrency>,
    global: Arc<Semaphore>,
    pub total_file_count: usize,
    pub total_bytes: u64,
}

impl StreamFromPathsProvider {
    pub fn new(path_partitions: &PathPartitions) -> Self {
        let global_count = path_partitions.concurrent_count.max(1);
        let mut partitions: Vec<LocalConcurrency> = path_partitions
            .partitions
            .iter()
            .map(|pp| LocalConcurrency { path: pp.path.clone(), limit: Semaphore::new(pp.concurrent_count.max(1)), files: VecDeque::new() })
            .collect();
        partitions.push(LocalConcurrency { path: String::new(), limit: Semaphore::new(global_count), files: VecDeque::new() });
        Self { partitions, global: Semaphore::new(global_count), total_file_count: 0, total_bytes: 0 }
    }

    /// Discover files below `paths`. `accept` filters files, `on_error` receives discovery
    /// problems.
    pub fn add_files(&mut self, paths: &[String], include_sub_folders: bool, accept: &mut dyn FnMut(&str) -> bool, on_error: &mut dyn FnMut(String)) {
        let mut on_file = |file_path: &str| {
            if !accept(file_path) {
                return;
            }
            // Resolve symlinks so that the byte total (and later ed2k links) refer to the target.
            let length = match std::fs::metadata(file_path) {
                Ok(md) => md.len(),
                Err(_) => {
                    println!("Could not resolve link target for {file_path}");
                    return;
                }
            };
            self.total_bytes += length;
            let idx = self.partitions.iter().position(|p| file_path.starts_with(&p.path)).unwrap_or(self.partitions.len() - 1);
            self.partitions[idx].files.push_back(file_path.to_string());
            self.total_file_count += 1;
        };
        crate::misc::file_traversal::traverse(paths, include_sub_folders, &mut on_file, on_error);
    }

    fn remaining(&self) -> usize {
        self.partitions.iter().map(|p| p.files.len()).sum()
    }

    fn partition_index(&self, file_path: &str) -> usize {
        self.partitions.iter().position(|p| file_path.starts_with(&p.path)).unwrap_or(self.partitions.len() - 1)
    }
}

impl StreamProvider for StreamFromPathsProvider {
    fn next(&mut self, ct: &CancelToken) -> Option<ProvidedStream> {
        while self.remaining() != 0 {
            if !self.global.acquire(ct) {
                return None;
            }
            // Wait for any partition with pending files to have a free slot.
            let idx = loop {
                if ct.is_cancelled() {
                    self.global.release();
                    return None;
                }
                let candidate = self.partitions.iter().position(|p| !p.files.is_empty() && p.limit.try_acquire());
                match candidate {
                    Some(i) => break i,
                    None => std::thread::sleep(Duration::from_millis(20)),
                }
            };
            let path = self.partitions[idx].files.pop_front().expect("non-empty partition");
            let local = Arc::clone(&self.partitions[idx].limit);
            let global = Arc::clone(&self.global);
            let release: Box<dyn FnOnce() + Send> = Box::new(move || {
                local.release();
                global.release();
            });
            match std::fs::File::open(&path) {
                Ok(file) => {
                    let length = file.metadata().map(|m| m.len()).unwrap_or(0);
                    return Some(ProvidedStream::new(path, length, Box::new(file), Some(release)));
                }
                Err(_) => {
                    release();
                    continue;
                }
            }
        }
        None
    }
}

impl StreamFromPathsProvider {
    /// Partition a path is assigned to (exposed for tests).
    pub fn partition_for(&self, file_path: &str) -> &str {
        &self.partitions[self.partition_index(file_path)].path
    }
}
