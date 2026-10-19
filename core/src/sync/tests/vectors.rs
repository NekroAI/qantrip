//! 固定测试向量（docs/sync-design.md §13）：用固定种子生成，其他平台的实现必须逐字节一致。
//!
//! 输入约定：
//! - 设备 A 的三把私钥种子分别为 32 字节的 0x01、0x02、0x03；设备 B 为 0x11、0x12、0x13
//! - 恢复短语熵为 32 字节的 0x07；vault_id = "v_vectors"；VK(epoch 1) 为 32 字节的 0x2a
//! - 记录 nonce 为 24 字节的 0x05；HPKE 的随机数来自 SHA-256("qantrip-test-rng" || u64_be(计数器)) 的串联
//! - 加入协议：n_N 为 32 字节的 0x0a，n_E 为 32 字节的 0x0b

use std::convert::Infallible;

use crate::sync::codec::{self, hex};
use crate::sync::crypto::{self, DeviceKeys, PhraseLanguage, RecoveryKeys};
use crate::sync::engine::{Keyring, envelope_plaintext};
use crate::sync::record::{Body, Kind, Plaintext, StateEntry};
use crate::sync::roster::{Entry, Op, Roster, Signer, ZERO_HASH, recovery_bind_message};

const VAULT: &str = "v_vectors";

/// 确定性随机数（仅测试向量使用）。
struct TestRng {
    counter: u64,
    buf: Vec<u8>,
}

impl rand_core::TryRng for TestRng {
    type Error = Infallible;
    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        let mut b = [0; 4];
        self.try_fill_bytes(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }
    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        let mut b = [0; 8];
        self.try_fill_bytes(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        while self.buf.len() < dst.len() {
            self.buf.extend(crypto::sha256(&[b"qantrip-test-rng", &self.counter.to_be_bytes()]));
            self.counter += 1;
        }
        let rest = self.buf.split_off(dst.len());
        dst.copy_from_slice(&self.buf);
        self.buf = rest;
        Ok(())
    }
}

impl rand_core::TryCryptoRng for TestRng {}

fn compute() -> Vec<(&'static str, String)> {
    let a = DeviceKeys::from_seeds([1; 32], [2; 32], [3; 32]);
    let b = DeviceKeys::from_seeds([0x11; 32], [0x12; 32], [0x13; 32]);
    let pa = a.public();
    let pb = b.public();
    let rk = RecoveryKeys::from_entropy(&[7; 32]);
    let vk = [0x2a; 32];
    let mut out = vec![
        ("a.x25519", hex(&pa.x25519)),
        ("a.roster", hex(&pa.roster)),
        ("a.auth", hex(&pa.auth)),
        ("a.device_id", pa.device_id()),
        ("b.device_id", pb.device_id()),
        ("recovery.phrase.en", crypto::phrase_from_entropy(&[7; 32], PhraseLanguage::English).to_string()),
        ("recovery.phrase.zh", crypto::phrase_from_entropy(&[7; 32], PhraseLanguage::ChineseSimplified).to_string()),
        ("recovery.x25519", hex(&rk.x_public())),
        ("recovery.ed25519", hex(&rk.s_public())),
        ("vk_commit.e1", hex(&crypto::vk_commit(&vk, VAULT, 1))),
        ("state_record_id.a", crypto::state_record_id(VAULT, &pa.device_id())),
        ("envelope_info", hex(&crypto::envelope_info(VAULT, &pb.device_id(), 1, 2))),
        ("record_aad", hex(&crypto::record_aad(VAULT, "AAAAAAAAAAAAAAAAAAAAAA", 1))),
    ];

    // 花名册：genesis(A) → set_recovery(A) → add_device(B, 由 A 签名)
    let g = Entry::signed(VAULT, 0, ZERO_HASH, Op::Genesis { device: pa.clone(), vk_commit: crypto::vk_commit(&vk, VAULT, 1) }, Signer::Device(pa.device_id()), |m| a.roster_sign(m));
    let bind = rk.sign(&recovery_bind_message(VAULT, &g.hash()));
    let s = Entry::signed(VAULT, 1, g.hash(), Op::SetRecovery { x25519: rk.x_public(), ed25519: rk.s_public(), bind }, Signer::Device(pa.device_id()), |m| a.roster_sign(m));
    let add = Entry::signed(VAULT, 2, s.hash(), Op::AddDevice { device: pb.clone() }, Signer::Device(pa.device_id()), |m| a.roster_sign(m));
    let roster = Roster::from_entries(VAULT, &[g.encode(), s.encode(), add.encode()]).unwrap();
    let (head_idx, head) = roster.head().unwrap();
    out.push(("roster.genesis", hex(&g.encode())));
    out.push(("roster.set_recovery", hex(&s.encode())));
    out.push(("roster.add_device", hex(&add.encode())));
    out.push(("roster.head", format!("{head_idx}:{}", hex(&head))));

    // 记录明文与密文
    let rid = "AAAAAAAAAAAAAAAAAAAAAA";
    let mut pt = Plaintext {
        kind: Kind::Template,
        counter: 1,
        deleted: false,
        writer: pa.device_id(),
        roster_index: head_idx,
        roster_head: head,
        time: 1_792_281_600,
        body: Body::Template { file_name: "zh.md".into(), source: "---\nname: zh\n---\n用中文回答".into() },
        sig: [0; 64],
    };
    out.push(("record.signing_message", hex(&pt.signing_message(VAULT, rid, 1))));
    pt.sign(VAULT, rid, 1, |m| a.roster_sign(m));
    assert!(pt.verify(VAULT, rid, 1, &pa.roster).is_ok());
    assert!(pt.verify(VAULT, rid, 2, &pa.roster).is_err(), "签名绑定 epoch");
    assert!(pt.verify(VAULT, "BAAAAAAAAAAAAAAAAAAAAA", 1, &pa.roster).is_err(), "签名绑定 record_id");
    let plain = pt.encode();
    out.push(("record.plaintext", hex(&plain)));
    out.push(("record.content_hash", hex(&pt.content_hash())));
    let ct = crypto::seal_record(&vk, &[5; 24], VAULT, rid, 1, &plain);
    out.push(("record.ciphertext.sha256", hex(&crypto::sha256(&[&ct]))));
    out.push(("record.ciphertext.len", ct.len().to_string()));
    let (back, hash) = Plaintext::decode_padded(&crypto::open_record(&vk, VAULT, rid, 1, &ct).unwrap()).unwrap();
    assert_eq!(back, pt);
    assert_eq!(hash, pt.content_hash());

    let mut entries = std::collections::BTreeMap::new();
    entries.insert(rid.to_string(), StateEntry { counter: 1, hash: pt.content_hash(), deleted: false });
    let mut state = Plaintext { kind: Kind::DeviceState, counter: 1, body: Body::DeviceState { entries }, ..pt.clone() };
    state.sign(VAULT, &crypto::state_record_id(VAULT, &pa.device_id()), 1, |m| a.roster_sign(m));
    out.push(("record.device_state", hex(&state.encode())));
    let mut tomb = Plaintext { counter: 2, deleted: true, body: Body::Empty, ..pt.clone() };
    tomb.sign(VAULT, rid, 1, |m| a.roster_sign(m));
    out.push(("record.tombstone", hex(&tomb.encode())));

    // HPKE Auth 信封：A → B，钥匙环 {1: VK}
    let mut ring = Keyring::default();
    ring.insert(1, vk);
    let env_pt = envelope_plaintext(&ring, head_idx, &head);
    out.push(("envelope.plaintext", hex(&env_pt)));
    let info = crypto::envelope_info(VAULT, &pb.device_id(), 1, head_idx);
    let mut rng = TestRng { counter: 0, buf: vec![] };
    let (enc, ect) = crypto::seal_envelope(&mut rng, a.enc_secret(), &pb.x25519, &info, &env_pt).unwrap();
    out.push(("envelope.enc", hex(&enc)));
    out.push(("envelope.ct", hex(&ect)));
    assert_eq!(*crypto::open_envelope(b.enc_secret(), &pa.x25519, &enc, &info, &ect).unwrap(), *env_pt);
    // 发送方认证：换一个发送方公钥就打不开
    assert!(crypto::open_envelope(b.enc_secret(), &pb.x25519, &enc, &info, &ect).is_err());

    // 加入协议
    let commit = crypto::join_commit(VAULT, &pb.concat(), &[0x0a; 32]);
    out.push(("join.commit", hex(&commit)));
    out.push(("join.sas", crypto::sas(VAULT, &head, &pb.concat(), &pa.concat(), &[0x0a; 32], &[0x0b; 32])));
    out.push(("join.result_message", hex(&crypto::sha256(&[&crypto::join_result_message(VAULT, [b"commit", b"reply", b"reveal"], b"body")]))));
    out.push(("cbor.keyring", codec::b64u(&ring.encode())));
    out
}

// 已用 Python（hashlib/hmac）独立复核 vk_commit、device_id、state_record_id、join.result_message。
const EXPECTED: &[(&str, &str)] = &[
    ("a.x25519", "a4e09292b651c278b9772c569f5fa9bb13d906b46ab68c9df9dc2b4409f8a209"),
    ("a.roster", "8139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394"),
    ("a.auth", "ed4928c628d1c2c6eae90338905995612959273a5c63f93636c14614ac8737d1"),
    ("a.device_id", "XE42PUJ6MNLIORR6Z3Y53V4SNA"),
    ("b.device_id", "V77DNSAV7TJ6NBTR6RICYT5V2R"),
    ("recovery.phrase.en", "alpha deal scrub asthma idea logic bright thought alpha deal scrub asthma idea logic bright thought alpha deal scrub asthma idea logic bright truly"),
    ("recovery.phrase.zh", "行 候 虾 些 希 筑 知 涌 行 候 虾 些 希 筑 知 涌 行 候 虾 些 希 筑 知 脆"),
    ("recovery.x25519", "bdd7158819574467da2893dd623321a1786e18cc1298ee1867ba2bab1d4f074a"),
    ("recovery.ed25519", "e1af167e350ffa7cd63f3784e944c472f5b18c75872b5990d32b8016c7e79f87"),
    ("vk_commit.e1", "30862276a8e67e43602e505a6eadfa2965e8ae75ef28bfb77fa576b9b30d4bcf"),
    ("state_record_id.a", "nJV-9Gtmp-iH3VFoVl8OKf"),
    ("envelope_info", "71616e747269702f76312f656e76656c6f7065765f766563746f7273563737444e53415637544a364e425452365249435954355632520000000100000002"),
    ("record_aad", "71616e747269702f76312f7265636f726401765f766563746f72734141414141414141414141414141414141414141414100000001"),
    ("roster.genesis", "a6626f706767656e65736973637369675840897499af58780977ca772251b4411a172b2c9bdf28a8c4d949354aa72a70373373750ccb408d7988e182490fe9e57cdd6c76e1a501f615eb0532b0955ca9d40564707265765820000000000000000000000000000000000000000000000000000000000000000065696e64657800667369676e6572781a5845343250554a364d4e4c494f5252365a335935335634534e41677375626a656374a564617574685820ed4928c628d1c2c6eae90338905995612959273a5c63f93636c14614ac8737d16565706f63680166726f7374657258208139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394667832353531395820a4e09292b651c278b9772c569f5fa9bb13d906b46ab68c9df9dc2b4409f8a20969766b5f636f6d6d6974582030862276a8e67e43602e505a6eadfa2965e8ae75ef28bfb77fa576b9b30d4bcf"),
    ("roster.set_recovery", "a6626f706c7365745f7265636f7665727963736967584094df685a8dcfa465551373ba70c4e8592fc124c71c021081d66fc1696818b5c041b3bc815ecf09a967c653d2f44a9e46f12b80bfd11f8326e96195c502af6403647072657658208cf662210c5d26415b1590a19e620a85404aacb3389d16cedbcd911b655aa5b465696e64657801667369676e6572781a5845343250554a364d4e4c494f5252365a335935335634534e41677375626a656374a36462696e645840b992b54f6f2f294511f21d7f2d87a4c4e3ebe5c4569f50ecb49436cf90dcd0cb9da3ba3747f901196b9a6210b98a8dfeb63ecbcf77c7e5d9e0fe3f6c073b240e667832353531395820bdd7158819574467da2893dd623321a1786e18cc1298ee1867ba2bab1d4f074a67656432353531395820e1af167e350ffa7cd63f3784e944c472f5b18c75872b5990d32b8016c7e79f87"),
    ("roster.add_device", "a6626f706a6164645f6465766963656373696758402f532b1a6c3f275dd77b167575b25ed58371368a01835f1081ac86f1eaee28d9610214d8906986b9d08cd8d6e0e832f9d1c403f046cb9ad9a6ee393c285be408647072657658204d8cbb468ab6e4fce7e9cb7bce8b41b7f543b9b68d5bfbf1d2d4ffc5cc6f695065696e64657802667369676e6572781a5845343250554a364d4e4c494f5252365a335935335634534e41677375626a656374a36461757468582066cd608b928b88e50e0efeaa33faf1c43cefe07294b0b87e9fe0aba6a3cf763366726f737465725820204040e364c10f2bec9c1fe500a1cd4c247c89d650a01ed7e82caba867877c216678323535313958207b4e909bbe7ffe44c465a220037d608ee35897d31ef972f07f74892cb0f73f13"),
    ("roster.head", "2:0f4d8317cf409e50855bc8d3af48df1e95f1ae4352591b50e140f4a462046bc1"),
    ("record.signing_message", "71616e747269702f76312f7265636f72642d736967a764686173685820c4f112e485a970ea118761ee2b2d2fc116b72359cbc7a28b1a6722916331a8786565706f63680166777269746572781a5845343250554a364d4e4c494f5252365a335935335634534e4167636f756e74657201687661756c745f696469765f766563746f7273697265636f72645f696476414141414141414141414141414141414141414141416c726f737465725f696e64657802"),
    ("record.plaintext", "a863736967584045242a4377e4a3baa7de04d6c44ce745f80cf54dd68939fefa0c02ba0cd18e0153cc353231e81586ec8da374189179b364efa0ffc27ebf47d92346dbb4148c0a64626f6479a266736f7572636578202d2d2d0a6e616d653a207a680a2d2d2d0ae794a8e4b8ade69687e59b9ee7ad946966696c655f6e616d65657a682e6d646474696d651a6ad40c0064747970656874656d706c61746566726f73746572a2646865616458200f4d8317cf409e50855bc8d3af48df1e95f1ae4352591b50e140f4a462046bc165696e6465780266777269746572781a5845343250554a364d4e4c494f5252365a335935335634534e4167636f756e746572016764656c65746564f4"),
    ("record.content_hash", "c4f112e485a970ea118761ee2b2d2fc116b72359cbc7a28b1a6722916331a878"),
    ("record.ciphertext.sha256", "1af20cbaf565af160be90cf3ef1fa9b51bf6cfefd272db8b232e9891f4486ab0"),
    ("record.ciphertext.len", "553"),
    ("record.device_state", "a86373696758406d0f3eb46bd61b497677c612f2467ccfbd75a8fe56a47a50e8a17e2334ec87770ff5ff21b29e2764cdc7c896783648e4682306a1cd95a3a2e869357f18655f0364626f6479a167656e7472696573a1764141414141414141414141414141414141414141414183015820c4f112e485a970ea118761ee2b2d2fc116b72359cbc7a28b1a6722916331a878f46474696d651a6ad40c0064747970656c6465766963655f737461746566726f73746572a2646865616458200f4d8317cf409e50855bc8d3af48df1e95f1ae4352591b50e140f4a462046bc165696e6465780266777269746572781a5845343250554a364d4e4c494f5252365a335935335634534e4167636f756e746572016764656c65746564f4"),
    ("record.tombstone", "a8637369675840199f2ba9e213c397c2f2a55d8d4e7cc79dc5a55a574a98692f07007d1ef1f8a8d3a23441796a3917a5241dd5688edbdb7611f330f651695924206047d3d69d0864626f6479a06474696d651a6ad40c0064747970656874656d706c61746566726f73746572a2646865616458200f4d8317cf409e50855bc8d3af48df1e95f1ae4352591b50e140f4a462046bc165696e6465780266777269746572781a5845343250554a364d4e4c494f5252365a335935335634534e4167636f756e746572026764656c65746564f5"),
    ("envelope.plaintext", "a2646b657973a10158202a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a66726f73746572a2646865616458200f4d8317cf409e50855bc8d3af48df1e95f1ae4352591b50e140f4a462046bc165696e64657802"),
    ("envelope.enc", "a304558f4702ec0e7d5bf3a0dfde691466a92fe08d2483e6922ed673c96d5e69"),
    ("envelope.ct", "3081151fb23e989671586a3aff206620c85b0307fb01edd599ff5d5e74e653ac9bbcfe25362a13b0ef51ecc371dca29bcd1380e37128627742d3da756e818c49c87bf2395211670d1f6165aef1d6fda9d1c51d99270041fba850249a5277b8cc3743b432af4b3f9007e2411bc561dda9"),
    ("join.commit", "8828557875f71542eaac583c9d13c0aab04802ac5a1335406a3079df9f9b11f3"),
    ("join.sas", "ZJYS-FGMM"),
    ("join.result_message", "406fe52419a24b531a9869b12984a987667d0f509ef4b1ea3f48983973bbd85f"),
    ("cbor.keyring", "oQFYICoqKioqKioqKioqKioqKioqKioqKioqKioqKioqKioq"),
];

#[test]
fn fixed_vectors() {
    let got = compute();
    assert_eq!(got.len(), EXPECTED.len());
    for ((k, v), (ek, ev)) in got.iter().zip(EXPECTED) {
        assert_eq!(k, ek);
        assert_eq!(v, ev, "测试向量 {k} 不一致");
    }
}

#[test]
fn record_layer_round_trip_and_padding() {
    let vk = [0x2a; 32];
    for size in [0usize, 10, 300, 5000, 70_000] {
        let pt = Plaintext {
            kind: Kind::Template,
            counter: 3,
            deleted: false,
            writer: "W".repeat(26),
            roster_index: 0,
            roster_head: [0; 32],
            time: 0,
            body: Body::Template { file_name: "a.md".into(), source: "x".repeat(size) },
            sig: [7; 64],
        };
        let plain = pt.encode();
        let ct = crypto::seal_record(&vk, &[1; 24], VAULT, "rid_rid_rid_rid_", 1, &plain);
        assert_eq!(ct.len() - 41, crypto::padded_len(plain.len()));
        let padded = crypto::open_record(&vk, VAULT, "rid_rid_rid_rid_", 1, &ct).unwrap();
        assert_eq!(Plaintext::decode_padded(&padded).unwrap().0, pt);
        // 填充中出现非零字节、或填充长度不对，都拒绝
        let mut bad = padded.to_vec();
        *bad.last_mut().unwrap() = 1;
        assert!(Plaintext::decode_padded(&bad).is_err());
        let mut short = padded.to_vec();
        short.push(0);
        assert!(Plaintext::decode_padded(&short).is_err());
    }
    // 墓碑必须没有内容；状态记录不能是墓碑
    let tomb = Plaintext {
        kind: Kind::Template,
        counter: 1,
        deleted: true,
        writer: "W".into(),
        roster_index: 0,
        roster_head: [0; 32],
        time: 0,
        body: Body::Template { file_name: "a.md".into(), source: "leak".into() },
        sig: [0; 64],
    };
    let mut padded = tomb.encode();
    padded.resize(crypto::padded_len(padded.len()), 0);
    assert!(Plaintext::decode_padded(&padded).is_err());
    // 预留的设置与 AI 配置类型可以编码往返
    for body in [
        Body::Setting { key: "theme".into(), value: codec::text("dark") },
        Body::AiProvider {
            id: "p1".into(),
            protocol: "openai".into(),
            base_url: "https://api.example".into(),
            api_key: "sk-test".into(),
            model: "m".into(),
            params: codec::map(vec![("t", codec::uint(1))]),
        },
    ] {
        let kind = if matches!(body, Body::Setting { .. }) { Kind::Setting } else { Kind::AiProvider };
        let pt = Plaintext { kind, deleted: false, body, ..tomb.clone() };
        let mut p = pt.encode();
        p.resize(crypto::padded_len(p.len()), 0);
        assert_eq!(Plaintext::decode_padded(&p).unwrap().0, pt);
    }
}

#[test]
fn keyring_round_trip() {
    let mut ring = Keyring::default();
    ring.insert(1, [1; 32]);
    ring.insert(300, [2; 32]);
    let back = Keyring::decode(&ring.encode()).unwrap();
    assert_eq!(back.epochs(), vec![1, 300]);
    assert_eq!(back.get(300), Some(&[2; 32]));
}
