//! Block consumers: every consumer runs in its own thread and reads the shared stream through a
//! [`BlockStreamReader`].

pub mod copy_to_file;
pub mod data_source;
pub mod hash_calculator;
pub mod matroska;
pub mod mp4;
pub mod ogg;

pub use copy_to_file::CopyToFileConsumer;
pub use hash_calculator::HashCalculator;

use super::block_stream::BlockStreamReader;
use super::{CancelToken, ProcessingError};
use crate::hashes;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

/// Something that consumes a stream of blocks.
pub trait BlockConsumer: Send {
    fn name(&self) -> &str;
    fn do_work(&mut self, reader: &mut BlockStreamReader, ct: &CancelToken) -> Result<(), ProcessingError>;
    fn as_any(&self) -> &dyn Any;
}

/// Runs a consumer to completion, attaching diagnostic data to any error and always marking the
/// reader as completed afterwards.
pub fn process_blocks(consumer: &mut dyn BlockConsumer, reader: &mut BlockStreamReader, ct: &CancelToken) -> Option<ProcessingError> {
    let result = consumer.do_work(reader, ct);
    reader.complete();
    result.err().map(|e| {
        e.with_data("BlockConsumerName", consumer.name().to_string())
            .with_data("BlockConsumerReadBytes", reader.bytes_read().to_string())
    })
}

/// What a factory receives when asked to create a consumer.
pub struct BlockConsumerSetup<'a> {
    pub name: &'a str,
    pub reader: &'a BlockStreamReader,
    /// Path of the file being processed (or a synthetic tag).
    pub tag: &'a str,
    /// Arguments supplied on the command line (`--Consumers=NAME:arg1|arg2`).
    pub arguments: &'a [String],
}

pub type CreateBlockConsumer = dyn Fn(&BlockConsumerSetup<'_>) -> Result<Box<dyn BlockConsumer>, ProcessingError> + Send + Sync;

/// Named factory for a block consumer.
#[derive(Clone)]
pub struct BlockConsumerFactory {
    pub name: String,
    pub description: String,
    create: Arc<CreateBlockConsumer>,
}

impl BlockConsumerFactory {
    pub fn new(name: &str, description: &str, create: impl Fn(&BlockConsumerSetup<'_>) -> Result<Box<dyn BlockConsumer>, ProcessingError> + Send + Sync + 'static) -> Self {
        Self { name: name.to_string(), description: description.to_string(), create: Arc::new(create) }
    }

    pub fn create(&self, setup: &BlockConsumerSetup<'_>) -> Result<Box<dyn BlockConsumer>, ProcessingError> {
        (self.create)(setup)
    }
}

/// Description text for the built-in consumers (from `Lang.resx`).
pub fn consumer_description(name: &str) -> &'static str {
    match name {
        "CPY" => "Copies the processed data to <Directory>/<FileName> (argument: target directory)",
        "CRC32" => "https://en.wikipedia.org/wiki/Cyclic_redundancy_check",
        "CRC32C" => "Castagnoli CRC32 Variant (0x1EDC6F41 Polynomial)",
        "ED2K" => "https://en.wikipedia.org/wiki/Ed2k_URI_scheme#eD2k_hash_algorithm",
        "KECCAK-224" | "KECCAK-256" | "KECCAK-384" | "KECCAK-512" => "https://en.wikipedia.org/wiki/SHA-3",
        "MD4" => "https://en.wikipedia.org/wiki/MD4",
        "MD5" => "https://en.wikipedia.org/wiki/MD5",
        "MKV" => "Consumes Matroska files to provide info for Reports",
        "MP4" => "Consumes MP4/ISO-BMFF files to provide info for Reports",
        "NULL" => "Consumes data without processing it",
        "OGG" => "Consumes OGG files to provide info for Reports",
        "SHA1" => "https://en.wikipedia.org/wiki/SHA-1",
        "SHA2-256" | "SHA2-384" | "SHA2-512" => "https://en.wikipedia.org/wiki/SHA-2",
        "SHA3-224" | "SHA3-256" | "SHA3-384" | "SHA3-512" => "https://en.wikipedia.org/wiki/SHA-3",
        "TIGER" => "https://en.wikipedia.org/wiki/Tiger_(hash_function)",
        "TTH" => "https://en.wikipedia.org/wiki/Merkle_tree#Tiger_tree_hash",
        _ => "<NoDescriptionGiven>",
    }
}

fn arg_at<'a>(setup: &'a BlockConsumerSetup<'_>, index: usize) -> Option<&'a str> {
    setup.arguments.get(index).map(|s| s.as_str())
}

fn hash_factory(name: &'static str, make: impl Fn(&BlockConsumerSetup<'_>) -> Result<Box<dyn hashes::AvdHash>, ProcessingError> + Send + Sync + 'static) -> BlockConsumerFactory {
    BlockConsumerFactory::new(name, consumer_description(name), move |s| {
        let hash = make(s)?;
        Ok(Box::new(HashCalculator::new(s.name, s.reader, hash)?))
    })
}

/// All built-in consumers, sorted by name.
pub fn default_block_consumer_factories() -> Vec<BlockConsumerFactory> {
    use hashes::rustcrypto::*;
    let mut factories: HashMap<String, BlockConsumerFactory> = HashMap::new();
    let mut add = |f: BlockConsumerFactory| {
        factories.insert(f.name.clone(), f);
    };

    add(hash_factory("NULL", |s| {
        let mib: usize = arg_at(s, 0).unwrap_or("4").trim().parse().map_err(|_| ProcessingError::other("NULL: invalid block size argument"))?;
        Ok(Box::new(hashes::null::NullHash::new(mib << 20)))
    }));
    add(BlockConsumerFactory::new("CPY", consumer_description("CPY"), |s| {
        let dir = arg_at(s, 0).ok_or_else(|| ProcessingError::other("CPY consumer requires a target directory argument (--Consumers=CPY:<Directory>)"))?;
        let file_name = crate::misc::file_name(std::path::Path::new(s.tag));
        let target = std::path::Path::new(dir).join(file_name);
        Ok(Box::new(CopyToFileConsumer::new(s.name, target)))
    }));
    add(hash_factory("MD5", |_| Ok(Box::new(Md5Hash::new()))));
    add(hash_factory("SHA1", |_| Ok(Box::new(Sha1Hash::new()))));
    add(hash_factory("SHA2-256", |_| Ok(Box::new(Sha256Hash::new()))));
    add(hash_factory("SHA2-384", |_| Ok(Box::new(Sha384Hash::new()))));
    add(hash_factory("SHA2-512", |_| Ok(Box::new(Sha512Hash::new()))));
    add(hash_factory("MD4", |_| Ok(Box::new(Md4Hash::new()))));
    add(hash_factory("ED2K", |_| Ok(Box::new(hashes::ed2k::Ed2kHash::new()))));
    add(hash_factory("CRC32", |_| Ok(Box::new(hashes::crc32::Crc32Hash::new()))));
    add(hash_factory("CRC32C", |_| Ok(Box::new(hashes::crc32::Crc32CHash::new()))));
    add(hash_factory("SHA3-224", |_| Ok(Box::new(Sha3Hash::new(224)))));
    add(hash_factory("SHA3-256", |_| Ok(Box::new(Sha3Hash::new(256)))));
    add(hash_factory("SHA3-384", |_| Ok(Box::new(Sha3Hash::new(384)))));
    add(hash_factory("SHA3-512", |_| Ok(Box::new(Sha3Hash::new(512)))));
    add(hash_factory("KECCAK-224", |_| Ok(Box::new(KeccakHash::new(224)))));
    add(hash_factory("KECCAK-256", |_| Ok(Box::new(KeccakHash::new(256)))));
    add(hash_factory("KECCAK-384", |_| Ok(Box::new(KeccakHash::new(384)))));
    add(hash_factory("KECCAK-512", |_| Ok(Box::new(KeccakHash::new(512)))));
    add(hash_factory("TIGER", |_| Ok(Box::new(TigerHash::new()))));
    add(hash_factory("TTH", |s| {
        let default_threads = crate::misc::processor_count().min(4);
        let threads: usize = match arg_at(s, 0) {
            Some(a) => a.trim().parse().map_err(|_| ProcessingError::other("TTH: invalid thread count argument"))?,
            None => default_threads,
        };
        Ok(Box::new(hashes::tth::TigerTreeHash::new(threads)))
    }));
    add(BlockConsumerFactory::new("MKV", consumer_description("MKV"), |s| Ok(Box::new(matroska::MatroskaParser::new(s.name)))));
    add(BlockConsumerFactory::new("OGG", consumer_description("OGG"), |s| Ok(Box::new(ogg::OggParser::new(s.name)))));
    add(BlockConsumerFactory::new("MP4", consumer_description("MP4"), |s| Ok(Box::new(mp4::Mp4Parser::new(s.name)))));

    let mut list: Vec<BlockConsumerFactory> = factories.into_values().collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}
