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
use kpf_protocol::tx::{pubkey_of, rpc_json, KeySet, KeyUtxo, MIN_FEE_RATE};
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
    ("create stake", "03660e0650773c70a99a1d2d8daa2fab686770308b8e258b6b16b6a78f8fb9d5", "33679faf9367c079899fab174777f36ec93324c7925f531223fe653398e5fc65"),
    ("create the underwriter's stake", "9efee37daef0df3ec923e6714f24a61fbd12b2ae84e101631430fd686e191051", "e493dd963bae803bccfb2522a6eb59dbf20c8856ffc8ccc1ec059a288b8747f0"),
    ("create long order", "986bde6dfa059595b355a21c9c855f61531ef2f88279576c3b298e4ba746d714", "42fc1408ee7489e914662d6eb971a2c3d3a691b2865a38fca1f2985e47b2a35c"),
    ("create short order", "3c9c7a4e666bbfabe9e87a26f1424b70bf043e8dbe66cce6bf3dd800d805d2fe", "2592210da011fbaac1eab7cf5e5ff22f1acc70bda3e5f2e13d256188ff3a4543"),
    ("open pair A", "6e7f337a3886637701d3675f53f148e643310168c9a76812d9104755c5718db6", "08bc612c2334e4586f0f65d82b0307add3f5ce0476a0a9df7e33172ffa752ba4"),
    ("open pair B", "03efde917817720bd66c4b3c100ace36ec89dd405f566739e2464c0b32ef2ccb", "837beed033da8ad8f8eb143f0c4e7a4ddd24020bc3ef17276c7a1de57073931b"),
    ("propose a liquidation", "0748681ea8a167970c5f207478ac070a83248b411303c751397a3a2262fd9204", "37fee9b14f334040636df0267f70435e498a76bf357b57d52c9a7c8fcef99260"),
    ("contest", "ec6dee3f15333c7f7bf55b336808390543534ee0cbb940bdb3665c42de72e657", "2a3c25d82877b2901e6cbf6171e7ea30b0b1f8efbed03519166503b1612cbeaa"),
    ("resolve and decide (refuted)", "3bd39e8c1dac907017c4a30a9fee41ca16e89c6382e5d13c31ea7f3d3c724ff5", "5d61481408b0b8f791f2d370599502477a33c95caad83cb90c5f27600b525304"),
    ("finalize refuted (restore)", "fa4a295172d4f181e7bde83655381f442f82fcef79a1f6d2eca0472cfd60265c", "84c7cc37645580b03a82c106d481ff7f0fc6c0805927d72c021490d4efe33187"),
    ("settle the opener's receipt", "b2331d3b95093e702e897e7686ee9ec15a7a673f7dd96f5fd7742a7e899b51ec", "ad7c7adbe032eb4c1df2f7bd6d26c348d1f82a2b56d38f4208579a231f5607d0"),
    ("close the pool", "ba0c83a7aed617b976dc9d158a2b58b1deed66b8016073e4eaf58778e5b6205f", "c4d497222bfb7b36c73701ea42ace646044842411b914787a3e44e98038fd163"),
    ("propose again", "70885e641ffcb367f8f912e2ea87a6e85a501b1aa8ca23a70b01c02c74bbc435", "2e0be20d9577be45d8dba89e5cb11f3f34c16800c53ff0e63a77179b9a502734"),
    ("create a second stake", "4f70794569c748301bef78e2806255354c4f6b6745257445c46bdfab89e0d975", "aa2d43d936c89687e31388fed49d01470925c7557892feb47a926bf7a8a28907"),
    ("contest a true claim", "488828a9670fc5ee2c0c01acba5d603592cd2d08a3b9503c569fb839b4937fc1", "adca4afe4d788fb6ecf34d6d440d2ae058237ff6c01719969eb25188f53960c4"),
    ("deposit the defence", "72ccd5ed5c924077a5c441fb14a2ee7e88b1aa42ea80113bcf4b9d119e74df78", "ab08d30af9f79a1a63f4e9854afc2b34cd7e2b0757f40f3912f511a43fe8a72a"),
    ("resolve and decide (upheld)", "24246043aa6d7e99fe82638b7423c60ee21518e7d24f50bb757c925146421897", "e4f07005a8c845199634e29b8a6ae29fd46954abf2a834a4018bf614f2d3170d"),
    ("finalize upheld (forfeit)", "4cb39b25db5e6fedad2b733c09737c03b53d212308273404884848cd9bf6bad7", "6dc0bc537167f05a5294aff323ab86809c5ddb4d0ac4363f86a1e3bdd6d37e34"),
    ("settle a winning and a losing receipt", "303979f395ec8a2442360d25b77ea1bd85788eae06e62f3325fc6f798be7b722", "968c5cd3f0bf9b46aaaf819d5055263ba3e9e9d8d2889abd2f29a31519357135"),
    ("close the pool with a burn", "b3a47f80ae9475d5482bb88abe8288083634d90e49ad865f4fd00a8c0a3ccbc3", "289dc954225179bc836d669e04841f7741cf6bce5cfb8b8ba24c65cb407fa777"),
    ("propose on B", "469f986c71241bde6b663f1fee727cb7d305463a8f95823bdacd1027929f0ac9", "c9c163376a85c48ea976b01b7e667277c151e200793a98d730aa52c0bfaf667a"),
    ("cure", "8a58f19bbbabc5ca4651d4739c7dbca0da36723cdba4091a6f51cf0a34d686c4", "2f75c569fdd302e12bd741b974bd5b4913061df8e858c6b7d92c4c38c0531d5c"),
    ("renew B", "3b75ed93392fd24c7316b69a1bff58561a38ae8aed46e03bdb9e713d1d2204b1", "5167a8751e8dbf86a5bf35a2f1b8cc6210f9ba7d7b2ce374ae09b679ffda77a3"),
    ("amend: list the long", "410583ba96dbad68ce25863e35056aed2e561d9c5841f7a32e62ccc50399e8d6", "09846aafbea82c1d36a197fff10e1993be75187241d1030760ceb7c110c6a8df"),
    ("transfer the long to a signing taker", "425aa7a5001eb3600c8afe5f85558826207897f771386bdd54931fcca458022d", "d106c5e47768304c62466c9f110eb147a3da7cdd0385d8382ec1d9504b53f77f"),
    ("amend: the taker lists", "a3031acf87aced2208feb0a790516c75ee857eacc87941204392cecf43dd03ec", "beddab24310bf01becf9b163113dd5fd792b15ea88fc1b4cf4b2a08fb6c6d365"),
    ("amend: the short lists", "cbde5b74669801ad32a54c2d5fbb4d8c58aeca9b93f26ef9904e0538f5f535ce", "9a5cf0825dc2810a5b06407b7a090f6dcee2f70d4fb8c77668643f22f6371cdb"),
    ("mutual close", "8b866572fbbc8056d3b2de47fab01f10f84e103f89955937b2f03cd9d9a2a56c", "b0553175a879000c3988895b35b3da82a72d9db4e3cfadc06aaeacce017700da"),
    ("lapse settle", "260f229261c4548bef3704a2b8fc9dec2c9211745a7f9e87fca7dece142ac7d3", "762f57af549b855669762545cbffecba0e1e730092a05db0cf5e1fb2940a2713"),
    ("create long order 3", "32034432860d7eb51252caf5999724f897a85933bb8f71f5505feba5afefe9b6", "e4bba9f91e82db0d003578162747e43ecaf409e9c1c7065b03c610cb3c2a2a04"),
    ("create short order 3", "8db3fc017679be3c5a646b4f638273401a585bda773e47073a073c3d39e2bac0", "6b500a9ad8af3f031e6f876ce7bc9bbc09d39fdd1e7fbfd52388c87a160310e3"),
    ("open pair C", "0b70f4414983f554ff6fef709c1b1a9b64cbe439400b380e7a0b18be083f84df", "f4ef7187666bbcc845ee3e36d356f7096123c0c9b7f42e0999d68595c4ec9986"),
    ("cooperative close", "0035ae061c160d8213a1f724ee8f177500e7f9d6ef9bbf00660557fb3067cb17", "7153e64669124bf5ad84eeea2655c01b004ed46078533748ede2f0e20e5e3469"),
    ("advance the slot", "3148be4d0ca2bae41484405815c628ad5bb543b02ab2898abe6f6a712f24db80", "21449ac7cf68b9e8819354f6267d877a5721fa059159c1546fa5a47fe7ce95ea"),
    ("split the slot", "68d34619d4b11075d35307510b935693fff6c389f945365dccaf3524de003838", "6fff0ac69e92ce312aee75c12f01ca6678ea164d686063e88a9542a3cf51178f"),
    ("unbond child A", "0d27ce15fd48e206ca0eda4284fc0c5983ac0ae6093903ef202b8b1d13b0c932", "01ace824896a6a16a0206531192250e4ebba480004d6665815235e63d6381b9c"),
    ("withdraw child A", "d132b80a41a3e01c31fa6cb2c43acae38c45020ed5cbc50c8a476a982323b69a", "3958a03f630a0aa4756ab6394304947fb1a36f7f730632f2b4bba4892c71eddd"),
    ("unbond a stake", "0bf78e62b077e5203b676fe286dc6c4e929df264731b60e5aa2803abb05f9df8", "3ec012526cb3fce2fd983764b86064b5f108bede7502ccf974293e6c4c409b9c"),
    ("withdraw a stake", "d461c029c7e92afc1b9d6238ce9bd821de1a2e8f9a688387ae81ec6e1c985e16", "c96d17d3bf5765b1c115ce5903f00699310dfa5ffd26cebb0028279871ada531"),
    ("create long order 4", "1192fb952c4b5ca2ebd5142b30637b746972bd29460d7081f43eabf9cada9f99", "e3a0502e4c7635f077b39f10f8bae286585903b0f66aae35a724c00076485ead"),
    ("create short order 4", "392d3971233e0efa2fd52bd532ad9134147646d6a571c3103974379f64db9665", "eaa6b874f428a39465b964f702df583f75cb0d0e93d26f29e662ba42183031d9"),
    ("create a second slot", "99cde7f0151cec52b0056b168c7f96f4d1c481f22045c03e1baa50aba766ad05", "d8074749166facddae6856fc8dac1fd5ae5ce5e61a169307501d861b75ed8ca5"),
    ("open pair D", "c21565233486fb2ef20df8d8f8ff0cdb9a2f31eb2df100131c851a8ad3c977e8", "d987a3becf6fe848d103227b676cb2e75c5f7be0d59483998b411f8a2bebf820"),
    ("propose a close", "e57f96d7496389d911a43f70fd085368aa9b53beb810b55c293d96b5a5b7bbc0", "5a4b042dfef3604c8404437b29b8ecb69570dd5543a36ea7f466f2f9b7e0023e"),
    ("finalize the close", "303faa95d071a3650a8029fe2c87796ddf7a0941d45a4f8a6ec740e7491ec25f", "2314fb4184d3d277f4badbd1eba2cec8cb5d92b4ea220471cd35e186455f22ad"),
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
    let pend = nth(&sg, 0, w, 8_000_000);
    let f = c.mint(ek, 10, 0);
    let (b, cw, d) = contest(&pend, &opener, 50_000_000, &[f], &ek, r).unwrap();
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
    let pend = nth(&sg, 0, w, 9_000_000);
    let opener2 = won; // the winner's stake came back fresh: too young to open a dispute on this claim
    let f = c.mint(stk, 1_010, 0);
    let sg2 = c.sign("create a second stake", &create_stake(&m, stk, 100 * KAS, &[f], &stk, r).unwrap());
    let opener3 = nth(&sg2, 1, Js::free(stk, m.judge_market()), 0);
    assert!(contest(&pend, &opener2, 50_000_000, &[], &ek, r).is_err(), "stake younger than M_age before the claim");
    let f = c.mint(ek, 10, 0);
    let (b, cw, d) = contest(&pend, &opener3, 50_000_000, &[f], &ek, r).unwrap();
    let sg = c.sign("contest a true claim", &b);
    let (cpart, dispute) = (nth(&sg, 0, cw, 9_050_000), nth(&sg, 2, d, 9_050_000));
    let r_no = nth(&sg, 1, Js::receipt(stk, m.judge_market(), kpf_protocol::judge::NO, 100 * KAS as i64, 0, 9_000_000), 9_050_000);
    let f = c.mint(ek, 10, 0);
    let (b, d2) = deposit(&dispute, YES, &[defence.clone()], &[f], &ek, r).unwrap();
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
