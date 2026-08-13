use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use rusqlite::Connection;
use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const ENV_TOKEN_KEY: &str = "CODEXMANAGER_TOKEN_ENCRYPTION_KEY";
const ENV_TOKEN_KEY_FILE: &str = "CODEXMANAGER_TOKEN_ENCRYPTION_KEY_FILE";
const KEYRING_SERVICE: &str = "CodexManager";
const KEYRING_USER: &str = "account-token-encryption-v1";
const ENCRYPTED_PREFIX: &str = "cmenc:v1:";
const DEFAULT_KEY_FILENAME: &str = "codexmanager.token-key";
const KEY_LEN: usize = 32;

#[derive(Debug)]
pub(super) struct TokenCryptoError {
    message: String,
}

impl TokenCryptoError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for TokenCryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for TokenCryptoError {}

pub(super) fn to_sql_error(error: TokenCryptoError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

#[derive(Debug)]
pub(super) struct TokenCipher {
    key: LessSafeKey,
}

impl TokenCipher {
    pub(super) fn for_database(
        database_path: &Path,
        conn: &Connection,
    ) -> Result<Self, TokenCryptoError> {
        let has_ciphertext = database_has_ciphertext(conn)?;
        let key = load_database_key(database_path, has_ciphertext)?;
        Self::from_key(key)
    }

    pub(super) fn ephemeral() -> Result<Self, TokenCryptoError> {
        Self::from_key(generate_key()?)
    }

    fn from_key(mut key: [u8; KEY_LEN]) -> Result<Self, TokenCryptoError> {
        let unbound_key = UnboundKey::new(&AES_256_GCM, &key);
        key.fill(0);
        let key = unbound_key
            .map_err(|_| TokenCryptoError::new("invalid account token encryption key"))?;
        Ok(Self {
            key: LessSafeKey::new(key),
        })
    }

    pub(super) fn encrypt(
        &self,
        account_id: &str,
        field: &str,
        plaintext: &str,
    ) -> Result<String, TokenCryptoError> {
        if plaintext.trim().is_empty() {
            return Ok(plaintext.to_string());
        }
        if plaintext.starts_with(ENCRYPTED_PREFIX) {
            self.decrypt(account_id, field, plaintext)?;
            return Ok(plaintext.to_string());
        }
        if plaintext.starts_with("cmenc:") {
            return Err(TokenCryptoError::new(
                "unsupported account token ciphertext version",
            ));
        }

        let mut nonce_bytes = [0_u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce_bytes)
            .map_err(|_| TokenCryptoError::new("failed to generate account token nonce"))?;
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut ciphertext = plaintext.as_bytes().to_vec();
        self.key
            .seal_in_place_append_tag(nonce, Aad::from(aad(account_id, field)), &mut ciphertext)
            .map_err(|_| TokenCryptoError::new("failed to encrypt account token"))?;

        let mut payload = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        payload.extend_from_slice(&nonce_bytes);
        payload.extend_from_slice(&ciphertext);
        Ok(format!(
            "{ENCRYPTED_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(payload)
        ))
    }

    pub(super) fn decrypt(
        &self,
        account_id: &str,
        field: &str,
        stored: &str,
    ) -> Result<String, TokenCryptoError> {
        if stored.trim().is_empty() || !stored.starts_with("cmenc:") {
            return Ok(stored.to_string());
        }
        let encoded = stored.strip_prefix(ENCRYPTED_PREFIX).ok_or_else(|| {
            TokenCryptoError::new("unsupported account token ciphertext version")
        })?;
        let payload = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| TokenCryptoError::new("invalid account token ciphertext encoding"))?;
        if payload.len() <= NONCE_LEN {
            return Err(TokenCryptoError::new(
                "account token ciphertext is truncated",
            ));
        }

        let mut nonce_bytes = [0_u8; NONCE_LEN];
        nonce_bytes.copy_from_slice(&payload[..NONCE_LEN]);
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut ciphertext = payload[NONCE_LEN..].to_vec();
        let plaintext_len = self
            .key
            .open_in_place(nonce, Aad::from(aad(account_id, field)), &mut ciphertext)
            .map_err(|_| {
                TokenCryptoError::new(
                    "account token decryption failed; restore the original encryption key",
                )
            })?
            .len();
        ciphertext.truncate(plaintext_len);
        String::from_utf8(ciphertext)
            .map_err(|_| TokenCryptoError::new("decrypted account token is not valid UTF-8"))
    }
}

fn aad(account_id: &str, field: &str) -> Vec<u8> {
    format!("codexmanager:account-token:v1\0{account_id}\0{field}").into_bytes()
}

fn generate_key() -> Result<[u8; KEY_LEN], TokenCryptoError> {
    let mut key = [0_u8; KEY_LEN];
    SystemRandom::new()
        .fill(&mut key)
        .map_err(|_| TokenCryptoError::new("failed to generate account token encryption key"))?;
    Ok(key)
}

fn load_database_key(
    database_path: &Path,
    has_ciphertext: bool,
) -> Result<[u8; KEY_LEN], TokenCryptoError> {
    if let Some(value) = std::env::var_os(ENV_TOKEN_KEY) {
        let value = value.to_string_lossy();
        return decode_key(value.trim(), ENV_TOKEN_KEY);
    }

    if let Some(path) = std::env::var_os(ENV_TOKEN_KEY_FILE) {
        return read_key_file(Path::new(&path));
    }

    let fallback_path = default_key_file_path(database_path);
    if fallback_path.is_file() {
        return read_key_file(&fallback_path);
    }

    if let Some(key) = load_or_create_platform_key(has_ciphertext)? {
        return Ok(key);
    }

    if has_ciphertext {
        return Err(TokenCryptoError::new(
            "account token encryption key is unavailable; restore the OS credential or set CODEXMANAGER_TOKEN_ENCRYPTION_KEY_FILE",
        ));
    }
    create_or_read_key_file(&fallback_path)
}

pub(super) fn default_key_file_path(database_path: &Path) -> PathBuf {
    database_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(DEFAULT_KEY_FILENAME)
}

fn decode_key(value: &str, source: &str) -> Result<[u8; KEY_LEN], TokenCryptoError> {
    let mut decoded = STANDARD
        .decode(value)
        .or_else(|_| URL_SAFE_NO_PAD.decode(value))
        .map_err(|_| {
            TokenCryptoError::new(format!(
                "{source} must contain a base64-encoded 32-byte key"
            ))
        })?;
    let result = key_from_bytes(&decoded, source);
    decoded.fill(0);
    result
}

fn key_from_bytes(bytes: &[u8], source: &str) -> Result<[u8; KEY_LEN], TokenCryptoError> {
    bytes.try_into().map_err(|_| {
        TokenCryptoError::new(format!(
            "{source} must contain exactly {KEY_LEN} key bytes"
        ))
    })
}

fn read_key_file(path: &Path) -> Result<[u8; KEY_LEN], TokenCryptoError> {
    let bytes = fs::read(path).map_err(|err| {
        TokenCryptoError::new(format!(
            "failed to read account token encryption key file {}: {err}",
            path.display()
        ))
    })?;
    if bytes.len() == KEY_LEN {
        return key_from_bytes(&bytes, "account token encryption key file");
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        TokenCryptoError::new("account token encryption key file is neither raw bytes nor base64")
    })?;
    decode_key(text.trim(), "account token encryption key file")
}

fn create_or_read_key_file(path: &Path) -> Result<[u8; KEY_LEN], TokenCryptoError> {
    let mut key = generate_key()?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    match options.open(path) {
        Ok(mut file) => {
            if let Err(err) = file.write_all(&key).and_then(|_| file.sync_all()) {
                key.fill(0);
                drop(file);
                let _ = fs::remove_file(path);
                return Err(TokenCryptoError::new(format!(
                    "failed to write account token encryption key file {}: {err}",
                    path.display()
                )));
            }
            Ok(key)
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            key.fill(0);
            read_key_file(path)
        }
        Err(err) => {
            key.fill(0);
            Err(TokenCryptoError::new(format!(
                "failed to create account token encryption key file {}: {err}",
                path.display()
            )))
        }
    }
}

fn database_has_ciphertext(conn: &Connection) -> Result<bool, TokenCryptoError> {
    let has_tokens_table = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'tokens')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|err| TokenCryptoError::new(format!("failed to inspect token storage: {err}")))?;
    if !has_tokens_table {
        return Ok(false);
    }

    let has_api_key_column = conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM pragma_table_info('tokens')
                WHERE name = 'api_key_access_token'
            )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|err| {
            TokenCryptoError::new(format!("failed to inspect token columns: {err}"))
        })?;
    let sql = if has_api_key_column {
        "SELECT EXISTS(
            SELECT 1 FROM tokens
            WHERE id_token LIKE 'cmenc:%'
               OR access_token LIKE 'cmenc:%'
               OR refresh_token LIKE 'cmenc:%'
               OR api_key_access_token LIKE 'cmenc:%'
        )"
    } else {
        "SELECT EXISTS(
            SELECT 1 FROM tokens
            WHERE id_token LIKE 'cmenc:%'
               OR access_token LIKE 'cmenc:%'
               OR refresh_token LIKE 'cmenc:%'
        )"
    };

    conn.query_row(
        sql,
        [],
        |row| row.get::<_, bool>(0),
    )
    .map_err(|err| TokenCryptoError::new(format!("failed to inspect encrypted tokens: {err}")))
}

#[cfg(any(
    target_os = "macos",
    target_os = "windows"
))]
fn load_or_create_platform_key(
    has_ciphertext: bool,
) -> Result<Option<[u8; KEY_LEN]>, TokenCryptoError> {
    let entry = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        Ok(entry) => entry,
        Err(_) if !has_ciphertext => return Ok(None),
        Err(err) => {
            return Err(TokenCryptoError::new(format!(
                "failed to open OS credential store for encrypted account tokens: {err}"
            )))
        }
    };

    match entry.get_secret() {
        Ok(mut secret) => {
            let result = key_from_bytes(&secret, "OS credential store entry").map(Some);
            secret.fill(0);
            result
        }
        Err(keyring::Error::NoEntry) if has_ciphertext => Err(TokenCryptoError::new(
            "OS credential store no longer contains the account token encryption key",
        )),
        Err(keyring::Error::NoEntry) => {
            let mut key = generate_key()?;
            match entry.set_secret(&key) {
                Ok(()) => Ok(Some(key)),
                Err(_) => {
                    key.fill(0);
                    Ok(None)
                }
            }
        }
        Err(err) if has_ciphertext => Err(TokenCryptoError::new(format!(
            "failed to read OS credential store for encrypted account tokens: {err}"
        ))),
        Err(_) => Ok(None),
    }
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "windows"
)))]
fn load_or_create_platform_key(
    _has_ciphertext: bool,
) -> Result<Option<[u8; KEY_LEN]>, TokenCryptoError> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_binds_ciphertext_to_account_and_field() {
        let cipher = TokenCipher::ephemeral().expect("cipher");
        let encrypted = cipher
            .encrypt("account-a", "access_token", "secret-token")
            .expect("encrypt");

        assert!(encrypted.starts_with(ENCRYPTED_PREFIX));
        assert_ne!(encrypted, "secret-token");
        assert_eq!(
            cipher
                .decrypt("account-a", "access_token", &encrypted)
                .expect("decrypt"),
            "secret-token"
        );
        assert!(cipher
            .decrypt("account-b", "access_token", &encrypted)
            .is_err());
        assert!(cipher
            .decrypt("account-a", "refresh_token", &encrypted)
            .is_err());
    }

    #[test]
    fn empty_values_remain_empty_for_sql_presence_checks() {
        let cipher = TokenCipher::ephemeral().expect("cipher");
        assert_eq!(
            cipher.encrypt("account-a", "access_token", "  ").unwrap(),
            "  "
        );
    }
}
