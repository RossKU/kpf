//! The oracle-market builders (`kpf_protocol::oracle_build`) on the pinned artifacts: every transaction shape of a
//! market's life, signed, metered and balanced, passes `verify::verify` (the engine with consensus metering, the fee
//! floor, outputs not above inputs, the storage-mass commitment, the block limits); the pinned manifest and checksums
//! agree with the sources; the codecs invert each other; and fixed TEST-ONLY keys and inputs give fixed transactions
//! (golden vectors: transaction id and the SHA-256 of the RPC JSON). If a contract or a builder rule changes on purpose,
//! run with `KPF_GOLDEN=print` and update `GOLDEN`.

use kaspa_consensus_core::tx::{TransactionId, TransactionOutpoint};
use kpf_protocol::bond::{Bond, RING};
use kpf_protocol::judge::{Js, YES};
use kpf_protocol::oracle::*;
use kpf_protocol::oracle_artifacts as pinned;
use kpf_protocol::oracle_build::*;
use kpf_protocol::state::{KIND_CLOSE, KIND_LIQ, LONG, SHORT};
use kpf_protocol::tx::{p2pk_spk, pubkey_of, rpc_json, KeySet, KeyUtxo, MIN_FEE_RATE};
use sha2::{Digest, Sha256};

const KAS: u64 = 100_000_000;

fn sha(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn contracts() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts")
}

#[test]
fn the_oracle_manifest_and_checksums_agree_with_the_sources() {
    let m = pinned::manifest();
    assert_eq!(m.format, "kpf-artifacts/oracle-1");
    for (name, text) in pinned::artifact_texts() {
        let c = &m.contracts[name];
        let src = std::fs::read_to_string(contracts().join(format!("legacy/{name}.sil"))).unwrap().replace("\r\n", "\n");
        assert_eq!(c.source_sha256, sha(src.as_bytes()), "contracts/legacy/{name}.sil changed: regenerate the artifacts");
        assert_eq!(c.artifact_sha256, sha(text.replace("\r\n", "\n").as_bytes()), "{name}");
    }
    let sums = std::fs::read_to_string(contracts().join("legacy/artifacts/oracle/SHA256SUMS")).unwrap();
    let mut seen = 0;
    for line in sums.lines() {
        let (h, path) = line.split_once("  ").unwrap();
        let text = std::fs::read_to_string(contracts().join(path)).unwrap().replace("\r\n", "\n");
        assert_eq!(h, sha(text.as_bytes()), "{path}");
        seen += 1;
    }
    assert_eq!(seen, 11, "five sources, five artifacts, the manifest");
    let k = pinned::kit();
    for t in [&k.open, &k.pair, &k.pending, &k.bond, &k.stake] {
        let c = &m.contracts[t.name];
        assert_eq!((t.prefix.len(), t.suffix.len(), t.state_len), (c.prefix_len, c.suffix_len, c.state_len), "{}", t.name);
        assert_eq!(t.hash.iter().map(|b| format!("{b:02x}")).collect::<String>(), c.template_hash);
    }
    assert_eq!((k.open.state_len, k.pair.state_len, k.pending.state_len), (OPEN_STATE_LEN, PAIR_STATE_LEN, PENDING_STATE_LEN));
    // The slot and the judge are the templates of slices A and B1, byte for byte (their tests compile them from source).
    assert_eq!((k.bond.state_len, k.stake.state_len), (kpf_protocol::bond::BOND_STATE_LEN, kpf_protocol::judge::STATE_LEN));
}

#[test]
fn the_codecs_invert_each_other_and_decode_by_template() {
    let m = samples::market();
    assert!(m.check().is_ok());
    assert_eq!(OMarket::decode(&m.bytes()).unwrap(), m);
    let p = samples::pair(&m);
    let w = samples::pending(&p);
    let o = samples::order(&m, LONG, [9; 32]);
    let b = Bond::new([7; 32], m.bond_market(), 3);
    let s = Js::free([7; 32], m.judge_market());
    for obj in [OObj::Pair(p), OObj::Pending(w), OObj::Order(o), OObj::Slot(b), OObj::Stake(s)] {
        assert_eq!(OObj::decode(&obj.redeem()).unwrap(), obj, "{}", obj.kind());
    }
    // The feed definition's hash is the block's; one byte changed is another market.
    assert_eq!(m.feed_hash, feed_hash_of(samples::FEED.as_bytes()));
    let mut m2 = m;
    m2.feed_hash = feed_hash_of(samples::FEED.replace("10 seconds", "11 seconds").as_bytes());
    assert_ne!(m.id(), m2.id());
    // Golden: the sample market's id and block.
    let golden_id = "see GOLDEN_MARKET";
    let id_hex: String = m.id().iter().map(|b| format!("{b:02x}")).collect();
    if std::env::var("KPF_GOLDEN").as_deref() == Ok("print") {
        println!("GOLDEN_MARKET = {id_hex} (block sha256 {})", sha(&m.bytes()));
    } else {
        assert_eq!(id_hex, GOLDEN_MARKET, "{golden_id}");
    }
}

const GOLDEN_MARKET: &str = "0d66a92749804ac495bfbf2ac60a337c498fb41012a7c3c4f41e2646830dc8d9";

const GOLDEN: &[(&str, &str, &str)] = &[
    ("create slot", "89457e21d301a44ece37bc43f804b89842c4528f45990ade0015ee2256860235", "de3cee3dfbeaf568c22a4722210108566867f29e34dd895c0842ba0f3b027966"),
    ("create stake", "d29445a7de64816238baa7a0228096719633b4548afbd2ed738ae6a4c4794e9f", "d28134d95e65a9d08832b2c491b2290dba55f5bfcda624f35c2f121f74321fba"),
    ("create the underwriter's stake", "4183a1d7f3064c1d29b3b3b0de58ce6a6a3d60b59c25d6b20928b5986a4aad0d", "f28d0e338717b2ed268bb6832c7d2236cdd85dc080fb0d8ebfa06cc331fb06ec"),
    ("create long order", "47c1fa51b0e5ef9788216b38c7c412ec350726a36d1bcf8bae0d1d4c480631d8", "b52328fc5cdb17eb4220d24d513cadd02ff4776d1fbf512fb4455098a50180b0"),
    ("create short order", "8718c5d0823c921838894ac28ff9f5f43091d35dc526e5457d357c7f0d7b4ca1", "9fa334bfbf702742751b45953de691fc281a0f9e18bafe0a39f1f3371c20097f"),
    ("open pair A", "88245b41c6805199a75a8fb8030fffe3d8a84c61958cfd6dbd8d33a8cfe4361e", "077a05f4925aa2046a1a3cd2961838e9b7b205903e1faf5e1ddac135b9832182"),
    ("open pair B", "24c636ebb79ed520dd15602b70adf1bae1fdcb0f12af33abb4cab0dfd7ef2716", "8fcd8d741406f83292f6037fdad8ae5a0060bd6db0e01cb65e66a957ca0d8e77"),
    ("propose a liquidation", "ae4badc382a522a3c5e3de1af555bf3b1c66e58f6b837f07984e28ec885dddd4", "e52684fcbdb552947ffb21284fcadea18d4c4867dc40731bb1b343e46c3d90a5"),
    ("contest", "e772e760702001c1ac11b8ae7ad1e154e429c906e15a99c5cf88a1ded77cf9f6", "db8ba2867425a707623ca5073f54e4b33627b5cb0bd9eada9c7b14e7639b2cb2"),
    ("resolve and decide (refuted)", "6e215118c80138b51d8a65c83f1a706787480c366c147aa35a44bafbd0f36ca1", "f12cc861151f829f71bf7973892edaa2cd41e95bcd00983b7bd4b6ddd751abe1"),
    ("finalize refuted (restore)", "341e7cbe6ca1db5649413023748573c0ac03678a90770014382bd1787c52cee8", "b56646ddff80ff5821f35e7966baee4f1aeb24f8a0f5a24022b335a2f4965d81"),
    ("settle the opener's receipt", "6010ac29dbceba5123226f8ae691d843c65d92cbbdadc9fda4f89de5727748a2", "ea47b5ab4857b563e2dc8e947bf0ea24aa1f8e99ef75715ee9a660b94efa369b"),
    ("close the pool", "bff00ea5052b6a01f3f2c54632ad8c27e6836a38ac329a69b39900b70e6e5176", "5768f0b1363ec11454984621df9616d151cf6c9eaa62a07854bcb18359e79ba3"),
    ("propose again", "5cd8d2685144113be939c5caaa37b665c5569973c06cdbc02e48442d4f4deb0e", "9986b0b263d45fce87b6b0151706f50d3349afe0a36f775490e8cbe3808f3de4"),
    ("create a second stake", "a717eac749d3a0130058d87064032cb7bb5e24b3bb6a1c251c71f0c31d18a430", "05d7bbbb60980074080bf2378b248e66a5278b533afc7e0a9864e06e0844cdf2"),
    ("contest a true claim", "e13aeeca257f6de4e2d7b957e0f959846a703ffa9f253d1a8284611507da7739", "6b7e64c97004efb76da676b6b32bea383cf3b95141abcdcdb8501c8028bde272"),
    ("deposit the defence", "b9ab49d6fe81e57e9d14ca4a3171a70842465a5f66f082297ea4cf23dd9c42d5", "c4974005b72881d5a8c6c8ff382e66cfbee343690c644cea501ff8715a23755b"),
    ("resolve and decide (upheld)", "160444bd4a6ff95d05b16c8daf9d6f0e7e55d36e326f2e570bd745ff1dd12e98", "bddae5ed28b0164a9974100678e13dab29681d4612f51b8fcf676b29502280a7"),
    ("finalize upheld (forfeit)", "a9e83a2290ba1978d18e3c3f7f30a2c3915ebe96bf91f18d3575f35918ade0d9", "db19584bcdabc490391218ecfcd144eaeaca32259166d1fb6bf9dbd82bcfc457"),
    ("settle a winning and a losing receipt", "498bf2ed1f4b850927092acb6e9d74b4ae6c8c0bfe735eed5a94f70493dbd820", "fbce1a5553790f7231ccada16dce55a6c319321b74edd3c1515b5ed692022db9"),
    ("close the pool with a burn", "79cbbe05b2b73a98f147311f2b21485ccddda5fbccdf0153230154188a33c433", "9301194edce0829f81cc7dda20df8242763c1efdff808acd1a19e09cbbf4e28b"),
    ("propose on B", "5381458c4031b4f73dfed1c911d58cba2da644b566ada1f674edcd61ad4f0ae5", "ef5c527e0bf5a727287e563438909cffcc7527aa4a6e966c5026f1fd90b79993"),
    ("cure", "1abd06b455de850fd461fd3ba9ed5dab426c06723473480db44b09a228aed769", "7343d7166f805f6e52980819fcb37cac0cb110e46dfe6cf442f3d7cd6e7f546f"),
    ("renew B", "beba5a74406df79298b5323782c6843b13728ae2338f5ee3cfe84ff3857bf7e5", "906500d0c26775997adfc121d175221c1289076175215ed6226ddf720b8ab103"),
    ("amend: list the long", "ae147fbbed8375e675cdb423755547d2588aa84788836acb4b53548540a0f510", "6bc8f0f0d1147c4c602f387f2b9841b78f1cb2baa69dea00ac56215986b26e37"),
    ("transfer the long to a signing taker", "d85ba8afac9123a0f74161c7bb9fe99d3d21ec0d5a693f08ff358c8c451dcafe", "ced3e36257bbd3086986b219ec051ad947cfb1a61900725bd36c5f3e8aac1d82"),
    ("amend: the taker lists", "b0b6d7ccc1765fac37344d93b2248a3f39d7824e8a161598a5bffeb234d52327", "149c50ba280371f83c967de82107954fa2e5b85d5676ceb90af5e4d9784d7544"),
    ("amend: the short lists", "6a0bfb3348da8a4c68100d2dd71d4879fe2a9603ead9e54c827108646f47f6be", "1bca101a75e49e4f726e29da4c59e4a80b07c9baf2be066ba5833259fde41ce2"),
    ("mutual close", "5c46ff3614ebb2f46397db5b361dc26cbcc15b7baad69fe74620e2e10885e5fb", "d43f8e452ade5863c80450ebf797a91777eed05fb54d480b8219f4ea97cf36f3"),
    ("lapse settle", "b50cc5ecada85d6c6a2f01ca9f91bc8f81dc9ef5c45f35ca3396cc6d697c9d15", "f3c48aa66059a989a61d7cc658cac2f233744ee85de90dc8aeb6bb190c24d660"),
    ("create long order 3", "dbe7acfeb8e23646eca9a20044d95d8c52fd8170ed00179ab33865e58f85a6c1", "054cc933812a2b32a5b581d28e689955946a9352ce8d64f37b616263bbf47a6b"),
    ("create short order 3", "b971ba229223c359ef667019fbf99181610bbb8b4fb56b2aa85146f5841a1f87", "c3644dc9858d08cdecea5747106586d76020aa90c1fbc359722aaefba32ac774"),
    ("open pair C", "17f3f15a8f8c1db56e17f7295e68b6baa5db186c59e59ef4441ba359e7bc9207", "01c2c7886fb7b99d3ef65a6bdaf29463d4d3dfa859179e220e07323dfd4d4f48"),
    ("cooperative close", "f45012f338a768921dd84e2a0feed01d997e268480dbcd81cf79ac8971ada771", "5046c7d56439236a1356724e458b2e46978d20a975cf02ef05cc6dffb38506f1"),
    ("advance the slot", "3ab3b67c83de397af7b0ea7be77d7352ce459b12525806533df84a303fcdca70", "ff81b28fe475ff1b8ae684ec892a2f264a9ffe5b63e0e59c823f59b82e8c1a65"),
    ("split the slot", "3c2541e42f7c7b31eead30090c6a998c6ca68b7a2f487049b0c29463ebaa98b8", "1a24d44a9da4ef606b15079b10e86eedc4b4fdd326990fb7957c95ad8ea42c24"),
    ("unbond child A", "f861ee926a4ae60b62b9215490ff4c29d11127b2822cc323065d39f2ac120e02", "13d58c1bee23072908e607767db928452fbbbaefe8413b335d58a3ca5a0829b6"),
    ("withdraw child A", "354ff0d7d84471c517c0e111b52f1cede3cd189fef0a0e9c88fed105995e3aed", "ed63c386e97da0fe143b8f1b9899307ed2e11f8efbffafa84af37a76b55bdef1"),
    ("unbond a stake", "52b3faf78a2c962e4c7c6a6f6d0b6bb5a939675b86a0779370d7648d74d8dfe8", "ca1cd8f51f8261c1ca130a22eebe9e11016e91abc8e6fe37725a840e92964451"),
    ("withdraw a stake", "e0cbf90c081293fe98658de38c3709f8432fbbc7eb8ce23f0e0ea77a335d61db", "1c9ca70bc4efd5be192735ba1e4eef1c945de413d46ebc879c43cb1efcd8f28e"),
    ("create long order 4", "f8bd87341209b833b053c122d3d3a64c36828fe0062cb8b78e9da6502948c7be", "9658b08f572dff623267721b3f8d0a225a6df498bed2c5ea77765126437986a2"),
    ("create short order 4", "e7d170636945c1fc472368b62304f6db22de272417cd9883255ef7ce89e752e8", "a2e262a1bd0e42fb199d8573e18a6341130a7b659c1c40ff99f0bbbb039eab87"),
    ("create a second slot", "7d11ace5b3c57e69e80af903ffb7f03d28a0f008bf504f7b385618e774f03e17", "d5e3ad656bff4d541f02672d5b30fb257358d7c035753a478959f97247612cd3"),
    ("open pair D", "d25ca676702590c8d3b7a8f31cd2fb327c355ccd4c9adab63b1052113975a356", "606cc57059225d6284fb6874643409e8ea21561e2aeb33ab0cf7e5c3eafa1c9e"),
    ("propose a close", "f4a60b43045e6ff8bb27d23a7b718924be7b9a50ebccc7a83cbfd7c7bd2aa2a5", "4530d5a43bac25d0f4e822de9dcea6e083776434a7e2ad2b35368ad151fd9812"),
    ("finalize the close", "38d737d17c0ad5a99ac4a2f0bc8403134d2967db873dc11c0fd845fe665f439a", "7f231cc63c3b18dc52762049c34129b2b1beae8da85657e1ba7e92af32d20e7c"),
];

struct Chain {
    n: u32,
    rows: Vec<(String, String, String)>,
    keys: KeySet,
}

impl Chain {
    fn mint(&mut self, key: [u8; 32], kas: u64, daa: u64) -> KeyUtxo {
        self.n += 1;
        let mut b = [0x7au8; 32];
        b[..4].copy_from_slice(&self.n.to_le_bytes());
        KeyUtxo { outpoint: TransactionOutpoint { transaction_id: TransactionId::from_bytes(b), index: 0 }, amount: kas * KAS, daa, key }
    }
    fn sign(&mut self, name: &str, b: &OBuilt) -> OSigned {
        let sg = b.sign(&self.keys).unwrap_or_else(|e| panic!("{name}: {e}"));
        let v = kpf_protocol::verify::verify(&sg.tx, &sg.entries);
        assert!(v.is_ok(), "{name}: {:?}", v.errors());
        self.rows.push((name.into(), sg.tx.id().to_string(), sha(serde_json::to_string(&rpc_json(&sg.tx)).unwrap().as_bytes())));
        sg
    }
}

/// The object `s` created at output index `i`.
fn nth<T>(sg: &OSigned, i: usize, s: T, daa: u64) -> OLive<T> {
    let o = &sg.tx.outputs[i];
    OLive { s, value: o.value, id: o.covenant.map(|c| c.covenant_id), op: TransactionOutpoint { transaction_id: sg.tx.id(), index: i as u32 }, daa }
}

/// Defect 1 of the first live run: the vouch input is the underwriter's own plain UTXO; `KpfPairO.propose` checks only
/// that it is a P2PK input of the underwriter with a SIGHASH_ALL signature, never where its value goes. What is left of
/// it must reach the underwriter in an explicit output (never the funder's change), and the underwriter's signature
/// must cover that output: redirecting it breaks the vouch input. A liquidation's proposer is the underwriter: the
/// vouch input funds everything the claim adds to the contract outputs, `adds` = pending part + remainder - the pair
/// (the bond, and the carrier of the new part when n < s; live run 1 found the funder paying that carrier while a
/// cure refunded it to the underwriter). The owner gets `value - adds`, no output if that is zero, and the funder only
/// pays the fee. A close's bond is the closer's (the owner gets the whole value back).
fn assert_vouch_returned(sg: &OSigned, vouch: &KeyUtxo, funder: &[u8; 32], is_close: bool, adds: i64) {
    let back = if is_close { vouch.amount } else { vouch.amount - adds as u64 };
    let closer_signs = is_close;
    let to_owner: Vec<usize> = (0..sg.tx.outputs.len()).filter(|&i| sg.tx.outputs[i].script_public_key == p2pk_spk(&vouch.key)).collect();
    assert_eq!(to_owner.len(), 1, "exactly one output goes to the underwriter: {:?}", sg.output_labels);
    assert_eq!(sg.tx.outputs[to_owner[0]].value, back, "the underwriter gets exactly {back}: {:?}", sg.output_labels);
    // What the claim adds is inside the contract outputs: they hold the pair's value plus `adds`.
    let ins_contract = sg.entries[0].amount as i64;
    let outs_contract: i64 = sg.output_labels.iter().zip(&sg.tx.outputs).filter(|(l, _)| *l == "pending part" || *l == "remainder").map(|(_, o)| o.value as i64).sum();
    assert_eq!(outs_contract, ins_contract + adds, "the pending part and the remainder hold the pair's value plus {adds}");
    if !is_close {
        // The funder pays only the fee: its change is its input less the fee.
        let fin: i64 = sg.entries.iter().enumerate().filter(|(i, _)| sg.input_labels[*i] != "pair" && !sg.input_labels[*i].contains("vouch")).map(|(_, e)| e.amount as i64).sum();
        let fout: i64 = sg.tx.outputs.iter().filter(|o| o.script_public_key == p2pk_spk(funder)).map(|o| o.value as i64).sum();
        let total_in: i64 = sg.entries.iter().map(|e| e.amount as i64).sum();
        let total_out: i64 = sg.tx.outputs.iter().map(|o| o.value as i64).sum();
        assert_eq!(fin - fout, total_in - total_out, "the funder pays exactly the network fee");
    }
    let j = sg.input_labels.iter().position(|l| l.contains("vouch")).expect("a vouch input");
    assert_eq!(sg.entries[j].amount, vouch.amount);
    // SIGHASH_ALL: one 65-byte signature, 0x01 last (the contract's `vouched` requires exactly this).
    let ss = &sg.tx.inputs[j].signature_script;
    assert!(ss.len() == 66 && ss[0] == 0x41 && ss[65] == 0x01, "the vouch signature is SIGHASH_ALL");
    // The funder's change is a different output.
    let change = sg.output_labels.iter().position(|l| l == "change").expect("change");
    assert_ne!(change, to_owner[0]);
    assert_eq!(sg.tx.outputs[change].script_public_key, p2pk_spk(funder));
    // Anyone who rebuilds the transaction to keep the remainder breaks the owner's signature.
    let mut stolen = sg.tx.clone();
    stolen.outputs[to_owner[0]].script_public_key = p2pk_spk(funder);
    stolen.finalize();
    let v = kpf_protocol::verify::verify(&stolen, &sg.entries);
    assert!(!v.input_ok(j), "an output redirected after the owner signed must fail the vouch input");
    // A liquidation has no other signature: the pair's own script accepts the redirected output (only a close carries the closer's SIGHASH_ALL signature, which also covers it).
    assert_eq!(v.input_ok(0), !closer_signs, "{:?}: the pair's script does not look at where the vouch value goes", v.errors());
}

#[test]
fn every_oracle_shape_builds_signs_and_verifies() {
    let seeds = [41u8, 42, 31, 70, 46, 47, 60];
    let mut keys = KeySet::new();
    for s in seeds {
        keys.add_secret(&[s; 32]).unwrap();
    }
    let k = |s: u8| pubkey_of(&[s; 32]).unwrap();
    let (lk, sk, uk, stk, tk, ek) = (k(41), k(42), k(31), k(70), k(46), k(60));
    let mut c = Chain { n: 0, rows: vec![], keys };
    let r = MIN_FEE_RATE;
    let m = samples::market();
    let step = m.bond.step as u64;

    // The underwriter's slot, the stakes, the orders.
    let f = c.mint(uk, 30_100, 0);
    let sg = c.sign("create slot", &create_slot(&m, uk, 30_000 * KAS, 0, &[f], &uk, r).unwrap());
    let mut slot = nth(&sg, 1, Bond::new(uk, m.bond_market(), 0), 0);
    let f = c.mint(stk, 1_010, 0);
    let sg = c.sign("create stake", &create_stake(&m, stk, 100 * KAS, &[f], &stk, r).unwrap());
    let opener = nth(&sg, 1, Js::free(stk, m.judge_market()), 0);
    let f = c.mint(uk, 1_010, 0);
    let sg = c.sign("create the underwriter's stake", &create_stake(&m, uk, 130 * KAS, &[f], &uk, r).unwrap());
    let defence = nth(&sg, 1, Js::free(uk, m.judge_market()), 0);
    let lo = samples::order(&m, LONG, lk);
    let so = samples::order(&m, SHORT, sk);
    let f = c.mint(lk, 3_100, 1_000_000);
    let sg = c.sign("create long order", &create_order(&lo, 3_000 * KAS, &[f], &lk, r).unwrap());
    let lorder = nth(&sg, 1, lo.clone(), 1_000_000);
    let f = c.mint(sk, 5_100, 1_000_000);
    let sg = c.sign("create short order", &create_order(&so, 5_000 * KAS, &[f], &sk, r).unwrap());
    let sorder = nth(&sg, 1, so.clone(), 1_000_000);

    // T1' open: 1,000 units at 10 KAS, cover rate 6, premium 0.02 per term, expiry 9 steps (27,216,000).
    let terms = OpenTerms { n: 100_000_000_000, x: 1_000_000_000, cover_rate: 600_000_000, premium: 2_000_000, expiry: 9 * step as i64, lock: 7_000_000 };
    let f = c.mint(ek, 10, 0);
    let (b, pa) = open(&lorder, &sorder, &slot, &terms, &[f], &ek, r).unwrap();
    let sg = c.sign("open pair A", &b);
    let pair_a = nth(&sg, 2, pa, 7_000_000);
    let lorder = nth(&sg, 0, OOpen { size_left: lo.size_left - terms.n, ..lo.clone() }, 7_000_000);
    let sorder = nth(&sg, 1, OOpen { size_left: so.size_left - terms.n, ..so.clone() }, 7_000_000);
    slot = nth(&sg, 3, match &sg.created.iter().find(|(i, _, _)| *i == 3).unwrap().1 {
        OObj::Slot(b) => b.clone(),
        _ => unreachable!(),
    }, 7_000_000);
    assert_eq!(slot.s.counted_at(terms.expiry), 6_000 * KAS as i64);
    // A second pair from the orders' rests, expiry 11 steps.
    let terms_b = OpenTerms { expiry: 11 * step as i64, lock: 7_100_000, ..terms };
    let f = c.mint(ek, 10, 0);
    let (b, pb) = open(&lorder, &sorder, &slot, &terms_b, &[f], &ek, r).unwrap();
    let sg = c.sign("open pair B", &b);
    let pair_b = nth(&sg, 2, pb, 7_100_000);
    slot = nth(&sg, 3, match &sg.created.iter().find(|(i, _, _)| *i == 3).unwrap().1 {
        OObj::Slot(b) => b.clone(),
        _ => unreachable!(),
    }, 7_100_000);

    // A false liquidation of A's long, contested, refuted: the part restored, the bond paid.
    let vouch = c.mint(uk, 700, 0);
    let f = c.mint(ek, 10, 0);
    let (b, w) = propose(&pair_a, &ClaimTerms { kind: KIND_LIQ, side: LONG, n: terms.n, price: 920_000_000, t0: 7_999_000 }, &vouch, &[f], &ek, r).unwrap();
    let sg = c.sign("propose a liquidation", &b);
    assert_vouch_returned(&sg, &vouch, &ek, false, w.bond);
    // The vouch input funds a liquidation's bond: it must hold at least the bond, and what is left must not be dust; an
    // exact fit pays the underwriter nothing back (no output). Not part of the golden chain.
    {
        let term = ClaimTerms { kind: KIND_LIQ, side: LONG, n: terms.n, price: 920_000_000, t0: 7_999_000 };
        let fund = KeyUtxo { outpoint: TransactionOutpoint { transaction_id: TransactionId::from_bytes([0x11; 32]), index: 0 }, amount: 10 * KAS, daa: 0, key: ek };
        let with = |amount: u64| KeyUtxo { amount, ..vouch.clone() };
        let e = propose(&pair_a, &term, &with(w.bond as u64 - 1), &[fund.clone()], &ek, r).unwrap_err().to_string();
        assert!(e.contains("less than the") && e.contains("the claim's bond"), "{e}");
        let e = propose(&pair_a, &term, &with(w.bond as u64 + 1), &[fund.clone()], &ek, r).unwrap_err().to_string();
        assert!(e.contains("too small for an output of its own"), "{e}");
        let (b, _) = propose(&pair_a, &term, &with(w.bond as u64), &[fund.clone()], &ek, r).unwrap();
        let s = b.sign(&c.keys).unwrap();
        assert!(kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok());
        assert!(s.tx.outputs.iter().all(|o| o.script_public_key != p2pk_spk(&uk)), "an exact fit leaves nothing to pay back");
        // A partial liquidation (n < s): the claim adds the bond and the carrier of the new part, and the vouch funds
        // both (live run 1: the funder paid the carrier and the cure refunded bond + carrier to the underwriter).
        let half = ClaimTerms { n: terms.n / 2, ..term };
        let (b, wh) = propose(&pair_a, &half, &vouch, &[fund.clone()], &ek, r).unwrap();
        let adds = funded_by_proposer(&pair_a, &wh);
        assert_eq!(adds, wh.bond + m.p.carrier, "a partial claim adds the bond and one carrier");
        assert_eq!(wh.refund, adds, "what the proposer funds is exactly the pending part's refund");
        let s = b.sign(&c.keys).unwrap();
        assert!(kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok());
        assert_vouch_returned(&s, &vouch, &ek, false, adds);
        let e = propose(&pair_a, &half, &with(adds as u64 - 1), &[fund.clone()], &ek, r).unwrap_err().to_string();
        assert!(e.contains("the carrier of the new part"), "{e}");
        let (b, _) = propose(&pair_a, &half, &with(adds as u64), &[fund], &ek, r).unwrap();
        let s = b.sign(&c.keys).unwrap();
        assert!(kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok());
        assert!(s.tx.outputs.iter().all(|o| o.script_public_key != p2pk_spk(&uk)), "an exact fit of bond + carrier leaves nothing to pay back");
    }
    let pend = nth(&sg, 0, w, 8_000_000);
    let f = c.mint(ek, 10, 0);
    let (b, cw, d) = contest(&pend, &opener, 50_000_000, &[f], &ek, r, &Trust::Untraced).unwrap();
    let sg = c.sign("contest", &b);
    let (cpart, dispute) = (nth(&sg, 0, cw, 8_000_100), nth(&sg, 2, d, 8_000_100));
    let receipt = nth(&sg, 1, Js::receipt(stk, m.judge_market(), kpf_protocol::judge::NO, opener.value as i64, 0, 8_000_000), 8_000_100);
    let f = c.mint(ek, 10, 0);
    let (b, dw, pool) = decide(&dispute, &cpart, &[f], &ek, r).unwrap();
    assert_eq!(dw.status, REFUTED);
    let sg = c.sign("resolve and decide (refuted)", &b);
    let (decided, pool) = (nth(&sg, 1, dw, 8_432_100), nth(&sg, 0, pool, 8_432_100));
    let f = c.mint(ek, 10, 0);
    let (b, s) = finalize(&decided, &[f], &ek, r).unwrap();
    let sg = c.sign("finalize refuted (restore)", &b);
    let Settlement::Restore { part, .. } = s else { panic!() };
    let restored = nth(&sg, 1, part, 8_600_000);
    let f = c.mint(ek, 10, 0);
    let (b, p2) = pool_settle(&pool, &[receipt], &[f], &ek, r).unwrap();
    let sg = c.sign("settle the opener's receipt", &b);
    let won = nth(&sg, 1, Js::free(stk, m.judge_market()), 8_600_000);
    c.sign("close the pool", &pool_close(&nth(&sg, 0, p2, 8_600_000), &ek, r).unwrap());

    // A true liquidation of the restored part, contested, defended, upheld: forfeit and keep.
    let vouch = c.mint(uk, 700, 0);
    let f = c.mint(ek, 10, 0);
    let (b, w) = propose(&restored, &ClaimTerms { kind: KIND_LIQ, side: LONG, n: terms.n, price: 920_000_000, t0: 8_999_000 }, &vouch, &[f], &ek, r).unwrap();
    let sg = c.sign("propose again", &b);
    assert_vouch_returned(&sg, &vouch, &ek, false, w.bond);
    let birth = sg.tx.clone();
    let pend = nth(&sg, 0, w, 9_000_000);
    let opener2 = won; // the winner's stake came back fresh: too young to open a dispute on this claim
    let f = c.mint(stk, 1_010, 0);
    let sg2 = c.sign("create a second stake", &create_stake(&m, stk, 100 * KAS, &[f], &stk, r).unwrap());
    let opener3 = nth(&sg2, 1, Js::free(stk, m.judge_market()), 0);
    assert!(contest(&pend, &opener2, 50_000_000, &[], &ek, r, &Trust::Untraced).is_err(), "stake younger than M_age before the claim");
    let f = c.mint(ek, 10, 0);
    // Review 7 (R7-F2): the claim traced to its propose before the stake is committed.
    let ct = trace_claim(pend.id.unwrap(), &birth, &pend.s.redeem(kpf_protocol::oracle_artifacts::kit())).expect("the claim traces to its propose");
    assert_eq!(ct.pair_in, restored.op);
    let (b, cw, d) = contest(&pend, &opener3, 50_000_000, &[f], &ek, r, &Trust::Traced(&ct)).unwrap();
    let sg = c.sign("contest a true claim", &b);
    let opening = sg.tx.clone();
    let (cpart, dispute) = (nth(&sg, 0, cw, 9_000_100), nth(&sg, 2, d, 9_000_100));
    let r_no = nth(&sg, 1, Js::receipt(stk, m.judge_market(), kpf_protocol::judge::NO, 100 * KAS as i64, 0, 9_000_000), 9_000_100);
    let f = c.mint(ek, 10, 0);
    let dt = trace_dispute(&dispute, &opening, &birth).expect("the dispute traces to its opening and its claim to a propose");
    assert_eq!(dt, ct);
    let (b, d2) = deposit(&dispute, YES, &[defence.clone()], &[f], &ek, r, &Trust::Traced(&dt)).unwrap();
    let sg = c.sign("deposit the defence", &b);
    let dispute = nth(&sg, 0, d2, 9_000_200);
    let r_yes = nth(&sg, 1, Js::receipt(uk, m.judge_market(), YES, 130 * KAS as i64, 0, 9_000_000), 9_000_200);
    let f = c.mint(ek, 10, 0);
    let (b, dw, pool) = decide(&dispute, &cpart, &[f], &ek, r).unwrap();
    assert_eq!(dw.status, UPHELD);
    let sg = c.sign("resolve and decide (upheld)", &b);
    let (decided, pool) = (nth(&sg, 1, dw, 9_600_000), nth(&sg, 0, pool, 9_600_000));
    let f = c.mint(ek, 10, 0);
    let (b, s) = finalize(&decided, &[f], &ek, r).unwrap();
    let sg = c.sign("finalize upheld (forfeit)", &b);
    let Settlement::Forfeit { part, .. } = s else { panic!() };
    let forfeited = nth(&sg, 1, part, 9_600_000);
    let f = c.mint(ek, 10, 0);
    let (b, p2) = pool_settle(&pool, &[r_yes, r_no], &[f], &ek, r).unwrap();
    let sg = c.sign("settle a winning and a losing receipt", &b);
    let won_yes = nth(&sg, 1, Js::free(uk, m.judge_market()), 9_600_000);
    c.sign("close the pool with a burn", &pool_close(&nth(&sg, 0, p2, 9_600_000), &ek, r).unwrap());

    // B: a liquidation the long cures; a renewal; listings; a transfer; a mutual close.
    let vouch = c.mint(uk, 700, 0);
    let f = c.mint(ek, 10, 0);
    let (b, w) = propose(&pair_b, &ClaimTerms { kind: KIND_LIQ, side: LONG, n: terms.n, price: 920_000_000, t0: 7_999_000 }, &vouch, &[f], &ek, r).unwrap();
    let sg = c.sign("propose on B", &b);
    assert_vouch_returned(&sg, &vouch, &ek, false, w.bond);
    let pend = nth(&sg, 0, w, 8_000_000);
    let f = c.mint(lk, 2_000, 0);
    let (b, cured) = cure(&pend, &[f], &lk, r).unwrap();
    let sg = c.sign("cure", &b);
    let pair_b = nth(&sg, 1, cured, 8_100_000);
    let f = c.mint(ek, 10, 0);
    let (b, rs) = renew(&[pair_b.clone()], &slot, 30_240_000, &[f], &ek, r).unwrap();
    let sg = c.sign("renew B", &b);
    let pair_b = nth(&sg, 0, rs[0].clone(), 30_240_000);
    slot = nth(&sg, 1, match &sg.created.iter().find(|(i, _, _)| *i == 1).unwrap().1 {
        OObj::Slot(b) => b.clone(),
        _ => unreachable!(),
    }, 30_240_000);
    let f = c.mint(ek, 10, 0);
    let (b, a) = amend(&pair_b, LONG, 1_000_000_000, 0, 0, &[f], &ek, r).unwrap();
    let sg = c.sign("amend: list the long", &b);
    let listed = nth(&sg, 0, a, 30_300_000);
    let f = c.mint(tk, 2_000, 0);
    let (b, t) = transfer(&listed, LONG, 1_000_000_000, listed.s.bl, tk, &[f], &tk, r).unwrap();
    let sg = c.sign("transfer the long to a signing taker", &b);
    let moved = nth(&sg, 1, t, 30_400_000);
    let f = c.mint(ek, 10, 0);
    let (b, a) = amend(&moved, LONG, 1_000_000_000, 0, 0, &[f], &ek, r).unwrap();
    let sg = c.sign("amend: the taker lists", &b);
    let l1 = nth(&sg, 0, a, 30_500_000);
    let f = c.mint(ek, 10, 0);
    let (b, a) = amend(&l1, SHORT, 1_000_000_000, 0, 0, &[f], &ek, r).unwrap();
    let sg = c.sign("amend: the short lists", &b);
    let both = nth(&sg, 0, a, 30_600_000);
    let f = c.mint(ek, 10, 0);
    c.sign("mutual close", &mutual_close(&both, 1_000_000_000, &[f], &ek, r).unwrap());
    // The forfeited part lapses (its owner holds both sides) and settles at its entry.
    let f = c.mint(ek, 10, 0);
    c.sign("lapse settle", &lapse_settle(&forfeited, &[f], &ek, r).unwrap());

    // A third pair closed cooperatively.
    let lo3 = samples::order(&m, LONG, lk);
    let so3 = samples::order(&m, SHORT, sk);
    let f = c.mint(lk, 3_100, 20_000_000);
    let sg = c.sign("create long order 3", &create_order(&lo3, 3_000 * KAS, &[f], &lk, r).unwrap());
    let l3 = nth(&sg, 1, lo3, 20_000_000);
    let f = c.mint(sk, 5_100, 20_000_000);
    let sg = c.sign("create short order 3", &create_order(&so3, 5_000 * KAS, &[f], &sk, r).unwrap());
    let s3 = nth(&sg, 1, so3, 20_000_000);
    let f = c.mint(ek, 10, 0);
    let terms_c = OpenTerms { expiry: 15 * step as i64, lock: 30_300_000, ..terms };
    let (b, pc) = open(&l3, &s3, &slot, &terms_c, &[f], &ek, r).unwrap();
    let sg = c.sign("open pair C", &b);
    let pair_c = nth(&sg, 2, pc, 30_300_000);
    slot = nth(&sg, 3, match &sg.created.iter().find(|(i, _, _)| *i == 3).unwrap().1 {
        OObj::Slot(b) => b.clone(),
        _ => unreachable!(),
    }, 30_300_000);
    c.sign("cooperative close", &coop_close(&pair_c, 1_000 * KAS, r).unwrap());

    // The slot: advance, split, unbond once every cover's liability ended, withdraw.
    let f = c.mint(ek, 10, 0);
    let (b, b2) = slot_advance(&slot, 1, 40_000_000, &[f], &ek, r).unwrap();
    let sg = c.sign("advance the slot", &b);
    slot = nth(&sg, 0, b2, 40_000_000);
    let f = c.mint(ek, 10, 0);
    let (b, ba, _bb) = slot_split(&slot, &slot.s.ring.clone(), 29_000 * KAS, &[f], &ek, r).unwrap();
    let sg = c.sign("split the slot", &b);
    let child = nth(&sg, 0, ba, 40_100_000);
    let last = (child.s.j0 as usize..child.s.j0 as usize + RING).rev().find(|&j| child.s.ring[j - child.s.j0 as usize] != 0).map(|j| j as i64 + 1).unwrap_or(child.s.j0);
    let steps = last - child.s.j0;
    let free_at = (last * m.bond.step + m.bond.tail) as u64;
    let f = c.mint(ek, 10, 0);
    let (b, ub) = slot_unbond(&child, steps, free_at, &[f], &ek, r).unwrap();
    let sg = c.sign("unbond child A", &b);
    let unb = nth(&sg, 0, ub, free_at);
    c.sign("withdraw child A", &slot_withdraw(&unb, &uk, r).unwrap());
    // A stake leaves: unbond, withdraw.
    let f = c.mint(ek, 10, 0);
    let (b, su) = stake_unbond(&won_yes, &[f], &ek, r).unwrap();
    let sg = c.sign("unbond a stake", &b);
    c.sign("withdraw a stake", &stake_withdraw(&nth(&sg, 0, su, 10_000_000), &uk, r).unwrap());

    // A close claim (kind 1): the long closes at a vouched 10.5 on a fourth pair.
    let lo4 = samples::order(&m, LONG, lk);
    let so4 = samples::order(&m, SHORT, sk);
    let f = c.mint(lk, 3_100, 1_000_000);
    let sg = c.sign("create long order 4", &create_order(&lo4, 3_000 * KAS, &[f], &lk, r).unwrap());
    let l4 = nth(&sg, 1, lo4, 1_000_000);
    let f = c.mint(sk, 5_100, 1_000_000);
    let sg = c.sign("create short order 4", &create_order(&so4, 5_000 * KAS, &[f], &sk, r).unwrap());
    let s4 = nth(&sg, 1, so4, 1_000_000);
    let f = c.mint(uk, 20_100, 0);
    let sg = c.sign("create a second slot", &create_slot(&m, uk, 20_000 * KAS, 0, &[f], &uk, r).unwrap());
    let slot2 = nth(&sg, 1, Bond::new(uk, m.bond_market(), 0), 0);
    let f = c.mint(ek, 10, 0);
    let (b, pd) = open(&l4, &s4, &slot2, &terms, &[f], &ek, r).unwrap();
    let sg = c.sign("open pair D", &b);
    let pair_d = nth(&sg, 2, pd, 7_000_000);
    // Review 11: a close is the party's own (no vouch input): the long prices it and posts the bond.
    let f = c.mint(lk, 700, 0);
    let vouch = c.mint(uk, 700, 0);
    assert!(propose(&pair_d, &ClaimTerms { kind: KIND_CLOSE, side: LONG, n: terms.n, price: 1_050_000_000, t0: 7_999_000 }, &vouch, &[f.clone()], &lk, r).is_err(), "propose builds liquidations only");
    let (b, w) = propose_close(&pair_d, LONG, terms.n, 1_050_000_000, 7_999_000, &[f], &lk, r).unwrap();
    let sg = c.sign("propose a close", &b);
    assert!(sg.tx.outputs.iter().all(|o| o.script_public_key != p2pk_spk(&uk)), "no underwriter output: no vouch input");
    let outs_contract: i64 = sg.output_labels.iter().zip(&sg.tx.outputs).filter(|(l, _)| l.starts_with("pending part")).map(|(_, o)| o.value as i64).sum();
    assert_eq!(outs_contract, pair_d.value as i64 + w.bond, "the closer posts the bond");
    let pend = nth(&sg, 0, w, 8_000_000);
    let f = c.mint(ek, 10, 0);
    c.sign("finalize the close", &finalize(&pend, &[f], &ek, r).unwrap().0);

    // The golden vectors.
    if std::env::var("KPF_GOLDEN").as_deref() == Ok("print") {
        for (n, id, h) in &c.rows {
            println!("    (\"{n}\", \"{id}\", \"{h}\"),");
        }
    } else if !GOLDEN.is_empty() {
        assert_eq!(c.rows.len(), GOLDEN.len());
        for ((n, id, h), (gn, gid, gh)) in c.rows.iter().zip(GOLDEN) {
            assert_eq!((n.as_str(), id.as_str(), h.as_str()), (*gn, *gid, *gh), "{n}: a contract or a builder rule changed (KPF_GOLDEN=print)");
        }
    }
    println!("{} oracle transactions built, signed and verified", c.rows.len());
}

/// Live run 2: a renewal of a 1-unit pair failed verification, "mass exceeds block limits: storage: 510287", because its
/// 0.02 KAS premium had an output of its own (KIP-9 charges 10^12 / value grams per output: 500,000 for 0.02 KAS, the
/// whole block limit). A 1-unit pair with the testnet defaults is opened, claimed, cured, renewed, claimed again,
/// settled and lapse-settled within the block limits: the premium joins the underwriter's change when the underwriter
/// funds the transaction, or one output with an underwriter UTXO; otherwise the builder refuses and names the output
/// and the least value it would need. Not part of the golden chain.
#[test]
fn a_one_unit_pair_with_the_testnet_defaults_fits_the_mass_limits_on_every_path() {
    let seeds = [41u8, 42, 31, 60];
    let mut keys = KeySet::new();
    for s in seeds {
        keys.add_secret(&[s; 32]).unwrap();
    }
    let k = |s: u8| pubkey_of(&[s; 32]).unwrap();
    let (lk, sk, uk, ek) = (k(41), k(42), k(31), k(60));
    let mut c = Chain { n: 100, rows: vec![], keys };
    let r = MIN_FEE_RATE;
    let m = samples::market();
    let step = m.bond.step;
    let unit = m.p.scale;
    let f = c.mint(uk, 1_010, 0);
    let sg = c.sign("create slot", &create_slot(&m, uk, 1_000 * KAS, 0, &[f], &uk, r).unwrap());
    let slot = nth(&sg, 1, Bond::new(uk, m.bond_market(), 0), 0);
    let lo = OOpen { size_left: unit, ..samples::order(&m, LONG, lk) };
    let so = OOpen { size_left: unit, ..samples::order(&m, SHORT, sk) };
    let f = c.mint(lk, 10, 1_000_000);
    let sg = c.sign("create long order", &create_order(&lo, 3 * KAS, &[f], &lk, r).unwrap());
    let lorder = nth(&sg, 1, lo.clone(), 1_000_000);
    let f = c.mint(sk, 10, 1_000_000);
    let sg = c.sign("create short order", &create_order(&so, 4 * KAS, &[f], &sk, r).unwrap());
    let sorder = nth(&sg, 1, so.clone(), 1_000_000);
    let terms = OpenTerms { n: unit, x: 1_000_000_000, cover_rate: 600_000_000, premium: 2_000_000, expiry: 9 * step, lock: 7_000_000 };
    // An executor funding the open: the 0.02 KAS premium alone cannot be mined, and the builder says so.
    let e = open(&lorder, &sorder, &slot, &terms, &[c.mint(ek, 10, 0)], &ek, r).unwrap_err().to_string();
    assert!(e.contains("(premium, 0.02 KAS) is too small to be mined: at least") && e.contains("--uw-utxo"), "{e}");
    // With an underwriter UTXO the premium joins it in one output.
    let extra = c.mint(uk, 1, 0);
    let (b, _) = open_with(&lorder, &sorder, &slot, &terms, &[c.mint(ek, 10, 0)], &ek, r, Some(&extra)).unwrap();
    let sg = c.sign("open, the premium with an underwriter UTXO", &b);
    let to_uw: Vec<u64> = sg.tx.outputs.iter().filter(|o| o.script_public_key == p2pk_spk(&uk)).map(|o| o.value).collect();
    assert_eq!(to_uw, vec![KAS + 2_000_000], "one output: the underwriter's UTXO and the premium");
    // The underwriter funding it: the premium is in its change.
    let (b, p) = open(&lorder, &sorder, &slot, &terms, &[c.mint(uk, 10, 0)], &uk, r).unwrap();
    assert!(!b.output_labels.iter().any(|l| l == "premium"), "{:?}", b.output_labels);
    let sg = c.sign("open, funded by the underwriter", &b);
    assert!(sg.mass.within_block_limits(), "{:?}", sg.mass);
    let pair = nth(&sg, 2, p, 7_000_000);
    let slot = nth(&sg, 3, match &sg.created.iter().find(|(i, _, _)| *i == 3).unwrap().1 {
        OObj::Slot(b) => b.clone(),
        _ => unreachable!(),
    }, 7_000_000);
    // A true liquidation of the long at 9.20, cured.
    let vouch = c.mint(uk, 2, 0);
    let (b, w) = propose(&pair, &ClaimTerms { kind: KIND_LIQ, side: LONG, n: unit, price: 920_000_000, t0: 7_999_000 }, &vouch, &[c.mint(ek, 10, 0)], &ek, r).unwrap();
    let sg = c.sign("propose a liquidation of the 1-unit pair", &b);
    let pend = nth(&sg, 0, w, 8_000_000);
    let (b, cured) = cure(&pend, &[c.mint(lk, 10, 0)], &lk, r).unwrap();
    let sg = c.sign("cure", &b);
    let pair = nth(&sg, 1, cured, 8_100_000);
    // The renewal: refused with an executor's funding alone, accepted with the underwriter's.
    let lock = (9 * step - step) as u64;
    let e = renew(&[pair.clone()], &slot, lock, &[c.mint(ek, 10, 0)], &ek, r).unwrap_err().to_string();
    assert!(e.contains("(premium, 0.02 KAS) is too small to be mined"), "{e}");
    let extra = c.mint(uk, 1, 0);
    let (b, _) = renew_with(&[pair.clone()], &slot, lock, &[c.mint(ek, 10, 0)], &ek, r, Some(&extra)).unwrap();
    c.sign("renew, the premium with an underwriter UTXO", &b);
    let (b, rs) = renew(&[pair.clone()], &slot, lock, &[c.mint(uk, 10, 0)], &uk, r).unwrap();
    let sg = c.sign("renew, funded by the underwriter", &b);
    assert!(sg.mass.within_block_limits(), "{:?}", sg.mass);
    let pair = nth(&sg, 0, rs[0].clone(), lock);
    // A second liquidation, uncontested: the forfeit, then the lapse settle of the forfeited part.
    let t0 = (lock + 1_000_000) as i64;
    let vouch = c.mint(uk, 2, 0);
    let (b, w) = propose(&pair, &ClaimTerms { kind: KIND_LIQ, side: LONG, n: unit, price: 900_000_000, t0 }, &vouch, &[c.mint(ek, 10, 0)], &ek, r).unwrap();
    let sg = c.sign("propose again", &b);
    let pend = nth(&sg, 0, w, t0 as u64);
    let (b, s) = finalize(&pend, &[c.mint(ek, 10, 0)], &ek, r).unwrap();
    let sg = c.sign("finalize: forfeit", &b);
    let Settlement::Forfeit { part, .. } = s else { panic!("{s:?}") };
    let forfeited = nth(&sg, 1, part, t0 as u64 + 300_000);
    let f = c.mint(ek, 10, 0);
    let sg = c.sign("lapse settle", &lapse_settle(&forfeited, &[f], &ek, r).unwrap());
    assert!(sg.mass.within_block_limits(), "{:?}", sg.mass);
    // The wallet's text names the premium for this pair.
    let wn = payout_warnings(&m, unit, 100_000_000, 200_000_000, 600_000_000, 2_000_000);
    assert!(wn.iter().any(|x| x.contains("the premium of a term is 0.02")), "{wn:?}");
}
