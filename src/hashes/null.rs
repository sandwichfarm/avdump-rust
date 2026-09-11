//! `NULL` consumer: reads data without processing it (throughput baseline).

use super::AvdHash;

pub struct NullHash {
    block_size: usize,
}

impl NullHash {
    /// `block_size` is the read granularity requested from the stream (default 4 MiB).
    pub fn new(block_size: usize) -> Self {
        Self { block_size: block_size.max(1) }
    }
}

impl AvdHash for NullHash {
    fn block_size(&self) -> usize {
        self.block_size
    }
    fn initialize(&mut self) {}
    fn transform_full_blocks(&mut self, data: &[u8]) -> usize {
        (data.len() / self.block_size) * self.block_size
    }
    fn transform_final_block(&mut self, _data: &[u8]) -> Vec<u8> {
        Vec::new()
    }
}
