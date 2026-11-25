//! Research probe R1 (`docs/RESEARCH-UNIFIED-MARGIN.md`): a script cites a transaction the chain accepted, without
//! spending anything of it (`contracts/probes/R1Cite.sil`). The commitment of a chain block is built here with the
//! node's own crates (`kaspa-seq-commit`, `kaspa-smt`) and given to the engine by a mock accessor, as the engine's own
//! tests do; the script recomputes it from a proof. The question is whether it can be done at all in a script, and at
//! what size. Whether the opcode is active on a network is not answered here.

mod common;

use common::*;
use kaspa_consensus_core::hashing::sighash::SigHashReusedValuesUnsync;
use kaspa_consensus_core::hashing::tx::{payload_digest, transaction_v1_rest_preimage};
use kaspa_consensus_core::subnets::SUBNETWORK_ID_NATIVE;
use kaspa_consensus_core::tx::{PopulatedTransaction, Transaction, TransactionInput, TransactionOutput, UtxoEntry};
use kaspa_consensus_core::Hash;
use kaspa_hashes::{HasherBase, SeqCommitActiveNode, SeqCommitMerkleBranch, ZERO_HASH};
use kaspa_seq_commit::hashing::{activity_digest_lane, activity_leaf, activity_root_hash, lane_key, lane_tip_next, mergeset_context_hash, seq_commit, seq_state_root, smt_leaf_hash};
use kaspa_seq_commit::types::{LaneId, LaneTipInput, MergesetContext, SeqCommitInput, SeqState, SmtLeafInput};
use kaspa_smt::proof::ProofTerminal;
use kaspa_smt::tree::SparseMerkleTree;
use kaspa_smt::{bit_at, SmtHasher};
use kaspa_txscript::caches::Cache;
use kaspa_txscript::covenants::CovenantsContext;
use kaspa_txscript::{pay_to_script_hash_script, EngineCtx, EngineFlags, SeqCommitAccessor, TxScriptEngine};
use kaspa_txscript_errors::TxScriptError;
use silverscript_abi::ArtifactValue;

const COV: Hash = Hash::from_bytes([0xc0; 32]);
const BLOCK: Hash = Hash::from_bytes([0xb1; 32]);
const KAS1: u64 = 100_000_000;

/// The chain as the script's transaction sees it: one block with a commitment.
struct Chain {
    block: Hash,
    commit: Hash,
}

impl SeqCommitAccessor for Chain {
    fn is_chain_ancestor_from_pov(&self, block_hash: Hash) -> Option<bool> {
        Some(block_hash == self.block)
    }
    fn seq_commitment_within_depth(&self, block_hash: Hash) -> Option<Hash> {
        (block_hash == self.block).then_some(self.commit)
    }
}

fn branch(l: Hash, r: Hash) -> Hash {
    let mut h = SeqCommitMerkleBranch::new();
    h.update(l).update(r);
    h.finalize()
}

/// The path from leaf `at` to the root of the node's Merkle tree over `leaves` (padded to a power of two; a missing
/// right child is the zero hash): (sibling, whether the node is the right child) from the bottom.
fn merkle_path(leaves: &[Hash], at: usize) -> Vec<(Hash, bool)> {
    let mut level: Vec<Option<Hash>> = leaves.iter().copied().map(Some).collect();
    level.resize(leaves.len().next_power_of_two(), None);
    let (mut i, mut path) = (at, vec![]);
    while level.len() > 1 {
        path.push((level[i ^ 1].unwrap_or(ZERO_HASH), i % 2 == 1));
        level = level.chunks(2).map(|c| c[0].map(|l| branch(l, c[1].unwrap_or(ZERO_HASH)))).collect();
        i /= 2;
    }
    path
}

fn pack(path: &[(Hash, bool)]) -> (Vec<u8>, i64) {
    let bytes = path.iter().flat_map(|(h, _)| h.as_bytes()).collect();
    let bits = path.iter().enumerate().map(|(k, (_, right))| (*right as i64) << k).sum();
    (bytes, bits)
}

fn lane(b: u8) -> LaneId {
    let mut id = [0u8; 20];
    id[0] = b;
    id
}

/// A transaction of version 1 whose first output is under `cov` with the script `note`, among `others` more inputs.
fn cited(cov: Hash, note: &[u8], inputs: usize, payload: Vec<u8>) -> Transaction {
    let ins = (0..inputs).map(|k| TransactionInput::new(outpoint(0x30 + k as u8), vec![1, 2, 3], 7, 1)).collect();
    let outs = vec![
        TransactionOutput { value: 3 * KAS1, script_public_key: pay_to_script_hash_script(note), covenant: bind(0, cov) },
        TransactionOutput { value: 5 * KAS1, script_public_key: p2pk_spk(&keypair(9)), covenant: None },
    ];
    Transaction::new(1, ins, outs, 0, SUBNETWORK_ID_NATIVE, 0, payload)
}

/// Everything a citation shows, as the script takes it.
struct Proof {
    args: Vec<ArtifactValue>,
    commit: Hash,
    bytes: usize,
}

/// The block's commitment with `tx` as the transaction `at` of `others + 1` the native lane had accepted there, among
/// `lanes` lanes; and the proof of it.
fn prove(tx: &Transaction, note: &[u8], at: usize, others: usize, lanes: u8) -> Proof {
    let rest = transaction_v1_rest_preimage(tx);
    let mut ids: Vec<Hash> = (0..others).map(|k| activity_leaf(&Hash::from_bytes([0x50; 32]), 7, k as u32)).collect();
    ids.insert(at, tx.id());
    let leaves: Vec<Hash> = ids.iter().enumerate().map(|(i, id)| activity_leaf(id, 1, i as u32)).collect();
    let activity = activity_digest_lane(leaves.iter().copied());
    let act_path = merkle_path(&leaves, at);
    let native: LaneId = AsRef::<[u8]>::as_ref(&SUBNETWORK_ID_NATIVE).try_into().expect("20 bytes");
    let (key, parent_ref) = (lane_key(&native), Hash::from_bytes([0x11; 32]));
    let ctx = mergeset_context_hash(&MergesetContext { timestamp: 1_764_000_000_000, daa_score: 630_300_000, blue_score: 629_000_000 });
    let tip = lane_tip_next(&LaneTipInput { parent_ref: &parent_ref, lane_key: &key, activity_digest: &activity, context_hash: &ctx });
    let blue_score = 629_000_000u64;
    let mut tree = SparseMerkleTree::<SeqCommitActiveNode>::new();
    tree.insert(key, smt_leaf_hash(&SmtLeafInput { lane_tip: &tip, blue_score }));
    for b in 1..lanes {
        tree.insert(lane_key(&lane(b)), smt_leaf_hash(&SmtLeafInput { lane_tip: &Hash::from_bytes([b; 32]), blue_score: 628_000_000 + b as u64 }));
    }
    let p = tree.prove(&key).expect("a proof");
    let depth = match p.terminal {
        ProofTerminal::Collapsed { depth } => depth as usize,
        ProofTerminal::Full => 256,
        other => panic!("the lane is in the tree: {other:?}"),
    };
    // From the collapsed node up: the sibling the proof gives, or the empty subtree's hash where its bitmap says so.
    let mut left = p.siblings.len();
    let smt_path: Vec<(Hash, bool)> = (0..depth)
        .rev()
        .map(|d| {
            let empty = p.bitmap[d / 8] & (1 << (d % 8)) != 0;
            let s = if empty {
                <SeqCommitActiveNode as SmtHasher>::EMPTY_HASHES[255 - d]
            } else {
                left -= 1;
                p.siblings[left]
            };
            (s, bit_at(&key, d))
        })
        .collect();
    let (inactivity, payload_ctx, parent_seq) = (Hash::from_bytes([0x21; 32]), Hash::from_bytes([0x22; 32]), Hash::from_bytes([0x23; 32]));
    let activity_root = activity_root_hash(&inactivity, &tree.root());
    let state_root = seq_state_root(&SeqState { activity_root: &activity_root, payload_and_ctx_digest: &payload_ctx });
    let commit = seq_commit(&SeqCommitInput { parent_seq_commit: &parent_seq, state_root: &state_root });
    if std::env::var("R1_LOG").is_ok() {
        eprintln!("RUST id {} activity {} tip {} leaf {} root {} activity_root {} state_root {} commit {}", tx.id(), activity, tip, smt_leaf_hash(&SmtLeafInput { lane_tip: &tip, blue_score }), tree.root(), activity_root, state_root, commit);
    }
    let ((act_bytes, act_bits), (smt_bytes, smt_bits)) = (pack(&act_path), pack(&smt_path));
    let h = |x: Hash| ArtifactValue::Bytes(x.as_bytes().to_vec());
    let bytes = rest.len() + act_bytes.len() + smt_bytes.len() + 9 * 32 + 35;
    let args = vec![
        h(BLOCK),
        ArtifactValue::Bytes(rest),
        int(tx.inputs.len() as i64),
        h(payload_digest(&tx.payload)),
        int(at as i64),
        ArtifactValue::Bytes(act_bytes),
        int(act_bits),
        int(act_path.len() as i64),
        h(parent_ref),
        h(key),
        h(ctx),
        int(blue_score as i64),
        ArtifactValue::Bytes(smt_bytes),
        int(smt_bits),
        int(smt_path.len() as i64),
        h(inactivity),
        h(payload_ctx),
        h(parent_seq),
        ArtifactValue::Bytes(pay_to_script_hash_script(note).script().to_vec()),
    ];
    Proof { args, commit, bytes }
}

/// The reader's transaction with the proof `args`, run in the engine against a chain whose block has `commit`:
/// (the result, the bytes of the reader's signature script, the largest stack).
fn read(args: Vec<ArtifactValue>, commit: Hash) -> (Result<(), TxScriptError>, usize, usize) {
    let art = compile("probes/R1Cite.sil", &[bytes32(COV), int(1)]);
    let (tx, entries) = build(&[contract_in(outpoint(1), 10 * KAS1, None, &art, "cite", args)], vec![out(9 * KAS1, p2pk_spk(&keypair(3)), None)]);
    let chain = Chain { block: BLOCK, commit };
    let populated = PopulatedTransaction::new(&tx, entries.to_vec());
    let cov_ctx = CovenantsContext::from_tx(&populated).expect("covenant bindings");
    let (reused, cache) = (SigHashReusedValuesUnsync::new(), Cache::new(10_000));
    let mut log: Vec<u8> = Vec::new();
    let entry: &UtxoEntry = &entries[0];
    let result = {
        let ctx = EngineCtx::new(&cache).with_reused(&reused).with_covenants_ctx(&cov_ctx).with_seq_commit_accessor(&chain);
        let mut vm = TxScriptEngine::from_transaction_input(&populated, &tx.inputs[0], 0, entry, ctx, EngineFlags::default()).with_opcode_execution_log_buffer(&mut log);
        vm.execute()
    };
    if result.is_err() && std::env::var("R1_LOG").is_ok() {
        let text = String::from_utf8_lossy(&log);
        let lines: Vec<&str> = text.lines().collect();
        for l in &lines[lines.len().saturating_sub(4)..] {
            let cut: String = l.chars().rev().take(700).collect::<String>().chars().rev().collect();
            eprintln!("LOG {} lines; tail: {cut}", lines.len());
        }
    }
    let max_stack = String::from_utf8_lossy(&log).lines().map(|l| l.matches("\"0x").count()).max().unwrap_or(0);
    (result, tx.inputs[0].signature_script.len(), max_stack)
}

#[test]
fn r1_a_script_cites_a_transaction_the_chain_accepted() {
    let note = [0x51u8, 0x52, 0x53];
    let art = compile("probes/R1Cite.sil", &[bytes32(COV), int(1)]);
    println!("R1Cite: {} bytes of script", bytecode(&art).len());
    // The transaction cited: three inputs, a note under the committee's id, a payload. Shapes of the block: how many
    // transactions of the lane it accepted, and how many lanes are active.
    for (others, at, lanes) in [(0usize, 0usize, 1u8), (4, 3, 2), (99, 57, 3), (999, 640, 5), (9_999, 8_000, 9)] {
        let tx = cited(COV, &note, 3, b"a price".to_vec());
        let p = prove(&tx, &note, at, others, lanes);
        let (r, sig, stack) = read(p.args.clone(), p.commit);
        assert!(r.is_ok(), "{others} others, {lanes} lanes: {r:?}");
        println!("R1 a citation among {} transactions of the lane and {lanes} lanes: a proof of {} bytes, a signature script of {sig} bytes (the script among them), a stack of {stack} at most", others + 1, p.bytes);
        // Against another block's commitment the same proof is nothing.
        assert!(read(p.args.clone(), Hash::from_bytes([0x77; 32])).0.is_err(), "another commitment");
    }
}

#[test]
fn r1_what_is_not_so_is_not_cited() {
    let note = [0x51u8, 0x52, 0x53];
    let tx = cited(COV, &note, 3, b"a price".to_vec());
    let good = prove(&tx, &note, 3, 6, 3);
    assert!(read(good.args.clone(), good.commit).0.is_ok());
    let refused = |what: &str, args: Vec<ArtifactValue>| assert!(read(args, good.commit).0.is_err(), "{what}");
    // The output is under another covenant id, or has another script: the proof of that transaction is a proof, and the
    // reader holds it to its own id and to the note it is told.
    let alien = cited(Hash::from_bytes([0xc1; 32]), &note, 3, b"a price".to_vec());
    let pa = prove(&alien, &note, 3, 6, 3);
    assert!(read(pa.args, pa.commit).0.is_err(), "an output under another id");
    let other_note = cited(COV, &[0x51, 0x52, 0x54], 3, b"a price".to_vec());
    let pn = prove(&other_note, &note, 3, 6, 3);
    assert!(read(pn.args, pn.commit).0.is_err(), "another note than the one the reader is told");
    // The note written as data inside another output is not the first output: the reader finds the first output by
    // the number of inputs, which it reads in the rest itself.
    let mut wrong_inputs = good.args.clone();
    wrong_inputs[2] = int(2);
    refused("a number of inputs that is not the transaction's", wrong_inputs);
    // A transaction that was not accepted: the same proof with another payload, another place, a sibling changed.
    let mut payload = good.args.clone();
    payload[3] = ArtifactValue::Bytes(payload_digest(b"another price").as_bytes().to_vec());
    refused("another payload", payload);
    let mut place = good.args.clone();
    place[4] = int(2);
    refused("another place among the lane's transactions", place);
    for (k, what) in [(5usize, "a sibling of the lane's transactions"), (12, "a sibling of the lanes' tree"), (8, "the parent reference"), (10, "the context"), (15, "the inactivity shortcut"), (16, "the payload and context digest"), (17, "the parent commitment")] {
        let mut a = good.args.clone();
        if let ArtifactValue::Bytes(b) = &mut a[k] {
            b[0] ^= 1;
        }
        refused(what, a);
    }
    let mut blue = good.args.clone();
    blue[11] = int(629_000_001);
    refused("another blue score of the lane", blue);
    // A block that is not a chain ancestor, or too deep: the engine refuses the opcode.
    let mut block = good.args.clone();
    block[0] = ArtifactValue::Bytes(vec![0xb2; 32]);
    refused("another block", block);
}
