//! 密码学原语（docs/sync-design.md 第 3、4、6 节）。只组合设计 §12 列出的库，不自行实现原语。
//!
//! 所有域分离字符串集中在本文件开头；各平台实现必须逐字节一致（测试向量见 `tests/vectors.rs`）。

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use hpke::aead::ChaCha20Poly1305 as HpkeChaCha;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem, OpModeR, OpModeS, Serializable};
use rand_core::CryptoRng;
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::codec;

// MARK: - 域分离字符串

pub const D_DEVICE_ID: &[u8] = b"qantrip/v1/device-id";
pub const D_RECOVERY_X: &[u8] = b"qantrip/v1/recovery/x25519";
pub const D_RECOVERY_S: &[u8] = b"qantrip/v1/recovery/ed25519";
/// 恢复公钥对花名册历史的绑定签名（设计未定义，见 `roster.rs` 中 set_recovery 的说明）
pub const D_RECOVERY_BIND: &[u8] = b"qantrip/v1/recovery-bind";
pub const D_VK_COMMIT: &[u8] = b"qantrip/v1/vk-commit";
pub const D_ENVELOPE: &[u8] = b"qantrip/v1/envelope";
pub const D_RECORD: &[u8] = b"qantrip/v1/record";
/// 记录明文中的写入者签名（v0.3）
pub const D_RECORD_SIG: &[u8] = b"qantrip/v1/record-sig";
pub const D_STATE: &[u8] = b"qantrip/v1/state";
pub const D_ROSTER: &[u8] = b"qantrip/v1/roster";
pub const D_COMMIT: &[u8] = b"qantrip/v1/commit";
pub const D_SAS: &[u8] = b"qantrip/v1/sas";
/// 加入结果的批准设备签名（v0.5）
pub const D_JOIN_RESULT: &[u8] = b"qantrip/v2/join-result";

/// 记录密文格式版本
pub const RECORD_VERSION: u8 = 0x01;
pub const RECOVERY_RECIPIENT: &str = "recovery";

// MARK: - 随机数

/// 系统随机数（`getrandom`）；失败时 panic，由 UniFFI 转为调用方可见的错误。
pub fn system_rng() -> rand_core::UnwrapErr<getrandom::SysRng> {
    rand_core::UnwrapErr(getrandom::SysRng)
}

pub fn random<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).expect("系统随机数不可用");
    out
}

pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

fn hkdf32(ikm: &[u8], info: &[&[u8]]) -> Zeroizing<[u8; 32]> {
    let info: Vec<u8> = info.concat();
    let mut out = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(None, ikm).expand(&info, out.as_mut()).expect("32 字节在 HKDF 输出范围内");
    out
}

/// 常量时间比较（长度不同直接返回 false）。
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// MARK: - 签名

pub fn ed25519_public(seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(seed).verifying_key().to_bytes()
}

pub fn sign(seed: &[u8; 32], msg: &[u8]) -> [u8; 64] {
    SigningKey::from_bytes(seed).sign(msg).to_bytes()
}

/// Ed25519 严格校验（拒绝小阶公钥与非规范签名）。
pub fn verify(public: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> Result<(), String> {
    let key = VerifyingKey::from_bytes(public).map_err(|_| "签名公钥无效".to_string())?;
    key.verify_strict(msg, &Signature::from_bytes(sig)).map_err(|_| "签名校验失败".to_string())
}

pub fn x25519_public(secret: &[u8; 32]) -> [u8; 32] {
    x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(*secret)).to_bytes()
}

// MARK: - 设备密钥（§3.1）

/// 一台设备的三把公钥：加密（X25519）、花名册签名（Ed25519）、登录认证（Ed25519）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKeys {
    pub x25519: [u8; 32],
    pub roster: [u8; 32],
    pub auth: [u8; 32],
}

impl PublicKeys {
    /// 加入协议中的 pk：三把公钥按 x25519 || roster || auth 拼接。
    pub fn concat(&self) -> [u8; 96] {
        let mut out = [0u8; 96];
        out[..32].copy_from_slice(&self.x25519);
        out[32..64].copy_from_slice(&self.roster);
        out[64..].copy_from_slice(&self.auth);
        out
    }

    pub fn from_concat(b: &[u8]) -> Result<PublicKeys, String> {
        if b.len() != 96 {
            return Err("设备公钥长度不正确".into());
        }
        let part = |i: usize| -> [u8; 32] { b[i * 32..(i + 1) * 32].try_into().unwrap() };
        Ok(PublicKeys { x25519: part(0), roster: part(1), auth: part(2) })
    }

    pub fn device_id(&self) -> String {
        device_id(self)
    }
}

/// `device_id = Base32(SHA-256("qantrip/v1/device-id" || x25519 || roster || auth))[0..26]`
pub fn device_id(pk: &PublicKeys) -> String {
    let h = sha256(&[D_DEVICE_ID, &pk.x25519, &pk.roster, &pk.auth]);
    codec::base32(&h)[..26].to_string()
}

/// 设备私钥。只在内存中出现，由平台安全存储保存；析构时清零。
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct DeviceKeys {
    enc: [u8; 32],
    roster: [u8; 32],
    auth: [u8; 32],
}

impl DeviceKeys {
    pub fn generate() -> DeviceKeys {
        DeviceKeys::from_seeds(random(), random(), random())
    }

    pub fn from_seeds(enc: [u8; 32], roster: [u8; 32], auth: [u8; 32]) -> DeviceKeys {
        DeviceKeys { enc, roster, auth }
    }

    pub fn to_bytes(&self) -> Zeroizing<Vec<u8>> {
        Zeroizing::new([self.enc, self.roster, self.auth].concat())
    }

    pub fn from_bytes(b: &[u8]) -> Result<DeviceKeys, String> {
        if b.len() != 96 {
            return Err("设备私钥长度不正确".into());
        }
        let part = |i: usize| -> [u8; 32] { b[i * 32..(i + 1) * 32].try_into().unwrap() };
        Ok(DeviceKeys { enc: part(0), roster: part(1), auth: part(2) })
    }

    pub fn public(&self) -> PublicKeys {
        PublicKeys { x25519: x25519_public(&self.enc), roster: ed25519_public(&self.roster), auth: ed25519_public(&self.auth) }
    }

    pub fn device_id(&self) -> String {
        device_id(&self.public())
    }

    pub fn enc_secret(&self) -> &[u8; 32] {
        &self.enc
    }

    /// 花名册密钥只签花名册条目与加入协议消息（调用方负责加域前缀）。
    pub fn roster_sign(&self, msg: &[u8]) -> [u8; 64] {
        sign(&self.roster, msg)
    }
}

// MARK: - 恢复短语（§3.3）

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PhraseLanguage {
    English,
    ChineseSimplified,
}

/// 由恢复短语熵派生的两把密钥。
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct RecoveryKeys {
    x: [u8; 32],
    s: [u8; 32],
}

impl RecoveryKeys {
    /// `recovery_x = HKDF-SHA256(seed, info = "qantrip/v1/recovery/x25519")`，`recovery_s` 同理。
    /// seed 取 BIP39 编码的 256 位熵本身（不用 BIP39 的 PBKDF2 种子，设计要求不用慢速 KDF）。
    pub fn from_entropy(entropy: &[u8; 32]) -> RecoveryKeys {
        RecoveryKeys { x: *hkdf32(entropy, &[D_RECOVERY_X]), s: *hkdf32(entropy, &[D_RECOVERY_S]) }
    }

    pub fn from_phrase(phrase: &str) -> Result<RecoveryKeys, String> {
        Ok(RecoveryKeys::from_entropy(&*parse_phrase(phrase)?))
    }

    pub fn x_secret(&self) -> &[u8; 32] {
        &self.x
    }

    pub fn x_public(&self) -> [u8; 32] {
        x25519_public(&self.x)
    }

    pub fn s_public(&self) -> [u8; 32] {
        ed25519_public(&self.s)
    }

    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        sign(&self.s, msg)
    }
}

fn bip39_language(l: PhraseLanguage) -> bip39::Language {
    match l {
        PhraseLanguage::English => bip39::Language::English,
        PhraseLanguage::ChineseSimplified => bip39::Language::SimplifiedChinese,
    }
}

/// 256 位熵 → 24 个词（中文词表以空格分隔）。
pub fn phrase_from_entropy(entropy: &[u8; 32], lang: PhraseLanguage) -> Zeroizing<String> {
    let m = bip39::Mnemonic::from_entropy_in(bip39_language(lang), entropy).expect("32 字节熵合法");
    Zeroizing::new(m.words().collect::<Vec<_>>().join(" "))
}

/// 解析恢复短语：忽略大小写与多余空白；中文短语可以不加空格。必须是 24 个词。
pub fn parse_phrase(input: &str) -> Result<Zeroizing<[u8; 32]>, String> {
    let lower = Zeroizing::new(input.trim().to_lowercase());
    let words: Vec<String> = if lower.chars().any(|c| c.is_ascii_alphabetic()) {
        lower.split_whitespace().map(str::to_string).collect()
    } else {
        lower.chars().filter(|c| !c.is_whitespace()).map(|c| c.to_string()).collect()
    };
    let normalized = Zeroizing::new(words.join(" "));
    for lang in [PhraseLanguage::English, PhraseLanguage::ChineseSimplified] {
        if let Ok(m) = bip39::Mnemonic::parse_in_normalized(bip39_language(lang), &normalized) {
            if m.word_count() != 24 {
                return Err("恢复短语应为 24 个词".into());
            }
            let (arr, len) = m.to_entropy_array();
            let mut out = Zeroizing::new([0u8; 32]);
            if len != 32 {
                return Err("恢复短语应为 24 个词".into());
            }
            out.copy_from_slice(&arr[..32]);
            return Ok(out);
        }
    }
    Err("恢复短语无效：请检查拼写与顺序".into())
}

// MARK: - VK 承诺（§3.4）

/// `vk_commit = HKDF-SHA256(VK_e, info = "qantrip/v1/vk-commit" || vault_id || u32_be(epoch))`
pub fn vk_commit(vk: &[u8; 32], vault_id: &str, epoch: u32) -> [u8; 32] {
    *hkdf32(vk, &[D_VK_COMMIT, vault_id.as_bytes(), &epoch.to_be_bytes()])
}

// MARK: - HPKE Auth 信封（§3.4）

/// `info = "qantrip/v1/envelope" || vault_id || recipient_id || u32_be(epoch) || u32_be(roster_index)`
pub fn envelope_info(vault_id: &str, recipient_id: &str, epoch: u32, roster_index: u32) -> Vec<u8> {
    [D_ENVELOPE, vault_id.as_bytes(), recipient_id.as_bytes(), &epoch.to_be_bytes(), &roster_index.to_be_bytes()].concat()
}

type HpkeKem = X25519HkdfSha256;

fn hpke_secret(sk: &[u8; 32]) -> Result<<HpkeKem as Kem>::PrivateKey, String> {
    <HpkeKem as Kem>::PrivateKey::from_bytes(sk).map_err(|_| "HPKE 私钥无效".to_string())
}

fn hpke_public(pk: &[u8; 32]) -> Result<<HpkeKem as Kem>::PublicKey, String> {
    <HpkeKem as Kem>::PublicKey::from_bytes(pk).map_err(|_| "HPKE 公钥无效".to_string())
}

/// HPKE Auth 模式加密（DHKEM(X25519, HKDF-SHA256) / HKDF-SHA256 / ChaCha20Poly1305），AAD 为空。
/// 返回封装密钥 `enc` 与密文。
pub fn seal_envelope(
    rng: &mut impl CryptoRng,
    sender_sk: &[u8; 32],
    recipient_pk: &[u8; 32],
    info: &[u8],
    plaintext: &[u8],
) -> Result<([u8; 32], Vec<u8>), String> {
    let sk = hpke_secret(sender_sk)?;
    let pk = <HpkeKem as Kem>::sk_to_pk(&sk);
    let mode = OpModeS::Auth((sk, pk));
    let (enc, ct) = hpke::single_shot_seal_with_rng::<HpkeChaCha, HkdfSha256, HpkeKem>(&mode, &hpke_public(recipient_pk)?, info, plaintext, b"", rng)
        .map_err(|_| "信封加密失败".to_string())?;
    let enc: [u8; 32] = enc.to_bytes().as_slice().try_into().map_err(|_| "封装密钥长度不正确".to_string())?;
    Ok((enc, ct))
}

/// 打开信封；同时认证发送方（`sender_pk` 必须是信封生成时在册设备的加密公钥）。
pub fn open_envelope(recipient_sk: &[u8; 32], sender_pk: &[u8; 32], enc: &[u8; 32], info: &[u8], ct: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    let sk = hpke_secret(recipient_sk)?;
    let enc = <HpkeKem as Kem>::EncappedKey::from_bytes(enc).map_err(|_| "封装密钥无效".to_string())?;
    let mode = OpModeR::Auth(hpke_public(sender_pk)?);
    hpke::single_shot_open::<HpkeChaCha, HkdfSha256, HpkeKem>(&mode, &sk, &enc, info, ct, b"")
        .map(Zeroizing::new)
        .map_err(|_| "信封认证失败".to_string())
}

// MARK: - 记录密文（§4.2）

/// `aad = "qantrip/v1/record" || ver || vault_id || record_id || u32_be(epoch)`
pub fn record_aad(vault_id: &str, record_id: &str, epoch: u32) -> Vec<u8> {
    [D_RECORD, &[RECORD_VERSION], vault_id.as_bytes(), record_id.as_bytes(), &epoch.to_be_bytes()].concat()
}

/// 长度填充：256 字节的整数倍；超过 4 KiB 后为 2 的整数次幂。
pub fn padded_len(n: usize) -> usize {
    if n <= 4096 { n.max(1).div_ceil(256) * 256 } else { n.next_power_of_two() }
}

/// `ciphertext = ver(0x01) || nonce(24) || XChaCha20-Poly1305(VK, nonce, aad, padded_plaintext)`，填充为零字节。
pub fn seal_record(vk: &[u8; 32], nonce: &[u8; 24], vault_id: &str, record_id: &str, epoch: u32, plaintext: &[u8]) -> Vec<u8> {
    let mut padded = Zeroizing::new(plaintext.to_vec());
    padded.resize(padded_len(plaintext.len()), 0);
    let cipher = XChaCha20Poly1305::new(vk.into());
    let aad = record_aad(vault_id, record_id, epoch);
    let ct = cipher.encrypt(&XNonce::from(*nonce), Payload { msg: &padded, aad: &aad }).expect("加密不会失败");
    [&[RECORD_VERSION][..], nonce, &ct].concat()
}

/// 解密并校验 AEAD（含 AAD）。返回带填充的明文。
pub fn open_record(vk: &[u8; 32], vault_id: &str, record_id: &str, epoch: u32, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    if ciphertext.len() < 1 + 24 + 16 || ciphertext[0] != RECORD_VERSION {
        return Err("密文格式不正确".into());
    }
    let nonce: [u8; 24] = ciphertext[1..25].try_into().unwrap();
    let cipher = XChaCha20Poly1305::new(vk.into());
    let aad = record_aad(vault_id, record_id, epoch);
    cipher
        .decrypt(&XNonce::from(nonce), Payload { msg: &ciphertext[25..], aad: &aad })
        .map(Zeroizing::new)
        .map_err(|_| "密文校验失败".to_string())
}

// MARK: - 记录 ID

/// 新记录的 ID：128 位随机数（base64url，22 个字符）。
pub fn random_record_id() -> String {
    codec::b64u(&random::<16>())
}

/// 设备状态记录 ID（§4.4）：`Base64url(SHA-256("qantrip/v1/state" || vault_id || device_id))[0..22]`
pub fn state_record_id(vault_id: &str, device_id: &str) -> String {
    codec::b64u(&sha256(&[D_STATE, vault_id.as_bytes(), device_id.as_bytes()]))[..22].to_string()
}

// MARK: - 加入协议（§6.2）

/// `c = SHA-256("qantrip/v1/commit" || vault_id || pk_N || n_N)`
pub fn join_commit(vault_id: &str, pk_n: &[u8; 96], n_n: &[u8; 32]) -> [u8; 32] {
    sha256(&[D_COMMIT, vault_id.as_bytes(), pk_n, n_n])
}

/// `SAS = 截取 40 位(SHA-256("qantrip/v1/sas" || vault_id || head || pk_N || pk_E || n_N || n_E))`，
/// 显示为 8 个 Base32 字符，中间加连字符（如 `K7QM-2XRA`）。
pub fn sas(vault_id: &str, head: &[u8; 32], pk_n: &[u8; 96], pk_e: &[u8; 96], n_n: &[u8; 32], n_e: &[u8; 32]) -> String {
    short_code(&sha256(&[D_SAS, vault_id.as_bytes(), head, pk_n, pk_e, n_n, n_e]))
}

/// 截取哈希的前 40 位，显示为 `XXXX-XXXX`（Base32）。
fn short_code(h: &[u8]) -> String {
    let s = codec::base32(&h[..5]);
    format!("{}-{}", &s[..4], &s[4..])
}

/// 与 SAS 格式相同的随机码：批准设备在 SAS 之外再显示两个，由用户选出与新设备上相同的那个（v0.5）。
pub fn random_code() -> String {
    short_code(&random::<5>())
}

/// 加入结果的签名消息（v0.5）：批准设备用花名册密钥签名，新设备用 reply 中锁定的 pk_E 验证。
///
/// `msg = "qantrip/v2/join-result" || vault_id || SHA-256(commit) || SHA-256(reply) || SHA-256(reveal) || SHA-256(body)`
///
/// 签名把结果（花名册新条目、信封、状态记录锚点）绑定到这一次加入的完整消息上；用户在两台设备上
/// 比对的 SAS 已经认证了 pk_E。
pub fn join_result_message(vault_id: &str, transcript: [&[u8]; 3], body: &[u8]) -> Vec<u8> {
    let [commit, reply, reveal] = transcript;
    [D_JOIN_RESULT, vault_id.as_bytes(), &sha256(&[commit]), &sha256(&[reply]), &sha256(&[reveal]), &sha256(&[body])].concat()
}

/// 规范化用户输入的校验码：去掉空白与连字符、转大写，并纠正 Base32 中不存在的易混字符（0→O、1→I、8→B）。
pub fn normalize_sas(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            '0' => 'O',
            '1' => 'I',
            '8' => 'B',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_buckets() {
        assert_eq!(padded_len(0), 256);
        assert_eq!(padded_len(1), 256);
        assert_eq!(padded_len(256), 256);
        assert_eq!(padded_len(257), 512);
        assert_eq!(padded_len(4096), 4096);
        assert_eq!(padded_len(4097), 8192);
        assert_eq!(padded_len(70_000), 131_072);
    }

    #[test]
    fn sas_normalization() {
        assert_eq!(normalize_sas(" k7qm-2xra "), "K7QM2XRA");
        assert_eq!(normalize_sas("0180"), "OIBO");
    }

    #[test]
    fn record_rejects_tampering() {
        let vk = [9u8; 32];
        let ct = seal_record(&vk, &[1u8; 24], "v_a", "rec", 1, b"hello");
        assert!(open_record(&vk, "v_a", "rec", 1, &ct).is_ok());
        assert!(open_record(&vk, "v_b", "rec", 1, &ct).is_err());
        assert!(open_record(&vk, "v_a", "rec2", 1, &ct).is_err());
        assert!(open_record(&vk, "v_a", "rec", 2, &ct).is_err());
        let mut bad = ct.clone();
        bad[30] ^= 1;
        assert!(open_record(&vk, "v_a", "rec", 1, &bad).is_err());
        let mut bad = ct;
        bad[0] = 2;
        assert!(open_record(&vk, "v_a", "rec", 1, &bad).is_err());
    }

    #[test]
    fn phrase_round_trip() {
        let e = [42u8; 32];
        for lang in [PhraseLanguage::English, PhraseLanguage::ChineseSimplified] {
            let p = phrase_from_entropy(&e, lang);
            assert_eq!(p.split(' ').count(), 24);
            assert_eq!(*parse_phrase(&p).unwrap(), e);
            assert_eq!(*parse_phrase(&format!("  {}  ", p.to_uppercase())).unwrap(), e);
        }
        let zh = phrase_from_entropy(&e, PhraseLanguage::ChineseSimplified);
        assert_eq!(*parse_phrase(&zh.replace(' ', "")).unwrap(), e);
        assert!(parse_phrase("abandon abandon").is_err());
        let en = phrase_from_entropy(&e, PhraseLanguage::English);
        let mut words: Vec<&str> = en.split(' ').collect();
        words.swap(0, 1);
        assert!(parse_phrase(&words.join(" ")).is_err() || *parse_phrase(&words.join(" ")).unwrap() != e);
    }
}
