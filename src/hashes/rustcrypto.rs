//! Adapters for the RustCrypto `Digest` implementations (MD5, SHA-1, SHA-2, MD4, SHA-3, Keccak, Tiger).

use super::{full_blocks, AvdHash};
use digest::Digest;

/// Generic adapter turning any `Digest` into an [`AvdHash`] with a chosen block size.
pub struct DigestHash<D: Digest + Send + Clone> {
    digest: D,
    block_size: usize,
}

impl<D: Digest + Send + Clone> DigestHash<D> {
    pub fn with_block_size(block_size: usize) -> Self {
        Self { digest: D::new(), block_size: block_size.max(1) }
    }
}

impl<D: Digest + Send + Clone> AvdHash for DigestHash<D> {
    fn block_size(&self) -> usize {
        self.block_size
    }
    fn initialize(&mut self) {
        self.digest = D::new();
    }
    fn transform_full_blocks(&mut self, data: &[u8]) -> usize {
        full_blocks(self.block_size, data, |d| self.digest.update(d))
    }
    fn transform_final_block(&mut self, data: &[u8]) -> Vec<u8> {
        self.digest.update(data);
        let out = std::mem::replace(&mut self.digest, D::new()).finalize();
        out.to_vec()
    }
}

macro_rules! digest_alias {
    ($name:ident, $digest:ty, $block:expr) => {
        pub struct $name(DigestHash<$digest>);
        impl $name {
            pub fn new() -> Self {
                Self(DigestHash::with_block_size($block))
            }
        }
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
        impl AvdHash for $name {
            fn block_size(&self) -> usize {
                self.0.block_size()
            }
            fn initialize(&mut self) {
                self.0.initialize()
            }
            fn transform_full_blocks(&mut self, data: &[u8]) -> usize {
                self.0.transform_full_blocks(data)
            }
            fn transform_final_block(&mut self, data: &[u8]) -> Vec<u8> {
                self.0.transform_final_block(data)
            }
        }
    };
}

digest_alias!(Md5Hash, md5::Md5, 1024);
digest_alias!(Sha1Hash, sha1::Sha1, 1024);
digest_alias!(Sha256Hash, sha2::Sha256, 1024);
digest_alias!(Sha384Hash, sha2::Sha384, 1024);
digest_alias!(Sha512Hash, sha2::Sha512, 1024);
digest_alias!(Md4Hash, md4::Md4, 64);
digest_alias!(TigerHash, tiger::Tiger, 64);

/// SHA-3 with a selectable output length (224/256/384/512 bits).
pub enum Sha3Hash {
    B224(DigestHash<sha3::Sha3_224>),
    B256(DigestHash<sha3::Sha3_256>),
    B384(DigestHash<sha3::Sha3_384>),
    B512(DigestHash<sha3::Sha3_512>),
}

impl Sha3Hash {
    pub fn new(bits: u32) -> Self {
        match bits {
            224 => Self::B224(DigestHash::with_block_size(144)),
            256 => Self::B256(DigestHash::with_block_size(136)),
            384 => Self::B384(DigestHash::with_block_size(104)),
            _ => Self::B512(DigestHash::with_block_size(72)),
        }
    }
    fn inner(&mut self) -> &mut dyn AvdHash {
        match self {
            Self::B224(h) => h,
            Self::B256(h) => h,
            Self::B384(h) => h,
            Self::B512(h) => h,
        }
    }
    fn inner_ref(&self) -> &dyn AvdHash {
        match self {
            Self::B224(h) => h,
            Self::B256(h) => h,
            Self::B384(h) => h,
            Self::B512(h) => h,
        }
    }
}

impl AvdHash for Sha3Hash {
    fn block_size(&self) -> usize {
        self.inner_ref().block_size()
    }
    fn initialize(&mut self) {
        self.inner().initialize()
    }
    fn transform_full_blocks(&mut self, data: &[u8]) -> usize {
        self.inner().transform_full_blocks(data)
    }
    fn transform_final_block(&mut self, data: &[u8]) -> Vec<u8> {
        self.inner().transform_final_block(data)
    }
}

/// Original Keccak (pre-FIPS202 padding) with a selectable output length.
pub enum KeccakHash {
    B224(DigestHash<sha3::Keccak224>),
    B256(DigestHash<sha3::Keccak256>),
    B384(DigestHash<sha3::Keccak384>),
    B512(DigestHash<sha3::Keccak512>),
}

impl KeccakHash {
    pub fn new(bits: u32) -> Self {
        match bits {
            224 => Self::B224(DigestHash::with_block_size(144)),
            256 => Self::B256(DigestHash::with_block_size(136)),
            384 => Self::B384(DigestHash::with_block_size(104)),
            _ => Self::B512(DigestHash::with_block_size(72)),
        }
    }
    fn inner(&mut self) -> &mut dyn AvdHash {
        match self {
            Self::B224(h) => h,
            Self::B256(h) => h,
            Self::B384(h) => h,
            Self::B512(h) => h,
        }
    }
    fn inner_ref(&self) -> &dyn AvdHash {
        match self {
            Self::B224(h) => h,
            Self::B256(h) => h,
            Self::B384(h) => h,
            Self::B512(h) => h,
        }
    }
}

impl AvdHash for KeccakHash {
    fn block_size(&self) -> usize {
        self.inner_ref().block_size()
    }
    fn initialize(&mut self) {
        self.inner().initialize()
    }
    fn transform_full_blocks(&mut self, data: &[u8]) -> usize {
        self.inner().transform_full_blocks(data)
    }
    fn transform_final_block(&mut self, data: &[u8]) -> Vec<u8> {
        self.inner().transform_final_block(data)
    }
}
