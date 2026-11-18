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
    ("create stake", "bf7c25047547ddee3b54ac58a64291ab4a56daef41b9fef029a08fcb50152378", "9ca6cf4cb7a18ba7670736602726df098fd9a22a7cd9f842dbf2a047ab1563ff"),
    ("create the underwriter's stake", "dd55f9fb35889de1cd344ec6d5cb389abda2c4de40d0800ebb7be391facb8729", "64b01e0f4ead67fbbc0b6cecc5c4086c24b0d10566822774792644e7d4391822"),
    ("create long order", "2bb9e4e3e24586b0df849c50044e99dda17b756e5c86b2b5f1345cb5a2ef4fd8", "3f8d3d5f4e059a7a612588b0b07169519bde819d77a37eaeec03d1a9562933a7"),
    ("create short order", "fd450caac5fbc13d14ae540f46a3b3e84666f13fb0ff4c29b54593e28694b2d7", "86ea14fa94dbea1c06a3fec24dabfdecb0856127c3b7caeff5f50fc2a049f819"),
    ("open pair A", "e35961fe4e1be89fb37e14bd657c82dba80cfea887f6fa2d2c15804ed1277749", "4d47d7c5048e3b785be48d0eed257d1ee61699160b947c22a28dab491113cd1d"),
    ("open pair B", "53e940a9cfae0c5b33d3a0dafb628b801af088196e371be421432bbceaee68d8", "6478829bd312288263e5b84fccca3c8fd441055532246323066d9111e06c6970"),
    ("propose a liquidation", "17e25cd40badf5d57c0d96ac4c4852cfd9b34d2bbe9c7161347bdba0e52a362f", "bf6d387eb6195ba65e07a9cd93ae581f9c61a8087fc603798490d9ec44967f56"),
    ("contest", "74d2d4699388e0d47d81fe66dcfeafb7790d77e1eb7ce9bf4b95ae184ebb185d", "03197c9f60162ac1bf9bf1d93ed508bf8871fb6f94aa68dd1c9655b9e3e42fa1"),
    ("resolve and decide (refuted)", "ed5edee010bb584eaf6f1b80fb312da12ff9b165b4cd38b3a8a9759d1b4fe30b", "1f963eb077e8fb43a3e767b4b6f4ddec3fd7bc2ff68594a61071439be64ff766"),
    ("finalize refuted (restore)", "e55cb4155a691b7e20b861cea992c9281d3fd8274595873abaedd737873de530", "8e92f99d3706d9c810b1690bc61a4f7f191f12f8845579c058b4d628b4ab2b71"),
    ("settle the opener's receipt", "91601529591461ec66c6afeae8b00011644a3d4aa75220986ebdb495283dd892", "c179dcef76a79f1f49ecc835f2b8c32fc1070354456f48ae062b4540f51ec423"),
    ("close the pool", "00b2e70204807795a21c69e56e16b24e6184f91725ad19dac1daa22ce9dce635", "a134374f8f7cf373a0662290cfe83b291ea4810d784c3d8659f74ffe7eeddad8"),
    ("propose again", "97df55493548142d839dd055089ea52dde59f1bea1809aebfb2e05f1cad6f914", "21febe42cf0b3d7f1b8b5cbb20932c8ba285b23bb5a3713a5c6512d1bb289a22"),
    ("create a second stake", "92e8da7a2ba86eb5d600daab1603ec54ed8204c09c17bca60a5d789a77551726", "a30f3333323920c2500a6865005ed3d198faa2fb114f2456f0d4b862b13bab43"),
    ("contest a true claim", "52e5c321506f07b16f3d11d2dda61fcdc01a5fb1e2fc7cab6d62dea69ffe42dd", "0794a1d24f17867e0738a5dc0f4b5fd5cac1626787e20538c853ee9965728f85"),
    ("deposit the defence", "d8168c3a3c1b5d2ac5015d36e0803e5351523837fe7e140c0eef967ce354d356", "1cb6dc213643ce48859ea57603e3a9796a380f10eab0f72f647dc4d758dc690f"),
    ("resolve and decide (upheld)", "932f72a8f7d65d7b97f9f5b7c5db02bebb4e1ac61ac1eae5653a6d1406c1dfbd", "84645105f78271a82b4bcb31362597a1041be411eb4f0646abb60f24477aab38"),
    ("finalize upheld (forfeit)", "4ef58b5a0e56cddcdf399de73f9384aeb56c6d23f09649e14d00c63846a8aeae", "efe922e1573f098730442de099dfed81e49bf64802e32988a525ad53da833d1e"),
    ("settle a winning and a losing receipt", "1fdbc6a2efb7f03406e67b338a3338957ede148be71c2e7a1a5722d448727f0b", "1e2b9989a0c68d8f98f0587640b7fe3b2f79a2f411e9b48cfe62415b22bb9141"),
    ("close the pool with a burn", "50492565b6fd2dacabc5a607d48f5149b60acbe072f882f7b0c0751070b9fa16", "4e04e989e1a1f70fd6d29737001ffcf7223cf131f507052e1f0b08dcaffd42e3"),
    ("propose on B", "13f426453935a3530fdaef944e3b50382ba52480c270f74008c471e63a4ec12b", "5b35704658cb680b2a78a27cae1214ece3c8e466e8f23a62f43b1cf2a672597c"),
    ("cure", "e8b285f60fffc76858a3e0b1b41c05016f1459a1dbdb614c15192c058c5cac47", "aa99a8d61cc6cb1327fd0056c0a9ee8387a42039363379bac968c1b442bf50f7"),
    ("renew B", "a612a2608e22970b7324d3e319aa77a605d7e876510268ac36270438c5aba65c", "5b7cc2f2bce83c78e9ab7c817f7917a7693517695e7ca9cb4c78833bacb1b0c4"),
    ("amend: list the long", "898cc8943c7ee036ffe76ae7646aa8e0ed1c31e0b6cd63268e3baf1791dd936e", "4792f727c1eede64a4296ca282b9776e95652737cce369418b3f86e391f2b25a"),
    ("transfer the long to a signing taker", "f67ce2935cf6ced00a494a53bf8fe729e8e0b865a8361bea11bebc2bf28fb1cd", "8556a662fdcc739e77e6ecb3714a93000a99ec1791e99ace2f272a9b4ba0c087"),
    ("amend: the taker lists", "c6629afc65157e55548b85bad72855df1f778ba76dbbaa51d8ed5dd9af8b57b2", "dc29d4688f54893db8a6ce040d08e3e16a3e64f0d1f00ef90aef880010f291b6"),
    ("amend: the short lists", "6c1cfde367f027c17960f448fe1abff92e43d3957035cae35806b2155becb8b5", "47f263aea3accb4583cbfb0f2bb2a075dcf5d52f7a106239fa0b47a142742808"),
    ("mutual close", "79f5806e3cfde2b269191968d92110f49df2015856d721218e92184e84cd9eca", "2ae0c272a428b01d8444141405dba4d3062907e126d1d7a1c525eaa842d20483"),
    ("lapse settle", "3754f1553cfef91916a1d5b5c209723003a359eb092c4b5c67bdfcd40d353904", "a8a73f25f04943b29d0a9c22af91d576f9b8ec41766e0407f1bee8532046b4c0"),
    ("create long order 3", "72873b80bc7230311f8702096568e573ffa72cff80ca7bbb1406136dc5790c04", "efbace103faef5ec2b1e4e2293e41441d16dd43e3d765ba36e3ce010b5a08f00"),
    ("create short order 3", "469ebc04d1b68c68cdd266b8ea0f5641bafc5142fe559119d1d25c1e45b0c9b9", "abaf0719da70cbce2befeb22d7253c6cccf5cb53855872b82606bd2e473f6d3e"),
    ("open pair C", "43ffeca769093f0cf31c9f6df810f5568f84b2e2e39c42589feeeaeefe3f0dc5", "5212b6ec254307db784a58f34fbe3097a76890a4b903347c07ad0f3e8f3889bf"),
    ("cooperative close", "7f6116e60213b9cd9c719af51c1ecc12f3bd859212e3b002bd71fdbbbc384424", "4a0cc8d26a90a0a3efaf576e7fb2a01125ae486067f29cec21580becfc18b623"),
    ("advance the slot", "9d463e1108de06686f997645c824eca4ef9634ffb8c3740eb3e4daaedf5b99d4", "0d8a7be5572f9472647b688629a75b82f2feaaf91c82bfbd91fd056d00f35667"),
    ("split the slot", "0a75bbcd98f7c675ed55e139710b2164fc0f4b283db76a27a3b1ddaf96bd8942", "ec6035eb2f988d00e97716c44025fdaf1877a10b7699d9107a932505abb1c6d5"),
    ("unbond child A", "3c16c0e49cae9f45968de5d6aed6311069172099c284f0b248ad704c7e5127dd", "d37a015f0accf0d254adc786fdaf25fef45d864e3d3b29059baec8907afe984a"),
    ("withdraw child A", "4610285e53d77e9ff6fad141085bb25995331982d26ef338bb01467d25203335", "f316b04f8e11c251376143149619b812d3b3f11df4fd6c9566c4dfa04c659240"),
    ("unbond a stake", "1cd3d6f495479f8903bfce9ad7cb88d77308f52e92d98a28cea6c2aab4a55550", "6af879fa070bc0b9e51c631a0e1b4d071c2491ba8119ae9d62c1a866e7e482fb"),
    ("withdraw a stake", "e466b4c24d9e6ceec867ef9c141e67d8523ba1c3179cd43dd3245141b8039a52", "c6c01d3999d08ac80c103cb5900e9314e8122f6abbe1fb73d0bac7e3b5debec0"),
    ("create long order 4", "c57907fe0968fe7dddf4b3252677a4ea5f57ee81fc800dc8f5d73011b4fc9468", "0ee7fd22400a117f902426dd983b73617b57fa670510d45be0a941bef596a925"),
    ("create short order 4", "600ffcc1f9b565e2cdc179a36d201f4088852bb6dd688996ba68f2fad842da26", "6ea61c7875518a5366f888df006a91a8731db9d48e1e3a5f84788cd9e72e2e3e"),
    ("create a second slot", "99cde7f0151cec52b0056b168c7f96f4d1c481f22045c03e1baa50aba766ad05", "d8074749166facddae6856fc8dac1fd5ae5ce5e61a169307501d861b75ed8ca5"),
    ("open pair D", "b545306068f17a7635969d21b6f17d4c0983926f137585336f112386eb40589f", "b43cd82e9a1b41f57d6d5d7ce61cf55866dcc35db0542c4fa86a210d3fd9dd39"),
    ("propose a close", "43470cd3008fe5d1174ba030c650a38871a0e19fde970beef94a9f319d414c5c", "a977990b830f1c857d26d33d839c37333ec18ba034d8810e34d90942f07a534b"),
    ("finalize the close", "b5f8331f2e7cc1d8324e376690fc712bf3591a633e424c87cce22018181f0bb6", "aaed3e0c3611422fc1846442fbb93f6bbd57b55099344c1275fbcd6d3a144cf8"),
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
