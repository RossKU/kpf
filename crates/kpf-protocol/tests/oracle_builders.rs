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
    ("create long order", "6df98061ab1510f11733d2cfede3ef93f7e3416e115d870ae0ac5694600aff77", "75986fd40704f04820eb0281c8f3c5043b1ed8fd405770cf5c42163f4a832b71"),
    ("create short order", "abdacf8835c5ec493ee6daac5aa14fdd94f1ee35f7917f826fb0d0575018e4c3", "2733e0134429aec79e4c4014cc7c1f4d98bfc8864ec9d9097068b76256f94461"),
    ("open pair A", "4b9f364b6e04352611e289459c21afe79450a3a9b3ffd742eada4cb197c0ba33", "d4e7434f03248e62333e514bee9d822e879106a84df4c72501540fc31fea9b90"),
    ("open pair B", "eac2adf5c9a28b5b5258356f657433d8c3a293db5a57a3691bd6aa1227fb8ee3", "575a8192bbf7433e2c1d8ffe0ca08fa3cf87e798b24c85695b76b64777354bb3"),
    ("propose a liquidation", "6cb281d826b00b597cc1fba64e00c9a208007e36089020e5dd4c8f9942460cde", "42d80dcf1eeb4fb893c95dce8b0ed1536d791ec9daaa913d6b1cfa0344d1bdc4"),
    ("contest", "2f9710977fb9c4504063feec9e1a3203826de6e3447a410d5e8245b01f23a967", "7ff51f005b5ce5463725945d895a8f5d5bf135f506d736c21cc293a1bc285cfe"),
    ("resolve and decide (refuted)", "060d8c434ce331c161bc86762c616350c51c38de0d1f35b976c146c85ca13518", "7d6a872da295444cebff03e91e0f270e2677d68eca8d22be827a4106d74992b3"),
    ("finalize refuted (restore)", "ae7e56eea28b31ce83104a633615132e3efb4ed5932fe6fd7e0a9d5de8a3d19d", "bea91ec70026180d7a7a64beb1eb2d3ab795334ba30a64cbb70ea09249b8f3e4"),
    ("settle the opener's receipt", "e3763b036cc4956884307e46684954fa071e5447e78a9032a587f7d99fc26c93", "f5b410d65397a5772c9ec3080b620313e3cc1189b40e34a518915826133f67d2"),
    ("close the pool", "bb8be5caa3201499187055685d629b56f7bcb08d6318086bd3a98ee4ccd2d452", "49249d2276b0253145a310fa9528d5db4572126b63dc127c47438bf2b831cf4a"),
    ("propose again", "4c6e26b8137a280b6d9ab208659b58c1fc0b46aeef34319d00bd497eca5bcb64", "7aad2c672b3a6f3128d3e99eb4368a0910d481ec35be1043ad59bb15c3da3195"),
    ("create a second stake", "6d5b6494e409a821c1c737cd4dae2646187878512d620496acf6115a62f69d11", "36c7ee53c89d632e0bdf50dc144f012b700b83148abcd0c83bfece53345ffd2b"),
    ("contest a true claim", "a8c54a4ca566bf50649e64ba9ed4c732df03df3deb12d6f8dfd081500593397f", "cead9d3661b1ba10442b8b766ad3a67ff62969f42467d2ce8c142afba8a11571"),
    ("deposit the defence", "60c47cdf5dd1333face339166f7d68e0a0c7c8e592be7278b8b60afa479f4fa3", "c9b5c90af1beefa6f6b2078c19d870e0d46d11aaf69cae531cb111839231314d"),
    ("resolve and decide (upheld)", "b15aca610e7668dce986f28f83de67857d129631fe3cbba914cbe56beb8942e2", "e6ea9bca2964e7f941c10c37e807ea45c583b3fd0472403d3c1c435c93d87e5b"),
    ("finalize upheld (forfeit)", "b25dc3adc0a271e689f464a5d378cd40182c21c57924376f938077d047896571", "d919182f164a170394c8561832800667f28759b2344fb232e3ca8ee8f1eb14aa"),
    ("settle a winning and a losing receipt", "83c28ffe71f4f88bc1b97bc0025a13421804aaf1160d1a06de3f2bb8a0e8d8f3", "bfb7144db710e77816a0584bbd0b082f9d76e6dd60be3c35f9ef74006d00d33c"),
    ("close the pool with a burn", "3067df43ffa6d8966f88d9fd480f0cb26f315bba3327172f7f88536557c60a63", "cd7803e67f431fd7c93bba5a994934a04c9f0bfe8c5da3f9ad8945e66cbfe48e"),
    ("propose on B", "78a9a62a9a91a16e078bb1de21c55e5d1683c8b7a698a3f1377d3438caffcc84", "3e6392988d4cc3ad23b119753a2f3d0cda2b8832b513e160c260a8b2dab1bb8c"),
    ("cure", "09d232cda715b391e5c82df9555dacbeb9bf7d2d5ccc21f880c48b3b92206937", "955ad9c2d2cb685b27cfde5f0d884de103ba1f55965f8f05818b7d4b9ddb4bc9"),
    ("renew B", "735f3008fde8d23065c5e3fa92a3ddb7a2d29931b9f76091ed2727bdaa5418c2", "88a145d62df2ebc482aae7c0033efe87c75c6304855efc684557119970529b3d"),
    ("amend: list the long", "6d7c14e2860e1c9e5b3a047f1d6ab3ff71e00ea0246fea16e320fc851b1087c7", "665f13bcf8a5e87b1587dc1f2ed7c4f4405b54328800bc2030cbb3ab8bbb29f2"),
    ("transfer the long to a signing taker", "62524ff789d8181148cafbfadf85c56a5943fc8c7523a7c05a7032a3f8a7b478", "c4c0b63fcaf5f406d7e4c771ca0e6d8904f16119d49fb06c49c0d3f6969383e7"),
    ("amend: the taker lists", "82ec420a4fc23d448626b9502eafe57968f682e60162125cc540e7855e06a995", "293ab85fd8310eddc7011a76c2fa91cfc64f61af7e3827b6a420998729367a75"),
    ("amend: the short lists", "d6961a8324fe4ccc1d3d7d5ecc6534086dbdbf447073b9e05743460e0e85645b", "b9f4ba4e7b0b03313c2cd2a79be528a7a9c7e747674b6e52602e8e0d91725706"),
    ("mutual close", "fd934866a1b80b2777465a39378b70c560664c7c852b1087f1a776fa5c025e1e", "72ea0accff4a6c1ae14d0df9d589e0668f144bc4d748b38764a479902eba150e"),
    ("lapse settle", "095f84274856ba7d2c56ebcafe1cb307df8fa6a80307683beb0686fea4a9382b", "5bb8122bfdc7eb2154d68388876247d68d4303f04ff1b0776d77f74672ca2402"),
    ("create long order 3", "5fb54132ec0f46ed7c4938262e5e34f884bb2acde2a488f7fe443d2564103a40", "3168e7bc728295b336fde94649b234a3ced32118696b6e8e6c445969933124af"),
    ("create short order 3", "8924fc01a495c6d994d84707afcf697902e54ba29e7344d87163ae93aae1c2c3", "20ee54273cc71dd018ccadddebb55e09f6ade765dc16ad69206e5dc2bb69a901"),
    ("open pair C", "35f48c800451805f0bdfaa68534cc8b6cb9fb0d2625a31ff41fd4c5fd80491b3", "3f3999f09231ba7ae486ac708a1891de5f495ddabaed62c3033854dd833ef338"),
    ("cooperative close", "5fa0a86a402fdba9fda151be3c091913568ab3c98288804bee9f5eebadba1c23", "61a8f78618d3ca7e7ef861836ac9e3fb90628162e51a3946bc4e4a9e1ebad3e4"),
    ("advance the slot", "3753a2434a7ba4dbe49ef51e8fac66fc7970d8619c54680db5d3641aada4dae9", "413e36c3cfb97782fe6bbb8378e22001d96ceb3673ddf147c553524e693f2f66"),
    ("split the slot", "12f39a926856a4153f1fe9eb77b2873aa9c8ffeac32209994774c61abdc97402", "31e7164179adfc3c633464cbc02f7bb7595fe58922bc5f851e721b18d516bdf5"),
    ("unbond child A", "58c26142d88fad7eabc57a9cd57dcfdee857b4ea6a3c8d7ce174899f3bee388c", "18c53c7157fc9e5bd20cc00d759b9c982d347df66da4153ab958c77368de5dfc"),
    ("withdraw child A", "5c7297455990135d7cba6039b5ea1f864310c79e2e7456bd04e4bc484f3bb59e", "3dd63e4120c46fee3e7b103eb09fc0128e61dfd63a32d22b9bf95a7135f94138"),
    ("unbond a stake", "061d024d9ade9184b403d4247aa57b00634d02a2afc4f752cd89232dc991a537", "f8764ecb3c3db765c5a0a6861e8a3a6566e6f196705fc9f90afff77829e4e3ab"),
    ("withdraw a stake", "890cb370c12e280dff40fb11230567be8099c2c39d09fc4548b7e43ec40f3932", "911d2270147f660fec11e0ae6161ee4044a423b4a24928ff2378199c8b6c53d2"),
    ("create long order 4", "314fa0d16a15b9bdf442ae58c2350c372d592670e6ba6eeb7eff4c1fb10b3e5a", "30b03e651a302a103b7a65c3710a1b1277483515541d9336a6af10c37c94dcbe"),
    ("create short order 4", "3c28f85feb22939a1e7486e3e1443dd95cbfe085d7a1b76028f9c696844c00f9", "eefc0152b8722043cd96cfc06a5beca781c963798d40bd70d38ecd1af5d1db5b"),
    ("create a second slot", "99cde7f0151cec52b0056b168c7f96f4d1c481f22045c03e1baa50aba766ad05", "d8074749166facddae6856fc8dac1fd5ae5ce5e61a169307501d861b75ed8ca5"),
    ("open pair D", "2f768e653d76df515ea4fea3aa7ec1bf36fdb1a9e37002b3e629fb36b9137ec6", "9631a5ccd666c76b0fb2f03fa2d056f79843d5cc87a9566e4481151c2b9422c3"),
    ("propose a close", "a64a71520d6e55b94410a472ddc781ee71c7d79b96a1175900508491cc147d80", "619138b29bbe261ec3d6c64581a905079de489e6637f434610513b8863bca068"),
    ("finalize the close", "b20acae38be425d9bfcdc3351bd772623010edcc8b8fd1771002ff85e551b947", "bff67b7fef70d488c036329ff1080c1461ac639fe1cb54bbd753e5b1e38471a4"),
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
