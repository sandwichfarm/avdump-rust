//! CRC32 (IEEE 802.3, reflected 0xEDB88320) and CRC32C (Castagnoli).

use super::{full_blocks, AvdHash};

pub struct Crc32Hash {
    state: crc32fast::Hasher,
}

impl Crc32Hash {
    pub fn new() -> Self {
        Self { state: crc32fast::Hasher::new() }
    }
}

impl Default for Crc32Hash {
    fn default() -> Self {
        Self::new()
    }
}

impl AvdHash for Crc32Hash {
    fn block_size(&self) -> usize {
        1
    }
    fn initialize(&mut self) {
        self.state = crc32fast::Hasher::new();
    }
    fn transform_full_blocks(&mut self, data: &[u8]) -> usize {
        full_blocks(1, data, |d| self.state.update(d))
    }
    fn transform_final_block(&mut self, data: &[u8]) -> Vec<u8> {
        self.state.update(data);
        let crc = std::mem::replace(&mut self.state, crc32fast::Hasher::new()).finalize();
        crc.to_be_bytes().to_vec()
    }
}

pub struct Crc32CHash {
    crc: u32,
}

impl Crc32CHash {
    pub fn new() -> Self {
        Self { crc: 0 }
    }
}

impl Default for Crc32CHash {
    fn default() -> Self {
        Self::new()
    }
}

impl AvdHash for Crc32CHash {
    fn block_size(&self) -> usize {
        1
    }
    fn initialize(&mut self) {
        self.crc = 0;
    }
    fn transform_full_blocks(&mut self, data: &[u8]) -> usize {
        full_blocks(1, data, |d| self.crc = crc32c::crc32c_append(self.crc, d))
    }
    fn transform_final_block(&mut self, data: &[u8]) -> Vec<u8> {
        self.crc = crc32c::crc32c_append(self.crc, data);
        let out = self.crc.to_be_bytes().to_vec();
        self.crc = 0;
        out
    }
}
