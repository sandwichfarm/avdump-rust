//! eD2k hash: MD4 over 9 728 000-byte chunks, then MD4 over the chunk digests.
//!
//! Two variants exist when the file size is an exact multiple of the chunk size:
//! the "red" hash appends an extra MD4 of zero bytes (returned as the primary digest) and the
//! "blue" hash omits it (exposed as the first additional hash, reported as `ED2K2`).

use super::AvdHash;
use digest::Digest;

pub const ED2K_BLOCK_SIZE: usize = 9_728_000;

pub struct Ed2kHash {
    block_hashes: Vec<u8>,
    blue_is_red: bool,
    red: Vec<u8>,
    blue: Vec<u8>,
    additional: Vec<Vec<u8>>,
}

impl Ed2kHash {
    pub fn new() -> Self {
        Self {
            block_hashes: Vec::with_capacity(16 * 512),
            blue_is_red: false,
            red: Vec::new(),
            blue: Vec::new(),
            additional: Vec::new(),
        }
    }

    pub fn red_hash(&self) -> &[u8] {
        &self.red
    }
    pub fn blue_hash(&self) -> &[u8] {
        &self.blue
    }
    pub fn blue_is_red(&self) -> bool {
        self.blue_is_red
    }

    fn md4(data: &[u8]) -> [u8; 16] {
        let mut out = [0u8; 16];
        out.copy_from_slice(&md4::Md4::digest(data));
        out
    }
}

impl Default for Ed2kHash {
    fn default() -> Self {
        Self::new()
    }
}

impl AvdHash for Ed2kHash {
    fn block_size(&self) -> usize {
        ED2K_BLOCK_SIZE
    }

    fn initialize(&mut self) {
        self.block_hashes.clear();
        self.blue_is_red = false;
        self.red.clear();
        self.blue.clear();
        self.additional.clear();
    }

    fn transform_full_blocks(&mut self, data: &[u8]) -> usize {
        let usable = (data.len() / ED2K_BLOCK_SIZE) * ED2K_BLOCK_SIZE;
        for chunk in data[..usable].chunks_exact(ED2K_BLOCK_SIZE) {
            self.block_hashes.extend_from_slice(&Self::md4(chunk));
        }
        usable
    }

    fn transform_final_block(&mut self, data: &[u8]) -> Vec<u8> {
        let hash: Vec<u8>;
        if self.block_hashes.is_empty() {
            hash = Self::md4(data).to_vec();
            self.red = hash.clone();
            self.blue = hash.clone();
            self.blue_is_red = true;
        } else if !data.is_empty() {
            // Data is not a multiple of the block length (common case).
            self.block_hashes.extend_from_slice(&Self::md4(data));
            hash = Self::md4(&self.block_hashes).to_vec();
            self.red = hash.clone();
            self.blue = hash.clone();
            self.blue_is_red = true;
        } else {
            let without_null = if self.block_hashes.len() == 16 {
                self.block_hashes.clone()
            } else {
                Self::md4(&self.block_hashes).to_vec()
            };
            self.block_hashes.extend_from_slice(&Self::md4(&[]));
            let with_null = Self::md4(&self.block_hashes).to_vec();
            self.blue = without_null;
            self.red = with_null.clone();
            self.additional.push(self.blue.clone());
            hash = with_null;
        }
        hash
    }

    fn additional_hashes(&self) -> Vec<Vec<u8>> {
        self.additional.clone()
    }
}
