//! Tiger Tree Hash (Merkle tree over 1024-byte leaves, Tiger as node function).
//!
//! Leaf hashes are computed in parallel with `thread_count` worker threads on every
//! `transform_full_blocks` call (up to 16 MiB per call), then folded into the tree using the
//! same "two hashes per level" bookkeeping as the original implementation.

use super::AvdHash;
use digest::Digest;
use tiger::Tiger;

pub const LEAF_SIZE: usize = 1024;
const HASH_LEN: usize = 24;
const MAX_LEVELS: usize = 56;
const MAX_CHUNK: usize = 16 << 20;

pub struct TigerTreeHash {
    thread_count: usize,
    /// Leaf hashes of the current chunk that have not been folded yet.
    leaves: Vec<[u8; HASH_LEN]>,
    /// Two node slots per level; `node_mask` says which levels hold a pending node.
    nodes: Vec<[u8; HASH_LEN]>,
    node_mask: u64,
}

fn leaf_hash(data: &[u8]) -> [u8; HASH_LEN] {
    let mut t = Tiger::new();
    t.update([0u8]);
    t.update(data);
    let mut out = [0u8; HASH_LEN];
    out.copy_from_slice(&t.finalize());
    out
}

fn node_hash(left: &[u8; HASH_LEN], right: &[u8; HASH_LEN]) -> [u8; HASH_LEN] {
    let mut t = Tiger::new();
    t.update([1u8]);
    t.update(left);
    t.update(right);
    let mut out = [0u8; HASH_LEN];
    out.copy_from_slice(&t.finalize());
    out
}

impl TigerTreeHash {
    pub fn new(thread_count: usize) -> Self {
        Self {
            thread_count: thread_count.max(1),
            leaves: Vec::with_capacity(MAX_CHUNK / LEAF_SIZE),
            nodes: vec![[0u8; HASH_LEN]; MAX_LEVELS * 2],
            node_mask: 0,
        }
    }

    /// Push a finished leaf/node hash into level `level`, carrying upward like a binary counter.
    fn push_node(&mut self, mut hash: [u8; HASH_LEN], mut level: usize) {
        loop {
            let bit = 1u64 << level;
            if self.node_mask & bit == 0 {
                self.nodes[level * 2] = hash;
                self.node_mask |= bit;
                return;
            }
            let left = self.nodes[level * 2];
            hash = node_hash(&left, &hash);
            self.node_mask &= !bit;
            level += 1;
        }
    }

    /// Fold all complete leaf pairs into the tree.
    fn compress(&mut self) {
        let leaves = std::mem::take(&mut self.leaves);
        for leaf in leaves {
            self.push_node(leaf, 0);
        }
    }

    fn hash_leaves_parallel(&self, data: &[u8]) -> Vec<[u8; HASH_LEN]> {
        let count = data.len().div_ceil(LEAF_SIZE);
        let mut out = vec![[0u8; HASH_LEN]; count];
        if self.thread_count <= 1 || count < 8 {
            for (i, chunk) in data.chunks(LEAF_SIZE).enumerate() {
                out[i] = leaf_hash(chunk);
            }
            return out;
        }
        let per_thread = count.div_ceil(self.thread_count);
        std::thread::scope(|s| {
            for (ti, out_chunk) in out.chunks_mut(per_thread).enumerate() {
                let start = ti * per_thread * LEAF_SIZE;
                let end = (start + out_chunk.len() * LEAF_SIZE).min(data.len());
                let slice = &data[start..end];
                s.spawn(move || {
                    for (i, chunk) in slice.chunks(LEAF_SIZE).enumerate() {
                        out_chunk[i] = leaf_hash(chunk);
                    }
                });
            }
        });
        out
    }
}

impl AvdHash for TigerTreeHash {
    fn block_size(&self) -> usize {
        LEAF_SIZE * 2
    }

    fn initialize(&mut self) {
        self.leaves.clear();
        self.node_mask = 0;
        for n in self.nodes.iter_mut() {
            *n = [0u8; HASH_LEN];
        }
    }

    fn transform_full_blocks(&mut self, data: &[u8]) -> usize {
        let len = data.len().min(MAX_CHUNK) & !(LEAF_SIZE * 2 - 1);
        if len == 0 {
            return 0;
        }
        let leaves = self.hash_leaves_parallel(&data[..len]);
        self.leaves.extend(leaves);
        self.compress();
        len
    }

    fn transform_final_block(&mut self, data: &[u8]) -> Vec<u8> {
        if !data.is_empty() {
            let leaves = self.hash_leaves_parallel(data);
            self.leaves.extend(leaves);
            self.compress();
        }
        if self.node_mask == 0 {
            // Empty input: the tree consists of a single empty leaf.
            return leaf_hash(&[]).to_vec();
        }
        // Promote pending nodes from the lowest level upwards; the last one is the root.
        let mut acc: Option<[u8; HASH_LEN]> = None;
        for level in 0..MAX_LEVELS {
            if self.node_mask & (1u64 << level) == 0 {
                continue;
            }
            let node = self.nodes[level * 2];
            acc = Some(match acc {
                None => node,
                Some(right) => node_hash(&node, &right),
            });
        }
        acc.map(|h| h.to_vec()).unwrap_or_default()
    }
}
