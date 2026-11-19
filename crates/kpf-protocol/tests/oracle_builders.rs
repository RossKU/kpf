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
    ("create stake", "03660e0650773c70a99a1d2d8daa2fab686770308b8e258b6b16b6a78f8fb9d5", "33679faf9367c079899fab174777f36ec93324c7925f531223fe653398e5fc65"),
    ("create the underwriter's stake", "9efee37daef0df3ec923e6714f24a61fbd12b2ae84e101631430fd686e191051", "e493dd963bae803bccfb2522a6eb59dbf20c8856ffc8ccc1ec059a288b8747f0"),
    ("create long order", "8d30fdc395b4536ee0bbf3f97bf55535428376410ecdea51ac5725b2aea79870", "2a5ca870f44662c4bb95667b25fd6d81225c4c3de12efbd79b87f9da999bb9ae"),
    ("create short order", "9fae47a9748b39cc17e3c91c8c575eaac1669ebd5e7ee440d619329d6a24ba83", "c3ff5a6e98fdf92b324225c22938f56ae56d5863db05f3332929eb4a8269b3ba"),
    ("open pair A", "a6666ca373261ca6b768a467a1e63bc85bdaa0808543574e1f06fd8dde84e0f2", "26a204135c190351f37d52e35db582f8b928dd07e4b0b9bf0d9ad67d0c6cf2da"),
    ("open pair B", "98d3efcc2d34e4bfc4ad1b3aca25b99c885523561fc723dd046298e6ad521e64", "44e99c89d242f6024438f0997fb966a9831b7d4eb227e14ff11a4e5dddcbd040"),
    ("propose a liquidation", "adbcdfdb069fe59b328cdbbbbded1699143547162a2995732bf439018ab96717", "cdfb229bf3f4f2970b3c7af6c57103ac28dd580ee24d6bb2789e2a91123e3e4a"),
    ("contest", "dd113e1e606058257333864e5e7e40131a4df852439013deb758c6e7bbdd5c33", "162490c5180c7d4eaa59b7d81a14860c33851f8236fe3c1cc45183fcfd373f38"),
    ("resolve and decide (refuted)", "8240850108bcc9b9deb0f17f03160909fc0a8059a0e618f7a678dd407e2ce181", "318ca4d526f7a99f0abbab1e6e24d298b9cd473d39dd73298c7d4a650b9f9c9a"),
    ("finalize refuted (restore)", "68dfffc9a96b119b2fb942079acc80dade7a1a34d6dbe38b37eefe9d9671a092", "5110d95712a96970c1903dbc094c935d45827e743ccf9d55cac7f71a8f4adbd8"),
    ("settle the opener's receipt", "003a51df0c2af4655bc758f72a0fbd36178c849210b75ebc0d4ec115392d17d2", "ed0b7a97c155aac70c775eafdb029861c01edf2f279bb56f14bdefb96dcac5c8"),
    ("close the pool", "57cea8f70350dda158a89fbdd01fcd39a13f659ec8651e905a1db149af7183cf", "8b1c397c97ce191aaa1eaac28a9a9fbba4576e52c1eea78deaeb32f42fa45eee"),
    ("propose again", "2a032ee88b65413dd51c40fb654296c9a73353ffd057c4f592235f88d811f540", "6ec96410d39f69bafcd2c4470237b00ea43d71ede2cf00c31e4e4b1972ac7ad7"),
    ("create a second stake", "4f70794569c748301bef78e2806255354c4f6b6745257445c46bdfab89e0d975", "aa2d43d936c89687e31388fed49d01470925c7557892feb47a926bf7a8a28907"),
    ("contest a true claim", "af0f034b4d373921e6378b242c02e770f3792daab6d87c08bda77d1556579ecd", "a2297fcdc66cedd158708c6d1d7292265fddf60fcb9e3afbea8de2d8c24cb46d"),
    ("deposit the defence", "bb293ea4901375f8d56308e6dfc446f5758added06c3a6c8b34a73e78235287f", "acacb5447d8d0db1892729c07fa3ba004ccf1dcff25539ffcd8dccc00bd6ff35"),
    ("resolve and decide (upheld)", "873d094d3e7ac28409bad7a70ed812cbb5cf5956c927873b62c00717b292a5e4", "3366427a0cfa582db58fd09104f6b44c274d89cb22b118d6626d206064aaee76"),
    ("finalize upheld (forfeit)", "c4af4e88da6b286cdbf955875382d6f5b5e0877618c7eefd0032e6c8a269cdef", "e875fd1c5b11d46e21bb6768f09b5a5221487d6ff43f5076ed8df53f96c12173"),
    ("settle a winning and a losing receipt", "6eded51f21e0f61f5d90237a9c0b3922c4d13bc781077fa3300f415cd0b65f7d", "cf7f7ad76c8adf0c192cd1579478c40b31d368efff8330d663d68424ab684904"),
    ("close the pool with a burn", "cb4ce99f9987da187f5f900fa0a617173fd4fb95b501b7947451c6ae2f3dec1b", "5f92939d35cf968c4cd818f78a1d758e60413ac1d1ff30accea276b273b08cb2"),
    ("propose on B", "721fff367db74ceb7db1838daa1741c0862fd0a42123004f42c95d22c1b20b25", "ce8aa228b906f77af280eb229b54e6b74f39086fac327f6d83c7a931b0e2a622"),
    ("cure", "7ad1ab1f60d5913d5337087a9afd14e7608071ce46b4e533c714e0203298ed79", "81c631648a81ab6c910dc7e348a037bd6d93fa8837d7e01f3d8fc2b72de8edc3"),
    ("renew B", "b1780a583d3930419dba0dde25bc9515d8a0f336e6881d67cdfcf7592b16bf4a", "a03de639cb0625ada0174fd021813b8809b654f3330ace866eca9c0a12bfec81"),
    ("amend: list the long", "ba7a43c8c43540b3695c08f613733c54f36ecd60d5d98586f21ad92a9d0369fe", "5fbf9b66665c284e17d9f975d3811f81290326ee95a57e01a4a9a5764ba69eaf"),
    ("transfer the long to a signing taker", "0b5e2069633baa1c6f8a099865a4738f5aaedd3d5c4f0aaae3edb998f183cd3a", "3908df12e4799db88e45fc62048bcc640e344fb92781ba843ed8c769c97c158a"),
    ("amend: the taker lists", "8be4d0360068d050678bfcd58e68ab0ebd3e7bee260708169d6520d48ff27d21", "55ad9326a783b7b45f4ec9dab900137e210876a1b75dffcf6dd26aabefa9ee20"),
    ("amend: the short lists", "8da98917fabaa95d9ddfc69ef91fb7e53e0c740b291a49e62d27eb54b6057bd9", "853b62c142040abef0d424501db04766a996938db081b55d435904ed210cfe6a"),
    ("mutual close", "922cee2eb1405d53e4bc02ff7848939171e46c4cf92296d0770a5ed0a35a8a06", "e97cfcbf71404aeb2306639a9075e8633fcdf9bc9daf6da4f7f53f41bf46c4f3"),
    ("lapse settle", "34e6d28504b8dc671c1172d0d7433bea70651d8b2748a9afcf85534e6fa4bb59", "b03f2877e29fbee197a5845994858d99ceb7946d4b8fef46b1c15cff006834aa"),
    ("create long order 3", "6111562188dc173cf6062cdb673a87409a202f99612bc48f22b75c4604af2fc2", "05630999200ed4fdb67f3af82bc3e22791e5ba7d680deb284ce4a448ec59da12"),
    ("create short order 3", "c70fe9405312ee11cb5202c4a0a4ca34c0cf166540d2a60c05b6e28d0ee538cd", "c03dcf6d7797bb244de8663f6b328dda73193a8d8ca934975f46c6b730fa62bf"),
    ("open pair C", "06993f6c9d40c903dfabb586e9ab2b90f2bf3c2a3cf682039bd81217978d34c8", "cb3b098d13d9fe5f2956dc3c703a8267990f517b27ad9661760408b0825e2b54"),
    ("cooperative close", "0ed23909c30bdae36994e602c5f191bfe4fc866acec0102041d93703de7c88ef", "5d69fb62e0ace9d5fec358a5bc52d60d8ae5bbb778cda7f05a4d54a8b9190ed1"),
    ("advance the slot", "57a169045039aeb441e346e51408ee4f090d7181950e9186d98ca326bf970683", "8c5d2b71c9c057d39943a7389c0e812198de571010c6737129ee667612d696ae"),
    ("split the slot", "75ff3ad53727afe2fb626807af8da48de9804376302803023d1a9fe222b3ed5b", "9ca4bd5472ffe9fb22bad49016a457b6360276d92f5496600e40b9effcc347ca"),
    ("unbond child A", "7bcec5f333a92602da7dc2222767093027a20c1b5652b2563b5ea83933097f68", "dfab3133c566d4304a109f6580b1528bb938f11f2b76d131df62f61325923026"),
    ("withdraw child A", "b367865a9c20382a0eb9f3f4467e6bd829a699d3a2105cf54afa175404e2a177", "8465bfd8367200ffafcd0ef96fddc90d7198a2ab99c5a2fe808e4f68b424f92d"),
    ("unbond a stake", "c326e038d2ff43a594956e88a5429f6857ae861ea68d9571f411ff8df3046f1e", "e5b3c2d4fae97f76acdf91a896be2d2afe167ba0a485b7fe80f28e5ebaf831bd"),
    ("withdraw a stake", "77dc74ac1de00028caeddf67d11a693ef32bdd3183000dc9ffbdd64b575d9f1e", "6929a357fa1d01f08c0dd8b7afe21ba4a737a87346221af4c76fd64fa688dc69"),
    ("create long order 4", "05e7f85f89087fbdf2489864b03da1132987c318a036c341e32bbc77d54a3d5e", "d0bbdcd3012abad2927ee6b5c9dd22d03ca538254e882298dd24f808ba62c4b4"),
    ("create short order 4", "c3eaf8b045b0ce6922f5e87bace8438e1342ce94f92b571f84eacba14fa1d8b4", "33c009b7cdadcf3da99f76b0d66430083e021a0bc9a1f5bb31b8f2fa6ee301db"),
    ("create a second slot", "99cde7f0151cec52b0056b168c7f96f4d1c481f22045c03e1baa50aba766ad05", "d8074749166facddae6856fc8dac1fd5ae5ce5e61a169307501d861b75ed8ca5"),
    ("open pair D", "038c01eace96b4d2179efda97e1114b9141f6128f7cf8ea4a1613435b1da7105", "5683747910521aac0be9bcfc181b8c928e9fec66659fc4e15ff7a9edff4158d0"),
    ("propose a close", "8d54cf3f7dcc85102c6f5ed889255721633f2b6332ac6bea7924aa11debc7a97", "fb82ada5fb06b3e6528dd6b24733abc361b9091429d9612146d710eee1965825"),
    ("finalize the close", "77961119d63ef4756b8edc8f9d6a7fca685267bea4c0f8a9ea0bfcaebcd681bd", "a64751a418c82a502e7902f0964240edf0b547b81084a60ed71fd32a8e80d04d"),
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
/// must cover that output: redirecting it breaks the vouch input. A liquidation's bond is posted by the underwriter
/// (the vouch input funds it: the owner gets `value - bond`, no output if that is zero); a close's bond is the closer's
/// (the owner gets the whole value back).
fn assert_vouch_returned(sg: &OSigned, vouch: &KeyUtxo, funder: &[u8; 32], is_close: bool, bond: i64) {
    let back = if is_close { vouch.amount } else { vouch.amount - bond as u64 };
    let closer_signs = is_close;
    let to_owner: Vec<usize> = (0..sg.tx.outputs.len()).filter(|&i| sg.tx.outputs[i].script_public_key == p2pk_spk(&vouch.key)).collect();
    assert_eq!(to_owner.len(), 1, "exactly one output goes to the underwriter: {:?}", sg.output_labels);
    assert_eq!(sg.tx.outputs[to_owner[0]].value, back, "the underwriter gets exactly {back}: {:?}", sg.output_labels);
    // The bond is inside the pending part (the pair's value plus the bond, whole claim).
    assert!(sg.tx.outputs[0].value as i64 >= sg.entries[0].amount as i64 + bond, "the pending part holds the bond");
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
        assert!(e.contains("less than the claim's bond"), "{e}");
        let e = propose(&pair_a, &term, &with(w.bond as u64 + 1), &[fund.clone()], &ek, r).unwrap_err().to_string();
        assert!(e.contains("below the plain-output minimum"), "{e}");
        let (b, _) = propose(&pair_a, &term, &with(w.bond as u64), &[fund], &ek, r).unwrap();
        let s = b.sign(&c.keys).unwrap();
        assert!(kpf_protocol::verify::verify(&s.tx, &s.entries).is_ok());
        assert!(s.tx.outputs.iter().all(|o| o.script_public_key != p2pk_spk(&uk)), "an exact fit leaves nothing to pay back");
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
