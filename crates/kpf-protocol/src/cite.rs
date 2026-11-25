//! Citing a transaction the chain accepted (`docs/RESEARCH-UNIFIED-MARGIN.md` R1, `docs/ARCHITECTURE-CLEARING.md` 3).
//!
//! A chain block's header carries its sequencing commitment (KIP-21, in `accepted_id_merkle_root`), which a script
//! reads with `OpChainblockSeqCommit`. This module is the tool's side: from what a node says of a block it builds the
//! witness that leads from one accepted transaction up to that commitment, and recomputes the commitment exactly as
//! the citing script does, so that a tool knows before it sends whether its citation holds.
//!
//! ```text
//! id            = H_v1id(payload digest, H_rest(rest))
//! activity leaf = H_leaf(id, version, merge index)
//! activity      = Merkle root of the lane's leaves in the block
//! lane tip      = H_tip(parent ref, lane key, activity, context)       context = H(timestamp, DAA score, blue score)
//! lane leaf     = H_collapsed(lane key, H_active(tip, blue score))     under the lanes' sparse tree
//! commitment    = H(parent commitment, H(H_activity(inactivity shortcut, lanes' root), payload and context))
//! ```

use kaspa_consensus_core::Hash;
use kaspa_hashes::{HasherBase, SeqCommitActiveCollapsedNode, SeqCommitActiveNode, SeqCommitMerkleBranch, ZERO_HASH};
use kaspa_seq_commit::hashing::{activity_leaf, activity_root_hash, lane_key, lane_tip_next, mergeset_context_hash, seq_commit, seq_state_root, smt_leaf_hash};
use kaspa_seq_commit::types::{LaneId, LaneTipInput, MergesetContext, SeqCommitInput, SeqState, SmtLeafInput};
use kaspa_smt::proof::{OwnedSmtProof, ProofTerminal};
use kaspa_smt::{bit_at, SmtHasher};

use crate::error::{Error, Result};

/// A step of a path: the sibling, and whether the node below is the right child.
pub type Step = (Hash, bool);

/// What leads from a transaction to the commitment of the chain block that accepted it.
#[derive(Clone, Debug)]
pub struct Witness {
    /// The chain block, and the commitment its header carries.
    pub block: Hash,
    /// The transaction's place among everything the block accepted, and the path from its leaf to the root of its
    /// lane's activity there.
    pub merge_idx: u32,
    pub activity_path: Vec<Step>,
    /// The lane: its key, its tip before this block, the block's context, and the blue score of its leaf.
    pub lane_key: Hash,
    pub parent_ref: Hash,
    pub context: Hash,
    pub blue_score: u64,
    /// The path from the lane's collapsed leaf to the lanes' root.
    pub lanes_path: Vec<Step>,
    pub inactivity_shortcut: Hash,
    pub payload_and_ctx: Hash,
    pub parent_commitment: Hash,
}

fn branch(l: Hash, r: Hash) -> Hash {
    let mut h = SeqCommitMerkleBranch::new();
    h.update(l).update(r);
    h.finalize()
}

fn fold(mut h: Hash, path: &[Step], node: impl Fn(Hash, Hash) -> Hash) -> Hash {
    for (s, right) in path {
        h = if *right { node(*s, h) } else { node(h, *s) };
    }
    h
}

/// The key of the native lane (every ordinary transaction's).
pub fn native_lane_key() -> Hash {
    let native: LaneId = [0u8; 20];
    lane_key(&native)
}

/// A chain block's context as its lanes' tips carry it.
pub fn context(timestamp: u64, daa_score: u64, blue_score: u64) -> Hash {
    mergeset_context_hash(&MergesetContext { timestamp, daa_score, blue_score })
}

/// The leaves of a lane's activity in a block: (id, version, place among everything the block accepted) of each of
/// the lane's transactions, in the block's order.
pub fn activity_leaves(lane: &[(Hash, u16, u32)]) -> Vec<Hash> {
    lane.iter().map(|(id, version, idx)| activity_leaf(id, *version, *idx)).collect()
}

/// The root of the node's Merkle tree over `leaves` (a single leaf is the root; a missing right child is the zero
/// hash), and the path of the leaf `at`.
pub fn merkle(leaves: &[Hash], at: usize) -> (Hash, Vec<Step>) {
    let mut level: Vec<Option<Hash>> = leaves.iter().copied().map(Some).collect();
    level.resize(leaves.len().next_power_of_two().max(1), None);
    let (mut i, mut path) = (at, vec![]);
    while level.len() > 1 {
        path.push((level[i ^ 1].unwrap_or(ZERO_HASH), i % 2 == 1));
        level = level.chunks(2).map(|c| c[0].map(|l| branch(l, c[1].unwrap_or(ZERO_HASH)))).collect();
        i /= 2;
    }
    (level[0].unwrap_or(ZERO_HASH), path)
}

/// The path from a lane's collapsed leaf to the lanes' root, from a node's proof of the lane (`bitmap || terminal ||
/// siblings`): the sibling the proof gives at each level, or the empty subtree's hash where its bitmap says so.
pub fn lanes_path(proof: &[u8], key: &Hash) -> Result<Vec<Step>> {
    let p = OwnedSmtProof::from_bytes(proof).map_err(|e| Error::Invalid(format!("the lane's proof does not read: {e}")))?;
    let depth = match p.terminal {
        ProofTerminal::Collapsed { depth } => depth as usize,
        ProofTerminal::Full => 256,
        ProofTerminal::CollapsedOther { .. } => return Err(Error::Invalid("the proof says the lane is not in the tree".into())),
    };
    let mut left = p.siblings.len();
    let mut path = vec![];
    for d in (0..depth).rev() {
        let s = if p.bitmap[d / 8] & (1 << (d % 8)) != 0 {
            <SeqCommitActiveNode as SmtHasher>::EMPTY_HASHES[255 - d]
        } else {
            left = left.checked_sub(1).ok_or_else(|| Error::Invalid("the lane's proof has fewer siblings than its bitmap says".into()))?;
            p.siblings[left]
        };
        path.push((s, bit_at(key, d)));
    }
    Ok(path)
}

impl Witness {
    /// The lane's tip the witness leads to, for a transaction of the id and version given.
    pub fn tip(&self, id: &Hash, version: u16) -> Hash {
        let activity = fold(activity_leaf(id, version, self.merge_idx), &self.activity_path, branch);
        lane_tip_next(&LaneTipInput { parent_ref: &self.parent_ref, lane_key: &self.lane_key, activity_digest: &activity, context_hash: &self.context })
    }

    /// The commitment the witness leads to: what the citing script compares with the engine's.
    pub fn commitment(&self, id: &Hash, version: u16) -> Hash {
        let tip = self.tip(id, version);
        let leaf = smt_leaf_hash(&SmtLeafInput { lane_tip: &tip, blue_score: self.blue_score });
        let mut c = SeqCommitActiveCollapsedNode::new();
        c.update(self.lane_key).update(leaf);
        let node = |l: Hash, r: Hash| {
            let mut h = SeqCommitActiveNode::new();
            h.update(l).update(r);
            h.finalize()
        };
        let lanes_root = fold(c.finalize(), &self.lanes_path, node);
        let activity_root = activity_root_hash(&self.inactivity_shortcut, &lanes_root);
        let state_root = seq_state_root(&SeqState { activity_root: &activity_root, payload_and_ctx_digest: &self.payload_and_ctx });
        seq_commit(&SeqCommitInput { parent_seq_commit: &self.parent_commitment, state_root: &state_root })
    }

    /// The bytes of the proof a citing script is shown beside the transaction's rest: 32 a step and nine hashes.
    pub fn bytes(&self) -> usize {
        32 * (self.activity_path.len() + self.lanes_path.len()) + 9 * 32
    }
}
