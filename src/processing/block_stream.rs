//! Single-writer / multi-reader circular block stream on top of a [`MirroredBuffer`].
//!
//! The producer (a file or any other forward-readable source) fills the buffer while every
//! consumer reads from its own position. The producer blocks while the slowest consumer has not
//! freed enough space, consumers block while not enough data is available.

use super::mirrored_buffer::MirroredBuffer;
use super::{CancelToken, ProcessingError, ProcessingErrorKind};
use std::borrow::Cow;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

const COMPLETED: u64 = u64::MAX;
const WAIT_SLICE: Duration = Duration::from_millis(50);

pub struct BlockStream {
    buffer: Arc<MirroredBuffer>,
    producer: AtomicU64,
    consumers: Vec<AtomicU64>,
    consumer_active: Vec<AtomicBool>,
    consumer_completed: Vec<AtomicBool>,
    production_completed: AtomicBool,
    source_length: u64,
    min_producer_read_length: usize,
    max_producer_read_length: usize,
    producer_lock: Mutex<()>,
    producer_cv: Condvar,
    consumer_lock: Mutex<()>,
    consumer_cv: Condvar,
    underrun_count: AtomicU32,
    overrun_count: AtomicU32,
    /// Bytes handed to each consumer (index 0 is the producer).
    bytes_progress: Vec<AtomicU64>,
    ct: CancelToken,
}

impl BlockStream {
    pub fn new(
        buffer: Arc<MirroredBuffer>,
        consumer_count: usize,
        source_length: u64,
        min_producer_read_length: usize,
        max_producer_read_length: usize,
        ct: CancelToken,
    ) -> Self {
        let len = buffer.len();
        Self {
            buffer,
            producer: AtomicU64::new(0),
            consumers: (0..consumer_count).map(|_| AtomicU64::new(0)).collect(),
            consumer_active: (0..consumer_count).map(|_| AtomicBool::new(true)).collect(),
            consumer_completed: (0..consumer_count).map(|_| AtomicBool::new(false)).collect(),
            production_completed: AtomicBool::new(false),
            source_length,
            min_producer_read_length: min_producer_read_length.clamp(1, len),
            max_producer_read_length: max_producer_read_length.clamp(1, len),
            producer_lock: Mutex::new(()),
            producer_cv: Condvar::new(),
            consumer_lock: Mutex::new(()),
            consumer_cv: Condvar::new(),
            underrun_count: AtomicU32::new(0),
            overrun_count: AtomicU32::new(0),
            bytes_progress: (0..=consumer_count).map(|_| AtomicU64::new(0)).collect(),
            ct,
        }
    }

    pub fn length(&self) -> u64 {
        self.source_length
    }
    pub fn buffer_length(&self) -> usize {
        self.buffer.len()
    }
    pub fn buffer_underrun_count(&self) -> u32 {
        self.underrun_count.load(Ordering::Relaxed)
    }
    pub fn buffer_overrun_count(&self) -> u32 {
        self.overrun_count.load(Ordering::Relaxed)
    }
    pub fn consumer_count(&self) -> usize {
        self.consumers.len()
    }
    pub fn is_production_completed(&self) -> bool {
        self.production_completed.load(Ordering::Acquire)
    }
    /// Bytes produced so far (index 0) or consumed by reader `index - 1`.
    pub fn progress_bytes(&self, index: usize) -> u64 {
        self.bytes_progress[index].load(Ordering::Relaxed)
    }
    pub fn is_consumer_active(&self, index: usize) -> bool {
        self.consumer_active[index].load(Ordering::Relaxed)
    }
    pub fn is_consumer_completed(&self, index: usize) -> bool {
        self.consumer_completed[index].load(Ordering::Acquire)
    }
    pub(crate) fn set_consumer_active(&self, index: usize, active: bool) {
        self.consumer_active[index].store(active, Ordering::Relaxed);
    }

    fn min_consumer_position(&self) -> u64 {
        self.consumers.iter().map(|c| c.load(Ordering::Acquire)).min().unwrap_or(COMPLETED)
    }

    fn producer_can_write(&self) -> usize {
        let producer = self.producer.load(Ordering::Acquire);
        let last = self.min_consumer_position();
        let pending = if last == COMPLETED { 0 } else { producer.saturating_sub(last) };
        self.buffer.len().saturating_sub(pending as usize)
    }

    fn consumer_reached_end(&self, index: usize) -> bool {
        self.is_production_completed() && self.consumers[index].load(Ordering::Acquire) == self.producer.load(Ordering::Acquire)
    }

    /// Run the producer loop on the current thread until the source is exhausted.
    pub fn produce<R: Read + ?Sized>(&self, source: &mut R) -> Result<(), ProcessingError> {
        while !self.is_production_completed() {
            let mut writable = self.producer_can_write();
            if writable < self.min_producer_read_length {
                let mut guard = self.producer_lock.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    writable = self.producer_can_write();
                    if self.is_production_completed() || writable >= self.min_producer_read_length {
                        break;
                    }
                    let (g, _) = self.producer_cv.wait_timeout(guard, WAIT_SLICE).unwrap_or_else(|e| e.into_inner());
                    guard = g;
                    self.ct.check()?;
                    self.overrun_count.fetch_add(1, Ordering::Relaxed);
                }
                drop(guard);
                if self.is_production_completed() {
                    break;
                }
            }
            // Limit read chunk length (avoids bouncing between buffer underrun and overrun).
            let want = writable.min(self.max_producer_read_length);
            let producer = self.producer.load(Ordering::Acquire);
            let offset = (producer % self.buffer.len() as u64) as usize;
            // SAFETY: the region [producer, producer + want) is free by the protocol invariant.
            let target = unsafe { self.buffer.slice_mut(offset, want) };
            // The non-mirrored backing truncates at the physical end of the buffer.
            let want = target.len();
            let mut read_total = 0usize;
            while read_total < target.len() {
                match source.read(&mut target[read_total..]) {
                    Ok(0) => break,
                    Ok(n) => read_total += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => {
                        self.production_completed.store(true, Ordering::Release);
                        self.notify_consumers();
                        return Err(ProcessingError::io(&e));
                    }
                }
            }
            self.producer.fetch_add(read_total as u64, Ordering::AcqRel);
            self.bytes_progress[0].fetch_add(read_total as u64, Ordering::Relaxed);
            if read_total != want {
                self.production_completed.store(true, Ordering::Release);
            }
            self.notify_consumers();
        }
        Ok(())
    }

    fn notify_consumers(&self) {
        let _g = self.consumer_lock.lock().unwrap_or_else(|e| e.into_inner());
        self.consumer_cv.notify_all();
    }

    fn notify_producer(&self) {
        let _g = self.producer_lock.lock().unwrap_or_else(|e| e.into_inner());
        self.producer_cv.notify_one();
    }

    /// Return a block of at least `min_block_length` bytes for consumer `index`, or fewer when
    /// the stream is exhausted.
    pub fn get_block(&self, index: usize, min_block_length: usize) -> Result<Cow<'_, [u8]>, ProcessingError> {
        let min_block_length = min_block_length.min(self.buffer.len());
        let consumer = self.consumers[index].load(Ordering::Acquire);
        let mut available = self.producer.load(Ordering::Acquire).saturating_sub(consumer) as usize;
        if available < min_block_length {
            if available == 0 && self.consumer_reached_end(index) && self.source_length != 0 {
                return Err(ProcessingError::other("Cannot read block when EOS is reached"));
            }
            let mut guard = self.consumer_lock.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                available = self.producer.load(Ordering::Acquire).saturating_sub(consumer) as usize;
                if available >= min_block_length {
                    break;
                }
                if self.is_production_completed() {
                    // Re-read: the producer may have loaded the last data in between.
                    available = self.producer.load(Ordering::Acquire).saturating_sub(consumer) as usize;
                    break;
                }
                let (g, _) = self.consumer_cv.wait_timeout(guard, WAIT_SLICE).unwrap_or_else(|e| e.into_inner());
                guard = g;
                self.ct.check()?;
                self.underrun_count.fetch_add(1, Ordering::Relaxed);
            }
        }
        let offset = (consumer % self.buffer.len() as u64) as usize;
        // SAFETY: [consumer, consumer + available) has been written by the producer and will not
        // be overwritten until this consumer advances past it.
        Ok(unsafe { self.buffer.slice(offset, available) })
    }

    /// Advance consumer `index` by `length` bytes. Returns `false` once the consumer has reached
    /// the end of the stream.
    pub fn advance(&self, index: usize, length: usize) -> bool {
        self.consumers[index].fetch_add(length as u64, Ordering::AcqRel);
        self.bytes_progress[index + 1].fetch_add(length as u64, Ordering::Relaxed);
        self.notify_producer();
        !self.consumer_reached_end(index)
    }

    pub fn complete_consumption(&self, index: usize) {
        self.consumers[index].store(COMPLETED, Ordering::Release);
        self.consumer_completed[index].store(true, Ordering::Release);
        self.notify_producer();
    }
}

/// A single consumer's restricted view onto a [`BlockStream`].
pub struct BlockStreamReader {
    stream: Arc<BlockStream>,
    index: usize,
    bytes_read: u64,
    completed: bool,
    buffer_length: usize,
    suggested_read_length: usize,
    max_read_length: usize,
}

impl BlockStreamReader {
    pub fn new(stream: Arc<BlockStream>, index: usize) -> Self {
        let buffer_length = stream.buffer_length();
        let max_read_length = buffer_length / 2;
        Self {
            stream,
            index,
            bytes_read: 0,
            completed: false,
            buffer_length,
            suggested_read_length: max_read_length / 2,
            max_read_length,
        }
    }

    pub fn index(&self) -> usize {
        self.index
    }
    pub fn length(&self) -> u64 {
        self.stream.length()
    }
    pub fn bytes_read(&self) -> u64 {
        self.bytes_read
    }
    pub fn completed(&self) -> bool {
        self.completed
    }
    pub fn buffer_length(&self) -> usize {
        self.buffer_length
    }
    pub fn suggested_read_length(&self) -> usize {
        self.suggested_read_length
    }
    pub fn max_read_length(&self) -> usize {
        self.max_read_length
    }
    pub fn stream(&self) -> &Arc<BlockStream> {
        &self.stream
    }

    /// Mark this consumer as no longer actively consuming (progress display only).
    pub fn set_active(&self, active: bool) {
        self.stream.set_consumer_active(self.index, active);
    }

    #[inline]
    pub fn get_block(&self, min_block_length: usize) -> Result<Cow<'_, [u8]>, ProcessingError> {
        self.stream.get_block(self.index, min_block_length)
    }

    #[inline]
    pub fn advance(&mut self, length: usize) -> bool {
        self.bytes_read += length as u64;
        self.stream.advance(self.index, length)
    }

    pub fn complete(&mut self) {
        self.stream.complete_consumption(self.index);
        self.completed = true;
    }

    /// Skip `length` bytes, blocking as needed. Returns the number of bytes actually skipped
    /// (fewer only at the end of the stream).
    pub fn skip(&mut self, mut length: u64) -> Result<u64, ProcessingError> {
        let mut skipped = 0u64;
        while length > 0 {
            let want = length.min(self.suggested_read_length as u64) as usize;
            let block_len = self.get_block(want)?.len();
            let step = block_len.min(length as usize);
            if step == 0 {
                break;
            }
            self.advance(step);
            skipped += step as u64;
            length -= step as u64;
        }
        Ok(skipped)
    }
}

impl ProcessingError {
    pub fn consumer(message: impl Into<String>) -> Self {
        Self::new(ProcessingErrorKind::Consumer, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn run_pipeline(data: Vec<u8>, buffer_len: usize, consumer_count: usize) -> Vec<Vec<u8>> {
        let buffer = Arc::new(MirroredBuffer::new(buffer_len).unwrap());
        let stream = Arc::new(BlockStream::new(buffer, consumer_count, data.len() as u64, 16, 1024, CancelToken::new()));
        let mut outputs = Vec::new();
        std::thread::scope(|s| {
            let producer_stream = Arc::clone(&stream);
            let mut source = Cursor::new(data.clone());
            s.spawn(move || producer_stream.produce(&mut source).unwrap());
            let mut handles = Vec::new();
            for i in 0..consumer_count {
                let st = Arc::clone(&stream);
                handles.push(s.spawn(move || {
                    let mut reader = BlockStreamReader::new(st, i);
                    let mut out = Vec::new();
                    loop {
                        let block = reader.get_block(7 + i).unwrap().to_vec();
                        out.extend_from_slice(&block);
                        if !reader.advance(block.len()) || block.is_empty() {
                            break;
                        }
                    }
                    reader.complete();
                    out
                }));
            }
            for h in handles {
                outputs.push(h.join().unwrap());
            }
        });
        outputs
    }

    #[test]
    fn all_consumers_see_all_bytes() {
        let data: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect();
        for out in run_pipeline(data.clone(), 8192, 3) {
            assert_eq!(out, data);
        }
    }

    #[test]
    fn empty_source() {
        for out in run_pipeline(Vec::new(), 4096, 2) {
            assert!(out.is_empty());
        }
    }

    #[test]
    fn source_smaller_than_buffer() {
        let data = vec![9u8; 100];
        for out in run_pipeline(data.clone(), 4096, 1) {
            assert_eq!(out, data);
        }
    }
}
