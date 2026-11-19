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
    ("create long order", "b39849d23c28da1b1608ca70776ed48ff2e97a5c37ec5b263188304204d3c50a", "d56c8a8fadec510cd022d9d7aa8ed6fe9dd2b0077afa14b66ced4cb47665192f"),
    ("create short order", "512353c1268f5f157f76d9ab3e10f15ccb20eda93fe887aa74605ba175733f61", "eeb38f0b9637227bd92fc65bfa3d7eae2579556ca0a2dc3e9fc60cd5b07555d3"),
    ("open pair A", "d119e79d1ab137caae1c5b548b3d67be8fb24670cc6cb3ca0b2c39c1e7edde56", "3c90af5a4f948f5d5032b1d1f2f245f29215d03f26ca655c4c1c03b9c382ae14"),
    ("open pair B", "5f9b01f527647c3bc4f11d050fd84265c5b023ab4d1b883c24022e79c064ec9f", "e06c76d6ab9da7ab5dea0e56177ea23d920585daecf0fd9e9e26bcab59aea3a3"),
    ("propose a liquidation", "fbf3c02d390c53db75aa753145f325197b5102c2ffc447aae1da80949f520078", "5a1df12fc34ea23ac637e7f7180521c7608661fb2fffc64d129fb596e13b9258"),
    ("contest", "74c5b420bf79095af624599dcc164186b50be86dc6506eee747a4f2bd751926d", "6f48bd12f56b66572671cef14125041d982a67d34d81324cecde1bbef7bb8040"),
    ("resolve and decide (refuted)", "d2f3eb638755cb37500ddc163178f4427e9b6afcc9977fe0d9c64d3b722cd54e", "8bfc30d47a3c68c88903498f39ee30756d7ade61af14e4b1239c8ff6b8019c7e"),
    ("finalize refuted (restore)", "536e542eb1d3763e64f05bfbb862fdceec293918584f9c9c91d2c62636647543", "14df7f2a1cf73ecd127a003d981f771f3d8cfc76b4806198c3e4278b121169a2"),
    ("settle the opener's receipt", "756a377ae5a3ff73accaf7d56126150400a575b17e2f21f58afc5122052dc292", "f0f11dac336e55ed8f1bcb84c2f7d766584f3cbaabb4c10917923c9cfd370529"),
    ("close the pool", "0b6f30bdf6c3032ca9a5e3609682346f8e7eba16a776db7f2dcad35be59d884c", "89d3514443118250a6bce74c58fbe04d599aff22be17ae3b4f5a1fcf36724a93"),
    ("propose again", "33c75670ccba2fc31a12b694328675404622fbe926ae27dcbcdd31fc0f97e247", "76924c95dfbbc1c103a1b4c4fcc85c9a0c924faa568a2044c16a862e80909cda"),
    ("create a second stake", "6d5b6494e409a821c1c737cd4dae2646187878512d620496acf6115a62f69d11", "36c7ee53c89d632e0bdf50dc144f012b700b83148abcd0c83bfece53345ffd2b"),
    ("contest a true claim", "590e569b69bc23dcdff15fbc3dc2e534425ed81504b43afdb6873becb7c66407", "36977532b9919c7bcafd0e25230677dd1e07cab0ca29e3fee7e018fbd7f35592"),
    ("deposit the defence", "16751cd01af59155aa39c5051236989646f9cb51dc22fc62756c044474861e8a", "9111944e001987c32c3bb064117b9148e9a186d81e605bb98e11ad5eb0cf4b6f"),
    ("resolve and decide (upheld)", "b105137e2f91ec42816d5dba6463c6cce6a1ef1408e4b0c96d2033edaced6825", "6660612ef155806f4d5197b87854cb4959d453269586c7a36387465e648402c3"),
    ("finalize upheld (forfeit)", "ba553a579859a386b2cbd5172e3718ab6f7bb38aacea6358fcc1a343bf12bb8e", "b655b3f43d49b587e0139dc228328109665093998868f16dba3bd9731c22a3c7"),
    ("settle a winning and a losing receipt", "c8eaa82ef39b739f21815fa374dc64463f2232a80092c2395249ac63edb9750c", "595aa2a0cdbc97e229a49351a7784fce64186fded751f81a7b691a5bb583ced4"),
    ("close the pool with a burn", "0d2e8220b9090e7f141b98b77865658e204ef77aa0c2e4600b8f7b1ae08f969f", "3ba136fc80bfac893abe3131b5dbb0772f01663eeebc08656d1c8f3edb892073"),
    ("propose on B", "e017fe593bf1e341d26ecf57cf00eac93d9b6c372b165977a2827478481c1123", "9861cba0bc76dd48a2f9ec1541351c77557591bc71db84f7a248eeb0af9d88f1"),
    ("cure", "a6f8f47e89bc123c16f125978e9b22cfda8ff12d6d73250f4c5be724c7f4a185", "01e1096ccf40631c0459e2ab8c2b5bdf3f29c2fae53fae9e4506394a32fce3c5"),
    ("renew B", "e9003da7d7d828b4c5b30f558dded7477177b953d8710e7fc9c72b47e1eced31", "134f4639d826c1e127de7ade999c752324b0da2cca9961284aae1f24b53118c3"),
    ("amend: list the long", "5f4864fc437864b826c63403b745ea56b5800b891fa66141ff48dcaa035e8ed5", "5e85bcd0023ef68faee0f192c57a40eb3b4e22982c081b2edf294bca4dda8505"),
    ("transfer the long to a signing taker", "a4b365075d3920b5ccd69a30933fef000ea88bb60501338cc555657c13452e60", "caa5094dd852aac8bae55c8a408bfd281c130d53713140ef9c393e2bac9623cc"),
    ("amend: the taker lists", "d763419d5b90f8f4842d86c49972fd7bbfc27dcdc378f2f6d5e2eeacb5d3ba43", "ddead767dc2825c5c77375117ed3d31e264f564059db226af1727d1670ea2345"),
    ("amend: the short lists", "73f9f4f2f17eaadbcdd160f471070dae543058ab3322f6fcf4cb71bf61cd4100", "2357122e3031cf3dcd4d6ce0d568db285dcd585559a667625445fa70d685fd0d"),
    ("mutual close", "771a1fe5160beb12657ea14e0eb82c01b05132e1db61749e1cdcbd8721b1083c", "e2c1cd8124133fcaabc67e5a2507ac2d7a3741ee7200b7d2e7235ca46012489c"),
    ("lapse settle", "0e13435eddeee8b68516502a23f2a685371dc9fbe5ed81fade8f4aa3062409f2", "36af3581cdd87f148355477b30e84c31704165fa75781b01963e08a16da90ff5"),
    ("create long order 3", "5a5db370913c1e47213c98cb6f3b908e9a7b8d669c3d923fba0b36d89c2eb470", "95e827637d0d9a7541b6cc4639e1b38d2bf05e3a1b25e66dadafa811c2ebe549"),
    ("create short order 3", "146b861da0de86b7e40628b49aec0d07a1eeb58878d53c4bf447ae9d11aa779e", "c66a97c4b6cab930a250d3dbe02f9a03de91cbbd93c7845319a6e55d7d1d8dc1"),
    ("open pair C", "ec0817bd06adb90cd24436e6262cacf21afedcde60dfc725a4e7e5f2361500ef", "528a52843ba9953534c6a604eab33816da44aba423f81cb061d31ed6d762d766"),
    ("cooperative close", "43f0d4ea4808e7d66587b1aa0dcfbdb750270ab66c758b5470918e65508a8918", "20767d28ce64c10c902caa6f1f4d5aa8ed9033c9a15cbd80d8bbb226522ea393"),
    ("advance the slot", "7052d10113fa882c582a4ae10a22b6ecb34f70826e2f5a94e79aaa4829a57aa0", "eeb5448cbbaf42d98926d05ca2e11f5c959b0c9c61a1f9d1745ea88741e0648e"),
    ("split the slot", "21fb925eb05281859895494a0531e03d1ec6e3152fbcc960613cb5d4fd911a82", "3621d7279c9d150879226a36d3d9949c3b5ab80d219d1034c5b49946ccc014d5"),
    ("unbond child A", "fddf7de6d8fac8a986e8807186316f3eb165388a0fd7183ba07d779ca109a4f7", "60f8aac2b5a272752734f19179d86a5e790d509f2dcb733792525a2b3bdcc47c"),
    ("withdraw child A", "7e18a7749f0fedcace472eee2e789f235a918e73f2f0b5cad3bd92889b1b0db7", "b1f603eba9f2c5af1e6a2badb42d40d5597b0434cb69f9a97850ffae47e6996f"),
    ("unbond a stake", "98c3cc404a1d825fe0955d18ddd094daacd1d70be0203e307863aff878e3e9d3", "9c6bd98dd0fbccafc3f67054872a4e3f9349c3b55e68e71daf5480b5b8f444e2"),
    ("withdraw a stake", "927c92855187fa6fe7bb69c4499a77e3cf49bc598539d79706be4c15bcf389d2", "178e2f4e3bd90ad054692a79a3b274d0266cc52aa6de16dbac77074b00a8799c"),
    ("create long order 4", "16e3e7e26dd3cd011ac91ea8938669bf5c3a23c2d4a5cd1b1bb8f46e18fce877", "a2735d8fa032d02b57f4fb66bed8400d296de3514a099b38e92849287b5105bf"),
    ("create short order 4", "8208ef87df13a75a24b7163f395c9596fab1649de458fb483862bb1830a71e0e", "157460eb3633c469980143355493b28b9154040cb1e10ace652174e9da648f0b"),
    ("create a second slot", "99cde7f0151cec52b0056b168c7f96f4d1c481f22045c03e1baa50aba766ad05", "d8074749166facddae6856fc8dac1fd5ae5ce5e61a169307501d861b75ed8ca5"),
    ("open pair D", "f4fd6e40ef01d0082e4dd59a2802e764f5f9400d0497e74cafbda59175e42087", "5441cf63d805b2bb33a7814ab4bc42380ec4f1579bccbf6129027393d5965f68"),
    ("propose a close", "6f43fb962630c4a8c889a883c1e5c9ffca48c20255ec9ed4ab7def9672ef5753", "d0bd7de58237da079fa3573612e172c760b56c3eff8b3e447abbbad0d3899efb"),
    ("finalize the close", "54d3485da2f9495e55d23f9faec1c3ba6384670dc4fddcbc87d2b3cd5e8742c7", "51dd544918bbfb7a528b363d85c2b3cd440dbdf6b6d3eb0638a125207657b6c7"),
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
