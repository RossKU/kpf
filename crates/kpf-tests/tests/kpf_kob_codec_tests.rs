//! The KOB codecs and artifacts of `kpf-protocol` (used by the evidence builders and the `evidence` check) against what
//! KOB's own compiler emits from the read-only sources in `vendor/kob/contracts`: the embedded artifacts have the same
//! templates, the state encoders emit the compiler's bytes, and the signature scripts the library assembles are the ones
//! the ABI encoder of the compiled instance assembles.

mod common;
mod kpf;

use common::*;
use kpf::kob::*;
use kpf::*;
use kpf_protocol::artifacts as lib;
use kpf_protocol::kob as k;
use secp256k1::Keypair;
use silverscript_abi::ArtifactValue;

#[test]
fn embedded_kob_artifacts_have_the_compilers_templates() {
    for (name, art, base) in [("KobAsk", lib::ask_template(), ask_base()), ("KobBid", lib::bid_template(), bid_base()), ("KCC20Ref", lib::token_template(), kcc20_base())] {
        let (pre, suf) = parts(base);
        assert_eq!(art.prefix, pre, "{name} prefix");
        assert_eq!(art.suffix, suf, "{name} suffix");
        assert_eq!(art.hash, template_hash(base), "{name} template hash");
    }
    // They are the constants KpfPair is built with.
    let (h, pre, suf) = tpl_facts(ask_base());
    assert_eq!((h, pre, suf), (KOB_ASK_TPL, KOB_ASK_PRE, KOB_ASK_SUF));
    let (h, pre, suf) = tpl_facts(bid_base());
    assert_eq!((h, pre, suf), (KOB_BID_TPL, KOB_BID_PRE, KOB_BID_SUF));
}

#[test]
fn state_encoders_emit_the_compilers_bytes() {
    let m = market();
    let mut ours = kpf_protocol::fixtures::ask_of(&m, [9; 32], 123_456_789, 456);
    let mut theirs = ask_of(&m, [9; 32], 123_456_789, 456);
    for (a, b) in [(&mut ours.interval, &mut theirs.interval)] {
        *a = -3;
        *b = -3;
    }
    ours.tif = 2;
    theirs.tif = 2;
    ours.tip = 7;
    theirs.tip = 7;
    assert_eq!(ours.redeem(), bytecode(&theirs.art()));
    assert_eq!(ours.redeem(), bytecode(&compile_src("KobAsk", &kob_source("orders/KobAsk.sil"), &theirs.ctor())));
    let ours = kpf_protocol::fixtures::bid_of(&m, [9; 32], 789);
    let theirs = bid_of(&m, [9; 32], 789);
    assert_eq!(ours.redeem(), bytecode(&theirs.art()));
    assert_eq!(ours.redeem(), bytecode(&compile_src("KobBid", &kob_source("orders/KobBid.sil"), &theirs.ctor())));
    let t = Tok { amount: 55, owner: [4; 32], scheme: SCHEME_COVID, borrow: 0, guard: [3; 32], ext: m.ext };
    let ours = k::TokenState { amount: 55, owner: [4; 32], scheme: SCHEME_COVID, borrow: 0, guard: [3; 32], ext: m.ext };
    assert_eq!(ours.redeem(), bytecode(&t.art()));
    assert_eq!(ours.redeem(), bytecode(&compile_src("KCC20Ref", &kob_source("kcc20/KCC20Ref.sil"), &t.ctor())));
    // And the decoders read the compiler's instances back.
    assert_eq!(k::TokenState::decode_redeem(&bytecode(&t.art())).unwrap(), ours);
}

/// The signature script of each KOB entry the builders use, assembled from the embedded artifact, equals the one the
/// compiled instance's ABI encodes.
#[test]
fn signature_scripts_equal_the_abi_encoders() {
    use kpf_protocol::tx::{bv, iv, Arg as LArg, KobWhich, Plan};
    let m = market();
    let kp: Keypair = keypair(31);
    let fill = sm8(1_000_000_000);
    // KobAsk.settle
    let a = ask_of(&m, xonly(&kp), 940_000_000, 1_500_000_000);
    let want = entry_sig_script(&a.art(), "settle", &[ArtifactValue::Bytes(fill.to_vec()), int(1), int(1), int(0)]);
    let plan = Plan::Kob { which: KobWhich::Ask, redeem: bytecode(&a.art()), entry: "settle", args: vec![bv(&fill), iv(1), iv(1), iv(0)] };
    assert_eq!(plan.sigscript(&|_| unreachable!()).unwrap(), want);
    // KobBid.fill
    let b = bid_of(&m, xonly(&kp), 1_000_000_000);
    let want = entry_sig_script(&b.art(), "fill", &[ArtifactValue::Bytes(fill.to_vec()), int(1), int(0)]);
    let plan = Plan::Kob { which: KobWhich::Bid, redeem: bytecode(&b.art()), entry: "fill", args: vec![bv(&fill), iv(1), iv(0)] };
    assert_eq!(plan.sigscript(&|_| unreachable!()).unwrap(), want);
    // KCC20Ref.transfer (a custody: witness 0x00; a P2PK holder: 0x00 || signature)
    let bought = Tok::p2pk(5, xonly(&kp), m.ext);
    let ours = k::TokenState::p2pk(5, xonly(&kp), m.ext);
    let custody = Tok::covid(5, Hash::from_bytes([1; 32]), m.ext);
    let want = entry_sig_script(&custody.art(), "transfer", &[ArtifactValue::Array(vec![bought.value()]), ArtifactValue::Bytes(vec![0])]);
    let plan = Plan::Kob {
        which: KobWhich::Token,
        redeem: bytecode(&custody.art()),
        entry: "transfer",
        args: vec![LArg::V(ArtifactValue::Array(vec![ours.value()])), bv(&[0])],
    };
    assert_eq!(plan.sigscript(&|_| unreachable!()).unwrap(), want);
    let sig = vec![7u8; 65];
    let holder = Tok::p2pk(5, xonly(&kp), m.ext);
    let want = entry_sig_script(&holder.art(), "transfer", &[ArtifactValue::Array(vec![bought.value()]), ArtifactValue::Bytes([vec![0], sig.clone()].concat())]);
    let plan = Plan::Kob {
        which: KobWhich::Token,
        redeem: bytecode(&holder.art()),
        entry: "transfer",
        args: vec![LArg::V(ArtifactValue::Array(vec![ours.value()])), LArg::LeaderSig(xonly(&kp))],
    };
    assert_eq!(plan.sigscript(&|_| Ok(sig.clone())).unwrap(), want);
}

use kaspa_consensus_core::Hash;
