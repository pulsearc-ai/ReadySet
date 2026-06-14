//! `ReadySet` encrypted secret bundle support.
//!
//! `ready-set-encrypt` owns `ReadySet`'s encrypted file container format and the
//! encryption used by that format. It deliberately uses standard `RustCrypto`
//! primitives instead of inventing new cryptographic algorithms.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use zeroize::Zeroize;

const FORMAT_ID: &str = "readyset.secret-bundle";
const FORMAT_VERSION: u16 = 1;
const CIPHER_ID: &str = "AES-256-GCM";
const LEGACY_CIPHER_ID: &str = "XCHACHA20-POLY1305";
const LOCAL_KEY_WRAP: &str = "local-key+aes-256-gcm";
const LEGACY_LOCAL_KEY_WRAP: &str = "local-key+xchacha20-poly1305";
const KEY_LEN: usize = 32;
const AES_GCM_NONCE_LEN: usize = 12;
const XCHACHA20_NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const LOCAL_KEY_HEADER: &str = "readyset-secret-bundle-local-key-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
struct AeadCiphertext {
    ciphertext: Vec<u8>,
    tag: Vec<u8>,
}

#[derive(thiserror::Error, Debug, Clone)]
enum CryptoError {
    #[error("invalid key length ({actual} bytes; expected {expected})")]
    InvalidKeyLength { actual: usize, expected: usize },
    #[error("invalid nonce length ({actual} bytes; expected {expected})")]
    InvalidNonceLength { actual: usize, expected: usize },
    #[error("authentication failed (tampered ciphertext or wrong key)")]
    AuthenticationFailed,
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Debug, Clone)]
struct EncryptAead {
    key: Vec<u8>,
    nonce: Vec<u8>,
    plaintext: Vec<u8>,
    aad: Vec<u8>,
}

#[derive(Debug, Clone)]
struct DecryptAead {
    key: Vec<u8>,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
    tag: Vec<u8>,
    aad: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CipherSuite {
    Aes256Gcm,
    XChaCha20Poly1305,
}

impl CipherSuite {
    const fn id(self) -> &'static str {
        match self {
            Self::Aes256Gcm => CIPHER_ID,
            Self::XChaCha20Poly1305 => LEGACY_CIPHER_ID,
        }
    }

    const fn nonce_len(self) -> usize {
        match self {
            Self::Aes256Gcm => AES_GCM_NONCE_LEN,
            Self::XChaCha20Poly1305 => XCHACHA20_NONCE_LEN,
        }
    }
}

/// Result alias for secret bundle operations.
pub type Result<T> = std::result::Result<T, BundleError>;

/// Errors produced while reading, writing, encrypting, or decrypting bundles.
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    /// A filesystem operation failed.
    #[error("{path}: {source}")]
    Io {
        /// Path being accessed.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// A TOML bundle could not be parsed or rendered.
    #[error("TOML error: {0}")]
    Toml(String),
    /// Base64 data in a key or bundle is malformed.
    #[error("base64 decode failed: {0}")]
    Base64(String),
    /// Random bytes could not be generated.
    #[error("random generation failed: {0}")]
    Random(String),
    /// The bundle format is unsupported.
    #[error("unsupported bundle format `{format}` version {version}")]
    UnsupportedFormat {
        /// Format identifier from the file.
        format: String,
        /// Format version from the file.
        version: u16,
    },
    /// The bundle cipher is unsupported.
    #[error("unsupported cipher `{0}`")]
    UnsupportedCipher(String),
    /// No configured local key could decrypt the bundle.
    #[error("no matching local key recipient decrypted the bundle")]
    NoMatchingRecipient,
    /// The local key file is malformed.
    #[error("invalid local key file: {0}")]
    InvalidLocalKey(String),
    /// Authenticated decryption failed.
    #[error("authentication failed")]
    AuthenticationFailed,
    /// Generic crypto failure.
    #[error("crypto error: {0}")]
    Crypto(String),
    /// Dotenv input is malformed.
    #[error("dotenv parse error at line {line}: {message}")]
    Dotenv {
        /// One-based line number.
        line: usize,
        /// Parse diagnostic.
        message: String,
    },
}

impl From<toml::de::Error> for BundleError {
    fn from(value: toml::de::Error) -> Self {
        Self::Toml(value.to_string())
    }
}

impl From<toml::ser::Error> for BundleError {
    fn from(value: toml::ser::Error) -> Self {
        Self::Toml(value.to_string())
    }
}

impl From<base64::DecodeError> for BundleError {
    fn from(value: base64::DecodeError) -> Self {
        Self::Base64(value.to_string())
    }
}

impl From<CryptoError> for BundleError {
    fn from(value: CryptoError) -> Self {
        match value {
            CryptoError::AuthenticationFailed => Self::AuthenticationFailed,
            other => Self::Crypto(other.to_string()),
        }
    }
}

/// Plaintext payload type stored inside a bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PayloadFormat {
    /// A dotenv file where each non-comment line is `NAME=value`.
    Dotenv,
}

impl PayloadFormat {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Dotenv => "dotenv",
        }
    }
}

impl std::str::FromStr for PayloadFormat {
    type Err = BundleError;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "dotenv" => Ok(Self::Dotenv),
            other => Err(BundleError::Toml(format!(
                "unsupported payload format `{other}`"
            ))),
        }
    }
}

/// Options used when encrypting a new secret bundle.
#[derive(Debug, Clone)]
pub struct EncryptOptions {
    /// Payload format.
    pub payload_format: PayloadFormat,
    /// Source path recorded in authenticated metadata.
    pub source_path: Option<String>,
    /// Environment label recorded in authenticated metadata.
    pub environment: Option<String>,
    /// Extra authenticated metadata.
    pub metadata: BTreeMap<String, String>,
}

impl Default for EncryptOptions {
    fn default() -> Self {
        Self {
            payload_format: PayloadFormat::Dotenv,
            source_path: None,
            environment: None,
            metadata: BTreeMap::new(),
        }
    }
}

/// A local symmetric wrapping key.
#[derive(Debug, Clone)]
pub struct LocalKey {
    id: String,
    bytes: Vec<u8>,
}

impl Drop for LocalKey {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl LocalKey {
    /// Create an in-memory local key from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when `bytes` is not 32 bytes.
    pub fn from_bytes(id: impl Into<String>, bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() != KEY_LEN {
            return Err(BundleError::InvalidLocalKey(format!(
                "expected {KEY_LEN} key bytes, got {}",
                bytes.len()
            )));
        }
        Ok(Self {
            id: id.into(),
            bytes,
        })
    }

    /// Local key identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// An encrypted secret bundle document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretBundle {
    /// Format identifier.
    pub format: String,
    /// Format version.
    pub version: u16,
    /// Payload format.
    pub payload_format: PayloadFormat,
    /// Payload AEAD cipher.
    pub cipher: String,
    /// Creation timestamp, RFC3339 UTC.
    pub created_at: String,
    /// Last update timestamp, RFC3339 UTC.
    pub updated_at: String,
    /// Authenticated metadata.
    #[serde(default)]
    pub metadata: BundleMetadata,
    /// Wrapped data-encryption-key recipients.
    pub recipients: Vec<Recipient>,
    /// Encrypted payload.
    pub encrypted: EncryptedPayload,
}

/// Authenticated bundle metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BundleMetadata {
    /// Project-relative plaintext source path.
    pub source_path: Option<String>,
    /// Environment label.
    pub environment: Option<String>,
    /// Additional string metadata.
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

/// A wrapped recipient entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipient {
    /// Recipient kind.
    #[serde(rename = "type")]
    pub recipient_type: String,
    /// Recipient id.
    pub id: String,
    /// Key wrap algorithm.
    pub key_wrap: String,
    /// Base64-encoded key wrap nonce.
    pub nonce: String,
    /// Base64-encoded wrapped data encryption key.
    pub wrapped_dek: String,
    /// Base64-encoded AEAD tag for the wrapped key.
    pub tag: String,
}

/// Encrypted payload bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedPayload {
    /// Base64-encoded payload nonce.
    pub nonce: String,
    /// Base64-encoded payload ciphertext.
    pub ciphertext: String,
    /// Base64-encoded payload AEAD tag.
    pub tag: String,
}

/// Create and persist a local key file.
///
/// # Errors
///
/// Returns an error when random generation or writing the key file fails.
pub fn create_local_key_file(path: &Path, id: &str) -> Result<LocalKey> {
    let key = create_local_key(id)?;
    write_local_key_file(path, &key)?;
    Ok(key)
}

/// Create an in-memory local key.
///
/// # Errors
///
/// Returns an error when random generation fails.
pub fn create_local_key(id: &str) -> Result<LocalKey> {
    let mut bytes = vec![0_u8; KEY_LEN];
    fill_random(&mut bytes)?;
    LocalKey::from_bytes(id, bytes)
}

/// Render a local key as the file format accepted by [`parse_local_key_text`].
#[must_use]
pub fn local_key_file_text(key: &LocalKey) -> String {
    format!(
        "{LOCAL_KEY_HEADER}\nid={}\nkey={}\n",
        key.id,
        STANDARD.encode(&key.bytes)
    )
}

/// Render a local key as a single-line token for environment variables.
#[must_use]
pub fn local_key_token(key: &LocalKey) -> String {
    format!(
        "{LOCAL_KEY_HEADER}:{}:{}",
        key.id,
        STANDARD.encode(&key.bytes)
    )
}

/// Load a local key file.
///
/// # Errors
///
/// Returns an error when the file cannot be read or has invalid syntax.
pub fn load_local_key_file(path: &Path) -> Result<LocalKey> {
    let raw = std::fs::read_to_string(path).map_err(|source| BundleError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_local_key_text(&raw)
}

/// Parse a local key from either the multi-line key file format or the
/// single-line token emitted by `ready-set-encrypt key generate`.
///
/// # Errors
///
/// Returns an error when the key text is malformed.
pub fn parse_local_key_text(raw: &str) -> Result<LocalKey> {
    let trimmed = raw.trim();
    if trimmed.starts_with(LOCAL_KEY_HEADER) && trimmed.contains('\n') {
        return parse_local_key(trimmed);
    }
    parse_local_key_token(trimmed)
}

/// Write a local key file.
///
/// # Errors
///
/// Returns an error when the destination cannot be written.
pub fn write_local_key_file(path: &Path, key: &LocalKey) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| BundleError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let content = local_key_file_text(key);
    std::fs::write(path, content).map_err(|source| BundleError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    restrict_user_only(path)?;
    Ok(())
}

/// Encrypt a plaintext payload for one or more local recipients.
///
/// # Errors
///
/// Returns an error when random generation or encryption fails.
pub fn encrypt(
    plaintext: &[u8],
    options: &EncryptOptions,
    keys: &[LocalKey],
) -> Result<SecretBundle> {
    if keys.is_empty() {
        return Err(BundleError::NoMatchingRecipient);
    }

    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|err| BundleError::Toml(err.to_string()))?;
    let mut dek = vec![0_u8; KEY_LEN];
    fill_random(&mut dek)?;

    let metadata = BundleMetadata {
        source_path: options.source_path.clone(),
        environment: options.environment.clone(),
        extra: options.metadata.clone(),
    };
    let aad = payload_aad(CIPHER_ID, options.payload_format, &metadata);
    let sealed = seal(CipherSuite::Aes256Gcm, &dek, plaintext, &aad)?;

    let mut recipients = Vec::with_capacity(keys.len());
    for key in keys {
        recipients.push(wrap_dek(key, &dek)?);
    }
    dek.zeroize();

    Ok(SecretBundle {
        format: FORMAT_ID.into(),
        version: FORMAT_VERSION,
        payload_format: options.payload_format,
        cipher: CIPHER_ID.into(),
        created_at: now.clone(),
        updated_at: now,
        metadata,
        recipients,
        encrypted: EncryptedPayload {
            nonce: STANDARD.encode(sealed.nonce),
            ciphertext: STANDARD.encode(sealed.ciphertext.ciphertext),
            tag: STANDARD.encode(sealed.ciphertext.tag),
        },
    })
}

/// Decrypt a bundle using one of the provided local keys.
///
/// # Errors
///
/// Returns an error when no key can unwrap the payload key or when the payload
/// authentication tag fails.
pub fn decrypt(bundle: &SecretBundle, keys: &[LocalKey]) -> Result<Vec<u8>> {
    validate_bundle(bundle)?;
    let payload_cipher = cipher_from_id(&bundle.cipher)?;
    for recipient in &bundle.recipients {
        for key in keys {
            if recipient.recipient_type != "local-key" || recipient.id != key.id {
                continue;
            }
            let Ok(mut dek) = unwrap_dek(key, recipient) else {
                continue;
            };
            let aad = payload_aad(payload_cipher.id(), bundle.payload_format, &bundle.metadata);
            let result = open(
                payload_cipher,
                &dek,
                &decode(&bundle.encrypted.nonce)?,
                &decode(&bundle.encrypted.ciphertext)?,
                &decode(&bundle.encrypted.tag)?,
                &aad,
            );
            dek.zeroize();
            return result;
        }
    }
    Err(BundleError::NoMatchingRecipient)
}

/// Read a bundle file from disk.
///
/// # Errors
///
/// Returns an error when the file cannot be read or parsed.
pub fn read_bundle_file(path: &Path) -> Result<SecretBundle> {
    let raw = std::fs::read_to_string(path).map_err(|source| BundleError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let bundle = toml::from_str(&raw)?;
    validate_bundle(&bundle)?;
    Ok(bundle)
}

/// Write a bundle file to disk.
///
/// # Errors
///
/// Returns an error when serialization or writing fails.
pub fn write_bundle_file(path: &Path, bundle: &SecretBundle) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| BundleError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let raw = toml::to_string_pretty(bundle)?;
    std::fs::write(path, raw).map_err(|source| BundleError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Return whether a bundle is already using the current write-side crypto.
#[must_use]
pub fn uses_current_crypto(bundle: &SecretBundle) -> bool {
    bundle.cipher == CIPHER_ID
        && bundle
            .recipients
            .iter()
            .all(|recipient| recipient.key_wrap == LOCAL_KEY_WRAP)
}

/// Extract dotenv key names from plaintext.
///
/// # Errors
///
/// Returns an error when the dotenv content has invalid assignment syntax.
pub fn dotenv_keys(input: &str) -> Result<Vec<String>> {
    let mut keys = Vec::new();
    for (idx, line) in input.lines().enumerate() {
        let Some((key, _value)) = parse_dotenv_line(idx + 1, line)? else {
            continue;
        };
        keys.push(key);
    }
    keys.sort();
    keys.dedup();
    Ok(keys)
}

/// Parse dotenv plaintext into environment variables.
///
/// # Errors
///
/// Returns an error when the dotenv content has invalid assignment syntax.
pub fn parse_dotenv(input: &str) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (idx, line) in input.lines().enumerate() {
        let Some((key, value)) = parse_dotenv_line(idx + 1, line)? else {
            continue;
        };
        out.insert(key, value);
    }
    Ok(out)
}

/// Return a stable content fingerprint for diagnostics.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

fn validate_bundle(bundle: &SecretBundle) -> Result<()> {
    if bundle.format != FORMAT_ID || bundle.version != FORMAT_VERSION {
        return Err(BundleError::UnsupportedFormat {
            format: bundle.format.clone(),
            version: bundle.version,
        });
    }
    cipher_from_id(&bundle.cipher)?;
    Ok(())
}

fn cipher_from_id(cipher_id: &str) -> Result<CipherSuite> {
    match cipher_id {
        CIPHER_ID => Ok(CipherSuite::Aes256Gcm),
        LEGACY_CIPHER_ID => Ok(CipherSuite::XChaCha20Poly1305),
        other => Err(BundleError::UnsupportedCipher(other.to_owned())),
    }
}

fn cipher_from_key_wrap(key_wrap: &str) -> Result<CipherSuite> {
    match key_wrap {
        LOCAL_KEY_WRAP => Ok(CipherSuite::Aes256Gcm),
        LEGACY_LOCAL_KEY_WRAP => Ok(CipherSuite::XChaCha20Poly1305),
        other => Err(BundleError::UnsupportedCipher(other.to_owned())),
    }
}

fn parse_local_key(raw: &str) -> Result<LocalKey> {
    let mut lines = raw.lines();
    match lines.next() {
        Some(header) if header == LOCAL_KEY_HEADER => {},
        _ => {
            return Err(BundleError::InvalidLocalKey(
                "missing readyset local key header".into(),
            ));
        },
    }
    let mut id = None;
    let mut key = None;
    for line in lines {
        if let Some(value) = line.strip_prefix("id=") {
            id = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("key=") {
            key = Some(decode(value)?);
        } else if !line.trim().is_empty() {
            return Err(BundleError::InvalidLocalKey(format!(
                "unexpected line `{line}`"
            )));
        }
    }
    LocalKey::from_bytes(
        id.ok_or_else(|| BundleError::InvalidLocalKey("missing id".into()))?,
        key.ok_or_else(|| BundleError::InvalidLocalKey("missing key".into()))?,
    )
}

fn parse_local_key_token(raw: &str) -> Result<LocalKey> {
    let mut parts = raw.splitn(3, ':');
    let header = parts.next();
    let id = parts.next();
    let key = parts.next();
    match (header, id, key) {
        (Some(LOCAL_KEY_HEADER), Some(id), Some(key)) if !id.is_empty() && !key.is_empty() => {
            LocalKey::from_bytes(id.to_owned(), decode(key)?)
        },
        _ => Err(BundleError::InvalidLocalKey(
            "expected readyset-secret-bundle-local-key-v1:<id>:<base64-key>".into(),
        )),
    }
}

struct Sealed {
    nonce: Vec<u8>,
    ciphertext: AeadCiphertext,
}

fn wrap_dek(key: &LocalKey, dek: &[u8]) -> Result<Recipient> {
    let aad = wrap_aad(&key.id, LOCAL_KEY_WRAP);
    let sealed = seal(CipherSuite::Aes256Gcm, &key.bytes, dek, &aad)?;
    Ok(Recipient {
        recipient_type: "local-key".into(),
        id: key.id.clone(),
        key_wrap: LOCAL_KEY_WRAP.into(),
        nonce: STANDARD.encode(sealed.nonce),
        wrapped_dek: STANDARD.encode(sealed.ciphertext.ciphertext),
        tag: STANDARD.encode(sealed.ciphertext.tag),
    })
}

fn unwrap_dek(key: &LocalKey, recipient: &Recipient) -> Result<Vec<u8>> {
    let cipher = cipher_from_key_wrap(&recipient.key_wrap)?;
    let aad = wrap_aad(&key.id, &recipient.key_wrap);
    open(
        cipher,
        &key.bytes,
        &decode(&recipient.nonce)?,
        &decode(&recipient.wrapped_dek)?,
        &decode(&recipient.tag)?,
        &aad,
    )
}

fn seal(cipher: CipherSuite, key: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Sealed> {
    let mut nonce = vec![0_u8; cipher.nonce_len()];
    fill_random(&mut nonce)?;
    let input = EncryptAead {
        key: key.to_vec(),
        nonce: nonce.clone(),
        plaintext: plaintext.to_vec(),
        aad: aad.to_vec(),
    };
    let ciphertext = match cipher {
        CipherSuite::Aes256Gcm => encrypt_aes_256_gcm(&input),
        CipherSuite::XChaCha20Poly1305 => encrypt_chacha20_poly1305(&input),
    }?;
    Ok(Sealed { nonce, ciphertext })
}

fn open(
    cipher: CipherSuite,
    key: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
    tag: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>> {
    let input = DecryptAead {
        key: key.to_vec(),
        nonce: nonce.to_vec(),
        ciphertext: ciphertext.to_vec(),
        tag: tag.to_vec(),
        aad: aad.to_vec(),
    };
    match cipher {
        CipherSuite::Aes256Gcm => decrypt_aes_256_gcm(input),
        CipherSuite::XChaCha20Poly1305 => decrypt_chacha20_poly1305(input),
    }
    .map_err(Into::into)
}

fn encrypt_aes_256_gcm(input: &EncryptAead) -> std::result::Result<AeadCiphertext, CryptoError> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};

    if input.key.len() != KEY_LEN {
        return Err(CryptoError::InvalidKeyLength {
            actual: input.key.len(),
            expected: KEY_LEN,
        });
    }
    if input.nonce.len() != AES_GCM_NONCE_LEN {
        return Err(CryptoError::InvalidNonceLength {
            actual: input.nonce.len(),
            expected: AES_GCM_NONCE_LEN,
        });
    }
    let cipher =
        Aes256Gcm::new_from_slice(&input.key).map_err(|_| CryptoError::InvalidKeyLength {
            actual: input.key.len(),
            expected: KEY_LEN,
        })?;
    let sealed = cipher
        .encrypt(
            Nonce::from_slice(&input.nonce),
            Payload {
                msg: &input.plaintext,
                aad: &input.aad,
            },
        )
        .map_err(|_| CryptoError::AuthenticationFailed)?;
    split_tag(sealed)
}

fn decrypt_aes_256_gcm(input: DecryptAead) -> std::result::Result<Vec<u8>, CryptoError> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};

    if input.key.len() != KEY_LEN {
        return Err(CryptoError::InvalidKeyLength {
            actual: input.key.len(),
            expected: KEY_LEN,
        });
    }
    if input.nonce.len() != AES_GCM_NONCE_LEN {
        return Err(CryptoError::InvalidNonceLength {
            actual: input.nonce.len(),
            expected: AES_GCM_NONCE_LEN,
        });
    }
    if input.tag.len() != TAG_LEN {
        return Err(CryptoError::Internal(format!(
            "AES-GCM tag must be {TAG_LEN} bytes"
        )));
    }
    let cipher =
        Aes256Gcm::new_from_slice(&input.key).map_err(|_| CryptoError::InvalidKeyLength {
            actual: input.key.len(),
            expected: KEY_LEN,
        })?;
    let mut sealed = input.ciphertext;
    sealed.extend_from_slice(&input.tag);
    cipher
        .decrypt(
            Nonce::from_slice(&input.nonce),
            Payload {
                msg: &sealed,
                aad: &input.aad,
            },
        )
        .map_err(|_| CryptoError::AuthenticationFailed)
}

fn encrypt_chacha20_poly1305(
    input: &EncryptAead,
) -> std::result::Result<AeadCiphertext, CryptoError> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

    if input.key.len() != KEY_LEN {
        return Err(CryptoError::InvalidKeyLength {
            actual: input.key.len(),
            expected: KEY_LEN,
        });
    }
    if input.nonce.len() != XCHACHA20_NONCE_LEN {
        return Err(CryptoError::InvalidNonceLength {
            actual: input.nonce.len(),
            expected: XCHACHA20_NONCE_LEN,
        });
    }
    let cipher = XChaCha20Poly1305::new(Key::from_slice(&input.key));
    let sealed = cipher
        .encrypt(
            XNonce::from_slice(&input.nonce),
            Payload {
                msg: &input.plaintext,
                aad: &input.aad,
            },
        )
        .map_err(|_| CryptoError::AuthenticationFailed)?;
    split_tag(sealed)
}

fn decrypt_chacha20_poly1305(input: DecryptAead) -> std::result::Result<Vec<u8>, CryptoError> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

    if input.key.len() != KEY_LEN {
        return Err(CryptoError::InvalidKeyLength {
            actual: input.key.len(),
            expected: KEY_LEN,
        });
    }
    if input.nonce.len() != XCHACHA20_NONCE_LEN {
        return Err(CryptoError::InvalidNonceLength {
            actual: input.nonce.len(),
            expected: XCHACHA20_NONCE_LEN,
        });
    }
    if input.tag.len() != TAG_LEN {
        return Err(CryptoError::Internal(format!(
            "ChaCha20-Poly1305 tag must be {TAG_LEN} bytes"
        )));
    }
    let cipher = XChaCha20Poly1305::new(Key::from_slice(&input.key));
    let mut sealed = input.ciphertext;
    sealed.extend_from_slice(&input.tag);
    cipher
        .decrypt(
            XNonce::from_slice(&input.nonce),
            Payload {
                msg: &sealed,
                aad: &input.aad,
            },
        )
        .map_err(|_| CryptoError::AuthenticationFailed)
}

fn split_tag(mut sealed: Vec<u8>) -> std::result::Result<AeadCiphertext, CryptoError> {
    if sealed.len() < TAG_LEN {
        return Err(CryptoError::Internal("ciphertext missing AEAD tag".into()));
    }
    let tag = sealed.split_off(sealed.len() - TAG_LEN);
    Ok(AeadCiphertext {
        ciphertext: sealed,
        tag,
    })
}

fn payload_aad(
    cipher_id: &str,
    payload_format: PayloadFormat,
    metadata: &BundleMetadata,
) -> Vec<u8> {
    let mut aad = format!(
        "format={FORMAT_ID}\nversion={FORMAT_VERSION}\npayload_format={}\ncipher={cipher_id}\n",
        payload_format.as_str()
    );
    if let Some(source_path) = &metadata.source_path {
        aad.push_str("source_path=");
        aad.push_str(source_path);
        aad.push('\n');
    }
    if let Some(environment) = &metadata.environment {
        aad.push_str("environment=");
        aad.push_str(environment);
        aad.push('\n');
    }
    for (key, value) in &metadata.extra {
        aad.push_str("meta.");
        aad.push_str(key);
        aad.push('=');
        aad.push_str(value);
        aad.push('\n');
    }
    aad.into_bytes()
}

fn wrap_aad(id: &str, key_wrap: &str) -> Vec<u8> {
    format!("format={FORMAT_ID}\nversion={FORMAT_VERSION}\nrecipient={id}\nwrap={key_wrap}\n")
        .into_bytes()
}

fn decode(input: &str) -> Result<Vec<u8>> {
    STANDARD.decode(input).map_err(Into::into)
}

fn fill_random(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf).map_err(|err| BundleError::Random(err.to_string()))
}

fn parse_dotenv_line(line_number: usize, line: &str) -> Result<Option<(String, String)>> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Ok(None);
    }
    let assignment = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let Some((raw_key, raw_value)) = assignment.split_once('=') else {
        return Err(BundleError::Dotenv {
            line: line_number,
            message: "expected KEY=value".into(),
        });
    };
    let key = raw_key.trim();
    if !is_env_key(key) {
        return Err(BundleError::Dotenv {
            line: line_number,
            message: format!("invalid key `{key}`"),
        });
    }
    Ok(Some((key.to_owned(), parse_dotenv_value(raw_value.trim()))))
}

fn parse_dotenv_value(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        return value[1..value.len() - 1]
            .replace("\\n", "\n")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
    }
    if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
        return value[1..value.len() - 1].to_owned();
    }
    value.to_owned()
}

fn is_env_key(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_uppercase() || first.is_ascii_lowercase() => {
        },
        _ => return false,
    }
    chars.all(|ch| {
        ch == '_' || ch.is_ascii_uppercase() || ch.is_ascii_lowercase() || ch.is_ascii_digit()
    })
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[(byte >> 4) as usize]));
        out.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    out
}

#[cfg(unix)]
fn restrict_user_only(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, permissions).map_err(|source| BundleError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn restrict_user_only(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: &str, byte: u8) -> LocalKey {
        LocalKey::from_bytes(id, vec![byte; KEY_LEN]).unwrap()
    }

    #[test]
    fn bundle_round_trips_dotenv_payload() {
        let local = key("test", 7);
        let opts = EncryptOptions {
            source_path: Some(".env".into()),
            environment: Some("test".into()),
            ..EncryptOptions::default()
        };
        let plaintext = b"API_KEY=secret\nAPP_ENV=test\n";
        let bundle = encrypt(plaintext, &opts, std::slice::from_ref(&local)).unwrap();
        assert_eq!(bundle.cipher, CIPHER_ID);
        assert!(uses_current_crypto(&bundle));
        assert_eq!(
            decode(&bundle.encrypted.nonce).unwrap().len(),
            AES_GCM_NONCE_LEN
        );
        assert_eq!(bundle.recipients[0].key_wrap, LOCAL_KEY_WRAP);
        let decrypted = decrypt(&bundle, &[local]).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn legacy_xchacha_bundle_decrypts_for_migration() {
        let local = key("legacy", 7);
        let dek = vec![9; KEY_LEN];
        let metadata = BundleMetadata {
            source_path: Some(".env".into()),
            environment: Some("legacy".into()),
            extra: BTreeMap::default(),
        };
        let plaintext = b"API_KEY=legacy\n";
        let aad = payload_aad(LEGACY_CIPHER_ID, PayloadFormat::Dotenv, &metadata);
        let sealed_payload = seal(CipherSuite::XChaCha20Poly1305, &dek, plaintext, &aad).unwrap();
        let wrap_aad = wrap_aad(local.id(), LEGACY_LOCAL_KEY_WRAP);
        let sealed_dek = seal(
            CipherSuite::XChaCha20Poly1305,
            &local.bytes,
            &dek,
            &wrap_aad,
        )
        .unwrap();
        let bundle = SecretBundle {
            format: FORMAT_ID.into(),
            version: FORMAT_VERSION,
            payload_format: PayloadFormat::Dotenv,
            cipher: LEGACY_CIPHER_ID.into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            metadata,
            recipients: vec![Recipient {
                recipient_type: "local-key".into(),
                id: local.id().into(),
                key_wrap: LEGACY_LOCAL_KEY_WRAP.into(),
                nonce: STANDARD.encode(sealed_dek.nonce),
                wrapped_dek: STANDARD.encode(sealed_dek.ciphertext.ciphertext),
                tag: STANDARD.encode(sealed_dek.ciphertext.tag),
            }],
            encrypted: EncryptedPayload {
                nonce: STANDARD.encode(sealed_payload.nonce),
                ciphertext: STANDARD.encode(sealed_payload.ciphertext.ciphertext),
                tag: STANDARD.encode(sealed_payload.ciphertext.tag),
            },
        };

        assert!(!uses_current_crypto(&bundle));
        let decrypted = decrypt(&bundle, &[local]).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn tampered_ciphertext_does_not_decrypt() {
        let local = key("test", 7);
        let mut bundle = encrypt(
            b"API_KEY=secret\n",
            &EncryptOptions::default(),
            std::slice::from_ref(&local),
        )
        .unwrap();
        bundle.encrypted.ciphertext.push('A');
        let err = decrypt(&bundle, &[local]).unwrap_err();
        assert!(matches!(
            err,
            BundleError::AuthenticationFailed | BundleError::Base64(_)
        ));
    }

    #[test]
    fn wrong_key_does_not_decrypt() {
        let local = key("test", 7);
        let wrong = key("test", 8);
        let bundle = encrypt(b"API_KEY=secret\n", &EncryptOptions::default(), &[local]).unwrap();
        let err = decrypt(&bundle, &[wrong]).unwrap_err();
        assert!(matches!(err, BundleError::NoMatchingRecipient));
    }

    #[test]
    fn local_key_file_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bundle.key");
        let key = create_local_key_file(&path, "dev").unwrap();
        let loaded = load_local_key_file(&path).unwrap();
        assert_eq!(loaded.id(), key.id());
        assert_eq!(loaded.bytes, key.bytes);
    }

    #[test]
    fn dotenv_parser_handles_basic_values() {
        let parsed = parse_dotenv("A=1\nexport B=\"two\\nlines\"\n# nope\nC='three'\n").unwrap();
        assert_eq!(parsed.get("A").unwrap(), "1");
        assert_eq!(parsed.get("B").unwrap(), "two\nlines");
        assert_eq!(parsed.get("C").unwrap(), "three");
        assert_eq!(dotenv_keys("A=1\nB=2\nA=3\n").unwrap(), vec!["A", "B"]);
    }
}
