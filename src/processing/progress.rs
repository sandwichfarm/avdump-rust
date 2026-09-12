//! Byte/file progress bookkeeping shared between the pipeline and the progress display.

use super::block_stream::BlockStream;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct StreamProgressInfo {
    started_on: Instant,
    file_name: String,
    stream: Arc<BlockStream>,
    consumer_names: Vec<String>,
    length: u64,
}

#[derive(Debug, Clone, Default)]
pub struct BlockConsumerProgress {
    pub name: String,
    pub files_processed: usize,
    pub bytes_processed: u64,
    pub buffer_fill: f64,
    pub active_count: usize,
}

#[derive(Debug, Clone)]
pub struct FileProgress {
    pub id: u64,
    pub file_path: String,
    pub started_on: Instant,
    pub file_length: u64,
    pub bytes_processed: u64,
    pub reader_lock_count: u32,
    pub writer_lock_count: u32,
    pub bytes_processed_per_block_consumer: Vec<(String, u64)>,
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub started_on: Instant,
    pub files_processed: usize,
    pub bytes_processed: u64,
    pub file_progress: Vec<FileProgress>,
    pub block_consumer_progress: Vec<BlockConsumerProgress>,
}

impl Default for Progress {
    fn default() -> Self {
        Self { started_on: Instant::now(), files_processed: 0, bytes_processed: 0, file_progress: Vec::new(), block_consumer_progress: Vec::new() }
    }
}

/// Thread-safe progress registry (`BytesReadProgress` in the original).
pub struct BytesReadProgress {
    files_processed: AtomicUsize,
    bytes_processed: AtomicU64,
    bc_files_processed: Vec<AtomicUsize>,
    bc_bytes_processed: Vec<AtomicU64>,
    bc_name_index: HashMap<String, usize>,
    bc_names: Vec<String>,
    started_on: Mutex<Option<Instant>>,
    streams: Mutex<HashMap<u64, StreamProgressInfo>>,
}

impl BytesReadProgress {
    pub fn new(block_consumer_names: impl IntoIterator<Item = String>) -> Self {
        let bc_names: Vec<String> = block_consumer_names.into_iter().collect();
        let bc_name_index = bc_names.iter().enumerate().map(|(i, n)| (n.clone(), i)).collect();
        Self {
            files_processed: AtomicUsize::new(0),
            bytes_processed: AtomicU64::new(0),
            bc_files_processed: bc_names.iter().map(|_| AtomicUsize::new(0)).collect(),
            bc_bytes_processed: bc_names.iter().map(|_| AtomicU64::new(0)).collect(),
            bc_name_index,
            bc_names,
            started_on: Mutex::new(None),
            streams: Mutex::new(HashMap::new()),
        }
    }

    fn ensure_started(&self) {
        let mut s = self.started_on.lock().unwrap_or_else(|e| e.into_inner());
        if s.is_none() {
            *s = Some(Instant::now());
        }
    }

    pub fn register(&self, id: u64, file_name: &str, length: u64, stream: Arc<BlockStream>, consumer_names: Vec<String>) {
        self.ensure_started();
        let info = StreamProgressInfo { started_on: Instant::now(), file_name: file_name.to_string(), stream, consumer_names, length };
        self.streams.lock().unwrap_or_else(|e| e.into_inner()).insert(id, info);
    }

    pub fn skip(&self, length: u64) {
        self.ensure_started();
        self.bytes_processed.fetch_add(length, Ordering::Relaxed);
        self.files_processed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn finished(&self, id: u64, ran_to_completion: bool) {
        let info = self.streams.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        if let (Some(info), true) = (info, ran_to_completion) {
            for name in &info.consumer_names {
                if let Some(&i) = self.bc_name_index.get(name) {
                    self.bc_bytes_processed[i].fetch_add(info.length, Ordering::Relaxed);
                    self.bc_files_processed[i].fetch_add(1, Ordering::Relaxed);
                }
            }
            self.bytes_processed.fetch_add(info.length, Ordering::Relaxed);
            self.files_processed.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn get_progress(&self) -> Progress {
        let mut bc_progress: Vec<BlockConsumerProgress> = self
            .bc_names
            .iter()
            .enumerate()
            .map(|(i, name)| BlockConsumerProgress {
                name: name.clone(),
                bytes_processed: self.bc_bytes_processed[i].load(Ordering::Relaxed),
                files_processed: self.bc_files_processed[i].load(Ordering::Relaxed),
                buffer_fill: 0.0,
                active_count: 0,
            })
            .collect();

        let mut bytes_processed = self.bytes_processed.load(Ordering::Relaxed);
        let files_processed = self.files_processed.load(Ordering::Relaxed);

        let streams = self.streams.lock().unwrap_or_else(|e| e.into_inner());
        let mut file_progress = Vec::with_capacity(streams.len());
        for (id, info) in streams.iter() {
            let stream = &info.stream;
            let buffer_length = stream.buffer_length() as f64;
            let produced = stream.progress_bytes(0);
            let mut bcf = Vec::with_capacity(info.consumer_names.len());
            let mut local_bytes = 0u64;
            let mut active = 0u64;
            for (i, name) in info.consumer_names.iter().enumerate() {
                let read = stream.progress_bytes(i + 1);
                bcf.push((name.clone(), read));
                if let Some(&idx) = self.bc_name_index.get(name) {
                    let p = &mut bc_progress[idx];
                    let consuming = !stream.is_consumer_completed(i) && stream.is_consumer_active(i);
                    if consuming {
                        p.active_count += 1;
                        active += 1;
                        local_bytes += read;
                    }
                    p.bytes_processed += read;
                    p.buffer_fill += produced.saturating_sub(read) as f64 / buffer_length;
                }
            }
            bytes_processed += local_bytes.checked_div(active).unwrap_or(0);
            file_progress.push(FileProgress {
                id: *id,
                file_path: info.file_name.clone(),
                started_on: info.started_on,
                file_length: info.length,
                bytes_processed: produced,
                reader_lock_count: stream.buffer_underrun_count(),
                writer_lock_count: stream.buffer_overrun_count(),
                bytes_processed_per_block_consumer: bcf,
            });
        }
        drop(streams);

        for p in bc_progress.iter_mut() {
            if p.active_count > 0 {
                p.buffer_fill /= p.active_count as f64;
            }
            p.buffer_fill = p.buffer_fill.clamp(0.0, 1.0);
        }

        let started_on = self.started_on.lock().unwrap_or_else(|e| e.into_inner()).unwrap_or_else(Instant::now);
        Progress { started_on, files_processed, bytes_processed, file_progress, block_consumer_progress: bc_progress }
    }
}
