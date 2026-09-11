//! Hash algorithms consumed by the [`HashCalculator`](crate::processing::consumers::HashCalculator).
//!
//! Every algorithm implements [`AvdHash`], which mirrors the `IAVDHashAlgorithm` contract of the
//! original AVDump3: data is fed in multiples of `block_size()` through `transform_full_blocks`,
//! the trailing partial block goes through `transform_final_block`, which yields the digest.

pub mod crc32;
pub mod ed2k;
pub mod null;
pub mod rustcrypto;
pub mod tth;

use std::fmt;

/// Incremental, block-oriented hash interface used by the processing pipeline.
pub trait AvdHash: Send {
    /// Preferred block granularity. `transform_full_blocks` only consumes whole multiples of it.
    fn block_size(&self) -> usize;
    /// Reset all internal state so the instance can hash a new stream.
    fn initialize(&mut self);
    /// Hash as many whole blocks as `data` contains, returning the number of bytes consumed.
    fn transform_full_blocks(&mut self, data: &[u8]) -> usize;
    /// Hash the trailing bytes (shorter than one block) and return the final digest.
    fn transform_final_block(&mut self, data: &[u8]) -> Vec<u8>;
    /// Secondary digests produced by the algorithm (e.g. the ED2K "blue" hash).
    fn additional_hashes(&self) -> Vec<Vec<u8>> {
        Vec::new()
    }
}

/// Default implementation of `transform_full_blocks` for algorithms that accept arbitrary
/// byte sequences.
pub(crate) fn full_blocks(block_size: usize, data: &[u8], mut feed: impl FnMut(&[u8])) -> usize {
    let usable = (data.len() / block_size) * block_size;
    if usable > 0 {
        feed(&data[..usable]);
    }
    usable
}

/// Error raised when a consumer name is unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownHash(pub String);

impl fmt::Display for UnknownHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown hash algorithm: {}", self.0)
    }
}

impl std::error::Error for UnknownHash {}

/// Convenience: hash a complete byte slice with the given algorithm in one go.
pub fn hash_all(hash: &mut dyn AvdHash, data: &[u8]) -> Vec<u8> {
    hash.initialize();
    let processed = hash.transform_full_blocks(data);
    hash.transform_final_block(&data[processed..])
}

/// Lower-case hexadecimal rendering of a digest.
pub fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// Upper-case hexadecimal rendering of a digest (matches `BitConverter.ToString`).
pub fn to_hex_upper(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02X}", b));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(name: &str, len: usize) -> Vec<u8> {
        match name {
            "BinaryZeros" => vec![0u8; len],
            "BinaryOnes" => vec![0xFFu8; len],
            "ASCIIZeros" => vec![b'0'; len],
            _ => vec![0u8; len],
        }
    }

    /// Hashes `data` exactly the way the original xunit test does: one block at a time.
    fn hash_blockwise(hash: &mut dyn AvdHash, data: &[u8]) -> (String, Option<String>) {
        hash.initialize();
        let bs = hash.block_size();
        let mut processed = 0usize;
        while processed + bs <= data.len() {
            hash.transform_full_blocks(&data[processed..processed + bs]);
            processed += bs;
        }
        let rest = data.len() % bs;
        let digest = hash.transform_final_block(&data[data.len() - rest..]);
        let extra = hash.additional_hashes().first().map(|h| to_hex(h));
        (to_hex(&digest), extra)
    }

    /// Vectors from `AVDump3Tests/HashTestVectors.xml`.
    #[test]
    fn ed2k_vectors_from_original_suite() {
        let vectors: &[(&str, usize, &str, Option<&str>)] = &[
            ("BinaryZeros", 0, "31d6cfe0d16ae931b73c59d7e0c089c0", None),
            ("BinaryZeros", 1, "47c61a0fa8738ba77308a8a600f88e4b", None),
            ("BinaryZeros", 9727999, "ac44b93fc9aff773ab0005c911f8396f", None),
            ("BinaryZeros", 9728000, "fc21d9af828f92a8df64beac3357425d", Some("d7def262a127cd79096a108e7a9fc138")),
            ("BinaryZeros", 9728001, "06329e9dba1373512c06386fe29e3c65", None),
            ("BinaryZeros", 19455999, "a4aed104a077de7e4210e7f5b131fe25", None),
            ("BinaryZeros", 19456000, "114b21c63a74b6ca922291a11177dd5c", Some("194ee9e4fa79b2ee9f8829284c466051")),
            ("BinaryZeros", 19456001, "e57f824d28f69fe90864e17673668457", None),
        ];
        for (pat, len, expected, expected2) in vectors {
            let data = pattern(pat, *len);
            let mut h = ed2k::Ed2kHash::new();
            let (got, got2) = hash_blockwise(&mut h, &data);
            assert_eq!(&got, expected, "ED2K {pat} {len}");
            assert_eq!(got2.as_deref(), *expected2, "ED2K2 {pat} {len}");
        }
    }

    #[test]
    fn standard_vectors() {
        use rustcrypto::*;
        let abc = b"abc";
        let empty = b"";
        let check = |mut h: Box<dyn AvdHash>, data: &[u8], expected: &str| {
            let (got, _) = hash_blockwise(h.as_mut(), data);
            assert_eq!(got, expected);
        };
        check(Box::new(Md5Hash::new()), abc, "900150983cd24fb0d6963f7d28e17f72");
        check(Box::new(Sha1Hash::new()), abc, "a9993e364706816aba3e25717850c26c9cd0d89d");
        check(Box::new(Sha256Hash::new()), abc, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        check(Box::new(Sha384Hash::new()), abc, "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7");
        check(Box::new(Sha512Hash::new()), abc, "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f");
        check(Box::new(Md4Hash::new()), abc, "a448017aaf21d8525fc10ae87aa6729d");
        check(Box::new(Md4Hash::new()), empty, "31d6cfe0d16ae931b73c59d7e0c089c0");
        check(Box::new(Sha3Hash::new(224)), abc, "e642824c3f8cf24ad09234ee7d3c766fc9a3a5168d0c94ad73b46fdf");
        check(Box::new(Sha3Hash::new(256)), abc, "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532");
        check(Box::new(Sha3Hash::new(384)), abc, "ec01498288516fc926459f58e2c6ad8df9b473cb0fc08c2596da7cf0e49be4b298d88cea927ac7f539f1edf228376d25");
        check(Box::new(Sha3Hash::new(512)), abc, "b751850b1a57168a5693cd924b6b096e08f621827444f70d884f5d0240d2712e10e116e9192af3c91a7ec57647e3934057340b4cf408d5a56592f8274eec53f0");
        check(Box::new(KeccakHash::new(256)), empty, "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470");
        check(Box::new(KeccakHash::new(224)), empty, "f71837502ba8e10837bdd8d365adb85591895602fc552b48b7390abd");
        check(Box::new(KeccakHash::new(384)), empty, "2c23146a63a29acf99e73b88f8c24eaa7dc60aa771780ccc006afbfa8fe2479b2dd2b21362337441ac12b515911957ff");
        check(Box::new(KeccakHash::new(512)), empty, "0eab42de4c3ceb9235fc91acffe746b29c29a8c366b7c60e4e67c466f36a4304c00fa9caf9d87976ba469bcbe06713b435f091ef2769fb160cdab33d3670680e");
        check(Box::new(TigerHash::new()), empty, "3293ac630c13f0245f92bbb1766e16167a4e58492dde73f3");
        check(Box::new(TigerHash::new()), abc, "2aab1484e8c158f2bfb8c5ff41b57a525129131c957b5f93");
        check(Box::new(crc32::Crc32Hash::new()), b"123456789", "cbf43926");
        check(Box::new(crc32::Crc32CHash::new()), b"123456789", "e3069283");
        check(Box::new(crc32::Crc32Hash::new()), empty, "00000000");
    }

    #[test]
    fn tth_vectors() {
        // Reference values from rhash / DC++.
        let check = |threads: usize, data: &[u8], expected_b32: &str| {
            let mut h = tth::TigerTreeHash::new(threads);
            let (got, _) = hash_blockwise(&mut h, data);
            let bytes = (0..got.len()).step_by(2).map(|i| u8::from_str_radix(&got[i..i + 2], 16).unwrap()).collect::<Vec<_>>();
            let b32 = crate::misc::base_convert::rfc4648_base32(&bytes);
            assert_eq!(b32, expected_b32, "threads={threads} len={}", data.len());
        };
        check(1, b"", "LWPNACQDBZRYXW3VHJVCJ64QBZNGHOHHHZWCLNQ");
        check(1, b"abc", "ASD4UJSEH5M47PDYB46KBTSQTSGDKLBHYXOMUIA");
        check(3, b"abc", "ASD4UJSEH5M47PDYB46KBTSQTSGDKLBHYXOMUIA");
        check(1, &vec![b'A'; 1024], "L66Q4YVNAFWVS23X2HJIRA5ZJ7WXR3F26RSASFA");
        check(1, &vec![b'A'; 1025], "PZMRYHGY6LTBEH63ZWAHDORHSYTLO4LEFUIKHWY");
        check(2, &vec![0u8; 1 << 20], "MUACEID6UTVUKTRE2MTZKOPTZTMS6A2OF6B4ZNY");
        check(4, &vec![0u8; (1 << 20) + 777], "GTMUI3C5NS42NYNRDZC7546KFUK23ZG3IULUPCA");
        check(1, &vec![0u8; 20 << 20], "K2XV3JDRR53NGXIIUHORTULT5NOUPNTLPFQA7YI");
    }
}
