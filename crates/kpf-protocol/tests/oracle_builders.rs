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
        let src = std::fs::read_to_string(contracts().join(format!("{name}.sil"))).unwrap().replace("\r\n", "\n");
        assert_eq!(c.source_sha256, sha(src.as_bytes()), "contracts/{name}.sil changed: regenerate the artifacts");
        assert_eq!(c.artifact_sha256, sha(text.replace("\r\n", "\n").as_bytes()), "{name}");
    }
    let sums = std::fs::read_to_string(contracts().join("artifacts/oracle/SHA256SUMS")).unwrap();
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
    m2.feed_hash = feed_hash_of(samples::FEED.replace("0.25 %", "0.26 %").as_bytes());
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

const GOLDEN_MARKET: &str = "bc0dcff4228ba2ca641b1311e29f35f69d07cbb01f2f1cc059d6469cc85f850c";

const GOLDEN: &[(&str, &str, &str)] = &[
    ("create slot", "0a7a8004089c7a8cb98c3ce06d8c5dc6b18fddd50edb78c0530a5d684828c1b0", "e0b47cd5efe2f0b88a8a2ca62e8f773936a1212c40461633440e626bdef6e51d"),
    ("create stake", "0d3ef8323743d68a7493c8a5720b81caa19e815d1c1230bef30e260055e098dc", "756a94862bff187828e05419028862be115295f55ab98960606765ea9314d6a4"),
    ("create the underwriter's stake", "0ea1f28e1e306f8ef537842e79110f6eb70d4130e2cb148cfd73c9309bdba802", "9bbc03285f485dee6a770a144886b91e127f878630db822a4f3764465d327804"),
    ("create long order", "0df5a151475fd910656d9a2c643c889fdf40c00d9d132d9a9467a5070967a860", "3b105eed7ea3c4b4f14f5b1607aefac1ad0be6446834ee54526028d276ab0dca"),
    ("create short order", "a261a1f64547c62c5e4918d94c6014647227afb49f454395d5351a7e56033c42", "e4f415783cf0860c3d2b153e239853cde0ea529caab165a764f7009d4c702e35"),
    ("open pair A", "c3517e48766ec5d33dca34ba215c3687ebed66c7922840edadd684e62bf764a5", "e8029d87e63162b51e8465f1ca515e0309cc142e9dbe7bec632c00b49959c88a"),
    ("open pair B", "5484ca2b13da613629de21cc0fe2e13f7cccd3515ec4742fcf5d574f2328b262", "44262bb1220cbb5f5fb3caf9756692c4c8ec61bbd0b8a87ef4fa3b194bb3a2c7"),
    ("propose a liquidation", "aac2697ef3f3ac7cdd4ff351f3608608baf61f6878a1eca24a4bae54cc3bf8af", "036fc90c046bf9c147c43b5dfac7374d29f3ce8a88d7db2825e56dbb026c60f9"),
    ("contest", "e142e97b774a086b06bf1af8f475ed9703c04ff48bf058ebf33ae232d16b1511", "7fb24ee6b1e685838a792ec1f8408cd93fde96c0a715e885a7c75410507e4ef3"),
    ("resolve and decide (refuted)", "0f1f7a8dcfeeb2331e7c52e862e3e7a71aadd7d3ea1f5795772b8bcbadb2650f", "c5d2ac3c91dcff05c5720ee2a1b438e95f4cace7fc2c5e0a0fcadfd7e35a9d7b"),
    ("finalize refuted (restore)", "ee90ec436a60b881e8adb813c3ae84bc81da5c238ecf233ac024d6555736c596", "3df455a13f8362f738179e6c66f31677414650d495bee8ac458aa65d67fc3cdc"),
    ("settle the opener's receipt", "e7082d1dfb9905633cc5de8193ea8c9f0f68493565e0a5e0d91684ce1f77438d", "5d303cf2faaee61b675bb8502121925054a29a9a7304d43844bcebc77cbd3393"),
    ("close the pool", "6738a46790d767345c0a8f10511959d34c7b9ec7abac12dde153401fa0745858", "9a57138e1aa9d024b123e825c7fb4a6a76316d2b8c75a05a82a617154e3d514e"),
    ("propose again", "37aad25ceb1d9dd7ec623eb1c76826916a3199b9076b32555341645544692179", "f1dd37bb5647431bd8b538598f767c68dba3d572f228bba5e7afaa885ff81bf6"),
    ("create a second stake", "6d5b6494e409a821c1c737cd4dae2646187878512d620496acf6115a62f69d11", "36c7ee53c89d632e0bdf50dc144f012b700b83148abcd0c83bfece53345ffd2b"),
    ("contest a true claim", "3aae7bd135097884173210813c6bca74e2bbfded2605578cee85e28dbd5c094c", "14f6cd290a8a0ae83d2a4df8e113415d266f3a8bcc738760281be4658253b16f"),
    ("deposit the defence", "73bc6a66eda9aacb3f5c1359d5505f4a6ddf3261eb90c1dc07c053e464ab8098", "5889704757a6c0148259109445778fd11f617987fd0429a2dde178040065e26c"),
    ("resolve and decide (upheld)", "bccecd8e310e1d154104e53db85dfae4a6ee0ec815c88e954c5f8ce4bc0ad328", "e5b0d367af2d2e57fb4188436fad37005ab3e04c892684f54e7f26a00ebd9f58"),
    ("finalize upheld (forfeit)", "98be52cc142244c08fcb6cd8adc45bbaa6fe37ac2e4e7b3dcfc09b71d9678159", "610862cffd8c0da9ac8bfa7799c5fcb93b8bdc15ed7f453577e203a778239bd1"),
    ("settle a winning and a losing receipt", "2ed18bd9c86c1d67d4a6545c062724a2db9b086db700104de329e5103ec14d7c", "e5d81d2a5842334af3bc33370f2cf305fc84358b1200dbbf815886fcc69ab092"),
    ("close the pool with a burn", "411e46a81d4ad5cbb0cdd4d3682fc91f8c385a0279357d9d20734f41f5fd2d51", "2b66e16b1740a338407b2f2b9d58cd6a9fcfce7ae5d0b267686be46de32eb233"),
    ("propose on B", "4f76b91c102abd89b014af1338588058038c0ac28dd5d6d5f8c0ab74d85aea47", "6130bc7cbaea9469d9a6dd56e577c991062c6bf70306db5ff734701428c6553b"),
    ("cure", "798facb048b4bb57acd8a0f01b30a7cc9cf99f2950af5e91ee95d9393397c49b", "fc8739b1e9a41a8145c6e65bf813f6c882f9489929a183605599bb160a66b07b"),
    ("renew B", "700ffdcfa616e3ab17cd8648231d6fc7c82ad8b9ddb4c8eedcb77d780d8127bc", "b1271fcbdb662fe8fd36e92cc8d9b8a5451f105e4121ba6e2166b6fb5d195f16"),
    ("amend: list the long", "82e8637db24d3078cdcfa2e3bae4ce058957e9e97d238dcd5890781b39bc2db7", "fbe645aef195f2ad6fb8d194be117aa93be890923ba6f36b15cfbe339e8555aa"),
    ("transfer the long to a signing taker", "0e530cead1a1070094f8b895c9673d9bf924e747c98d4ce0e9afd991c3f46f26", "c3b2c823eecfcee163a781ef8ec905184c92b7e8ca53737fbcdcda5c3a7bfa7c"),
    ("amend: the taker lists", "0017509356be08eee9117566fd57950013ae9be05f385f8d8f2bdb690bb061e2", "95f7788ccd473cd92ea6b165883cb16f4008e24e006f7db87d91e9ecf9847e7d"),
    ("amend: the short lists", "85008c93a8d93dd4e583d95d3cf23d051e68cb4df2486b0c5534dd6d007f7be8", "0e53a6c505b050f10976ad7b67f9ca3979599a49b2dc5f439b7090d23bde82c9"),
    ("mutual close", "995100ed8db3c4fe30164b95eab8aee0e0d20eb29d009e175e13e2a709faef6c", "642883c229001c3da49cbd037875769444fe7a1b5dc1e407d926bb3e963ef728"),
    ("lapse settle", "776e18e297813412c7e8b160e7c7b662ac7bdd48212401ad4f4445debab917e6", "6c6f7bbb79d5db186c817f26c73644e24d20c6bded5c0499b36fd230074c1e43"),
    ("create long order 3", "bcf933fafc83593e28dd619f9bd5e5f0f9cfc6b295093eb8adaa96b0820de0e6", "7186f3ff7718170793ad2167a759fda3322f003257b39d54288aa1d47837943c"),
    ("create short order 3", "8098a31321b299f4323e47447f919ec4ec08ab283330cc688a2ab68aeb2fa113", "5f569540de26a80604e3d0d1261dc4e6e2bd7669ea9d5e0e53b9796958fa5c9d"),
    ("open pair C", "2bdc3fe6b301d9cc3b7a8bcf16b673b05b1cf6f0dbc87ba7498a047207e3c0a4", "3c92a05373cd1432027f01348cb9f4b8e284be718a7d517ab4bac8eb70497bda"),
    ("cooperative close", "3bc66d1511b1e09100f5d1c820518f436b7801a2cc697cf81b3928fb7c241b25", "d2d08ac986e848148d8e6c7b12a7682eb97d224229bf488fe39e0fcb4752ab64"),
    ("advance the slot", "a11acab13883b45f74a8943b0bb23d37bfce4dd51983d325f72814dc33dd152e", "e20e79410f15a6d06816525c0dad16e68493fbaf2656aecfe31639ff3f1311dc"),
    ("split the slot", "ff6e2b44295467dd39e0556a8ce1d64625acf97ed4c09007bc7c67c2ce930176", "c550817d97c20ff0eb2c2c06887ff3aa9a352aee683476e0378d97e345cb59ad"),
    ("unbond child A", "939192f8ba4bc1ba1df48f3798439c96ed8a5f3c3eb040cb9cdd4605485973ff", "d8e591a317fb31bab79f1c92225b5cd2cce373d9cc2c9c4a723549a97611ceaa"),
    ("withdraw child A", "518aa07ce114f3a6a3428932ac37873115c3861112bb9787d11a613d43dd5200", "d0236dc3a3431e00019065e69c2acb6a6d02f3b249fbdc45241fad945bde88df"),
    ("unbond a stake", "35e3a0b5e468ae166ac5b9f4f55a2a7bbd511f705bc19ba20b78ebf88601fce6", "15bc29ee0a2fc4d71a341505d06eb194ac0170c206568fa8c1be5001224409e4"),
    ("withdraw a stake", "1211f21a9149ab3e9f4908adb7ed88b43763cce53ec317063c1376fcfccf9c20", "8cfb63d512d7b19ace6ec992483ac9dafd08ef998ac655b7ac04b29bad9e1173"),
    ("create long order 4", "ed8689ce0f086d69e583d0a735de36a7e88c09cd2442d3c783b2528fa42853e1", "df6de18372dc2e1a8080505c0652db874eb9e8ecdb081d6f3fe4fb96b61a68df"),
    ("create short order 4", "54e7e4ee74e6e219bf6647eef7099b0e53ae0933a51cbfd094186b4643014b8e", "70b8869b3d73af6c31618003ea52267778d6d6c46bb9c625af910355f970c4c6"),
    ("create a second slot", "99cde7f0151cec52b0056b168c7f96f4d1c481f22045c03e1baa50aba766ad05", "d8074749166facddae6856fc8dac1fd5ae5ce5e61a169307501d861b75ed8ca5"),
    ("open pair D", "3386c758e3b00e0c1cbe30944d69f32c940a29b8e0d539fda7eb15e556f910d1", "3eb0757fe2de8e24fbc13ae4ec1640ca53091cb5110c6538dff877c07364ced0"),
    ("propose a close", "2031b7776aefe828b9cc427bd1dce9154c45ee6c9a563f6bb3c80a8bc4cf558b", "cbe023da6796a8abd192976b0cadcc25f7514eed03e7ca227f207d39357c6466"),
    ("finalize the close", "d8f29d0b91fcea0c0a3f443dc6e2c12c3cd6da334136bc2be70a268848ea6d74", "a0849988d46f9c041a3408cdbacf82228be4c556f8dfadcc53f48eafb3916867"),
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
        assert!(e.contains("below the plain-output minimum"), "{e}");
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
    let (cpart, dispute) = (nth(&sg, 0, cw, 8_100_000), nth(&sg, 2, d, 8_100_000));
    let receipt = nth(&sg, 1, Js::receipt(stk, m.judge_market(), kpf_protocol::judge::NO, opener.value as i64, 0, 8_000_000), 8_100_000);
    let f = c.mint(ek, 10, 0);
    let (b, dw, pool) = decide(&dispute, &cpart, &[f], &ek, r).unwrap();
    assert_eq!(dw.status, REFUTED);
    let sg = c.sign("resolve and decide (refuted)", &b);
    let (decided, pool) = (nth(&sg, 1, dw, 8_532_000), nth(&sg, 0, pool, 8_532_000));
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
    let (cpart, dispute) = (nth(&sg, 0, cw, 9_050_000), nth(&sg, 2, d, 9_050_000));
    let r_no = nth(&sg, 1, Js::receipt(stk, m.judge_market(), kpf_protocol::judge::NO, 100 * KAS as i64, 0, 9_000_000), 9_050_000);
    let f = c.mint(ek, 10, 0);
    let dt = trace_dispute(&dispute, &opening, &birth).expect("the dispute traces to its opening and its claim to a propose");
    assert_eq!(dt, ct);
    let (b, d2) = deposit(&dispute, YES, &[defence.clone()], &[f], &ek, r, &Trust::Traced(&dt)).unwrap();
    let sg = c.sign("deposit the defence", &b);
    let dispute = nth(&sg, 0, d2, 9_060_000);
    let r_yes = nth(&sg, 1, Js::receipt(uk, m.judge_market(), YES, 130 * KAS as i64, 0, 9_000_000), 9_060_000);
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
    let vouch = c.mint(uk, 700, 0);
    let f = c.mint(lk, 700, 0);
    let (b, w) = propose(&pair_d, &ClaimTerms { kind: KIND_CLOSE, side: LONG, n: terms.n, price: 1_050_000_000, t0: 7_999_000 }, &vouch, &[f], &lk, r).unwrap();
    let sg = c.sign("propose a close", &b);
    assert_vouch_returned(&sg, &vouch, &lk, true, w.bond);
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
