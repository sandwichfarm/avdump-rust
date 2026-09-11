//! Feeds stream blocks into an [`AvdHash`].

use super::BlockConsumer;
use crate::hashes::AvdHash;
use crate::processing::{BlockStreamReader, CancelToken, ProcessingError};
use std::any::Any;

pub struct HashCalculator {
    name: String,
    read_length: usize,
    hash: Box<dyn AvdHash>,
    hash_value: Vec<u8>,
    additional_hash_values: Vec<Vec<u8>>,
}

impl HashCalculator {
    pub fn new(name: &str, reader: &BlockStreamReader, hash: Box<dyn AvdHash>) -> Result<Self, ProcessingError> {
        let block_size = hash.block_size().max(1);
        let mut length = ((reader.suggested_read_length() / block_size) + 1) * block_size;
        if length > reader.max_read_length() {
            length -= block_size;
            if length == 0 {
                return Err(ProcessingError::other("Min/Max BlockLength too restrictive")
                    .with_data("TransformName", name.to_string())
                    .with_data("MaxBlockLength", reader.max_read_length().to_string())
                    .with_data("HashBlockLength", block_size.to_string()));
            }
        }
        Ok(Self { name: name.to_string(), read_length: length, hash, hash_value: Vec::new(), additional_hash_values: Vec::new() })
    }

    pub fn read_length(&self) -> usize {
        self.read_length
    }
    pub fn hash_value(&self) -> &[u8] {
        &self.hash_value
    }
    pub fn additional_hash_values(&self) -> &[Vec<u8>] {
        &self.additional_hash_values
    }
}

impl BlockConsumer for HashCalculator {
    fn name(&self) -> &str {
        &self.name
    }

    fn do_work(&mut self, reader: &mut BlockStreamReader, ct: &CancelToken) -> Result<(), ProcessingError> {
        self.hash.initialize();
        let mut remainder: Vec<u8>;
        loop {
            ct.check()?;
            let block = reader.get_block(self.read_length)?;
            let processed = self.hash.transform_full_blocks(&block);
            remainder = block[processed..].to_vec();
            let more = reader.advance(processed);
            if !more || processed == 0 {
                break;
            }
        }
        self.hash_value = self.hash.transform_final_block(&remainder);
        self.additional_hash_values = self.hash.additional_hashes();
        reader.advance(remainder.len());
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
