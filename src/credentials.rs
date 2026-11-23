//! Secure local persistence for Sign in with ChatGPT (SIWC) credentials,
//! host registration identity, and Google Gemini API keys.
//!
//! - `ext_agent_host_id` (`urn:uuid:...`) is generated once per machine/install
//!   and persisted locally across restarts, sign-outs, and account switches.
//! - OAuth tokens (`access_token`, `refresh_token`, `id_token`) and the user's
//!   Gemini API key are kept strictly on the user's device — never written to
//!   SQLite, logs, or any remote service.
//! - Secrets are stored in the OS credential store (Windows Credential Manager,
//!   macOS Keychain, Linux Secret Service) split into individual entries so
//!   Windows's 2,560-byte blob limit is never exceeded, with an atomic
//!   owner-only (`0600` on Unix) local file fallback when the OS keychain is
//!   unavailable or rejects a secret.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::contracts::{CHATGPT_PLAN_USAGE_SCOPE, ChatGptAuthStatus};

const KEYRING_SERVICE: &str = "vanguardcv";
const KEYRING_ACCESS_TOKEN: &str = "chatgpt_siwc_access_token";
const KEYRING_REFRESH_TOKEN: &str = "chatgpt_siwc_refresh_token";
const KEYRING_ID_TOKEN: &str = "chatgpt_siwc_id_token";
const KEYRING_GEMINI_API_KEY: &str = "gemini_api_key";

/// Local Sign in with ChatGPT session record.
///
/// Mirrors the recommended fields in OpenAI's SIWC Accounts & Sessions guide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatGptSessionRecord {
    pub issuer: String,
    pub client_id: String,
    pub ext_agent_host_id: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default = "default_token_type")]
    pub token_type: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub access_token_expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub earliest_refresh_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub needs_reauth: bool,
    pub saved_at: DateTime<Utc>,
}

fn default_token_type() -> String {
    "Bearer".to_string()
}

impl ChatGptSessionRecord {
    /// Returns `true` when the granted scope list authorizes direct ChatGPT plan usage.
    pub fn has_plan_usage_scope(&self) -> bool {
        self.scopes
            .iter()
            .any(|scope| scope == CHATGPT_PLAN_USAGE_SCOPE)
    }

    /// Returns `true` when at least an access or refresh token is currently stored.
    pub fn has_active_tokens(&self) -> bool {
        self.access_token
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty())
            || self
                .refresh_token
                .as_deref()
                .is_some_and(|t| !t.trim().is_empty())
    }

    /// Projects this local record into the UI-safe [`ChatGptAuthStatus`] summary
    /// (contains zero secret tokens).
    pub fn to_auth_status(&self) -> ChatGptAuthStatus {
        let has_tokens = self.has_active_tokens();
        ChatGptAuthStatus {
            signed_in: has_tokens && !self.needs_reauth,
            plan_usage_enabled: has_tokens && !self.needs_reauth && self.has_plan_usage_scope(),
            email: self.email.clone(),
            name: self.name.clone(),
            subject: self.subject.clone(),
            client_id: (!self.client_id.trim().is_empty()).then(|| self.client_id.clone()),
            access_token_expires_at: self.access_token_expires_at,
            needs_reauth: self.needs_reauth,
        }
    }
}

/// Non-secret account/registration metadata persisted alongside keychain secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionMetadata {
    issuer: String,
    client_id: String,
    ext_agent_host_id: String,
    subject: Option<String>,
    email: Option<String>,
    name: Option<String>,
    token_type: String,
    expires_in: Option<u64>,
    access_token_expires_at: Option<DateTime<Utc>>,
    earliest_refresh_at: Option<DateTime<Utc>>,
    scopes: Vec<String>,
    needs_reauth: bool,
    saved_at: DateTime<Utc>,
    /// True when secrets are stored in the OS keychain rather than the fallback file.
    secrets_in_keychain: bool,
}

impl From<&ChatGptSessionRecord> for SessionMetadata {
    fn from(record: &ChatGptSessionRecord) -> Self {
        Self {
            issuer: record.issuer.clone(),
            client_id: record.client_id.clone(),
            ext_agent_host_id: record.ext_agent_host_id.clone(),
            subject: record.subject.clone(),
            email: record.email.clone(),
            name: record.name.clone(),
            token_type: record.token_type.clone(),
            expires_in: record.expires_in,
            access_token_expires_at: record.access_token_expires_at,
            earliest_refresh_at: record.earliest_refresh_at,
            scopes: record.scopes.clone(),
            needs_reauth: record.needs_reauth,
            saved_at: record.saved_at,
            secrets_in_keychain: true,
        }
    }
}

/// Storage abstraction for Sign in with ChatGPT credentials and host ID.
pub trait ChatGptCredentialStore: Send + Sync {
    /// Returns the persistent `ext_agent_host_id` (`urn:uuid:...`), creating and
    /// saving one on first call.
    fn load_or_create_host_id(&self) -> Result<String>;

    /// Loads the saved ChatGPT session record, if any.
    fn load_session(&self) -> Result<Option<ChatGptSessionRecord>>;

    /// Atomically persists the ChatGPT session record (including rotated tokens).
    fn save_session(&self, record: &ChatGptSessionRecord) -> Result<()>;

    /// Clears saved OAuth tokens and session state while keeping `ext_agent_host_id`.
    fn clear_session(&self) -> Result<()>;
}

/// Default VanguardCV config directory (`~/.config/vanguard-cv` / `%APPDATA%\vanguard-cv`).
pub fn vanguard_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("vanguard-cv")
}

/// Production credential store using OS keychain for secrets + owner-only local
/// file fallback in the user's config directory.
pub struct LocalChatGptStore {
    config_dir: PathBuf,
    use_keychain: bool,
    io_lock: Mutex<()>,
}

impl LocalChatGptStore {
    /// Creates a store backed by the default VanguardCV config directory and OS keychain.
    pub fn default_store() -> Self {
        Self {
            config_dir: vanguard_config_dir(),
            use_keychain: true,
            io_lock: Mutex::new(()),
        }
    }

    /// Creates a file-only store rooted at `config_dir` (useful for tests or
    /// headless environments without an OS keychain).
    pub fn file_only(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            use_keychain: false,
            io_lock: Mutex::new(()),
        }
    }

    fn host_id_path(&self) -> PathBuf {
        self.config_dir.join("chatgpt_host_id")
    }

    fn metadata_path(&self) -> PathBuf {
        self.config_dir.join("chatgpt_account.json")
    }

    fn fallback_session_path(&self) -> PathBuf {
        self.config_dir.join("chatgpt_session.json")
    }

    fn ensure_config_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.config_dir).with_context(|| {
            format!(
                "creating VanguardCV config directory {}",
                self.config_dir.display()
            )
        })
    }
}

impl ChatGptCredentialStore for LocalChatGptStore {
    fn load_or_create_host_id(&self) -> Result<String> {
        let _guard = self.io_lock.lock().unwrap_or_else(|e| e.into_inner());
        let path = self.host_id_path();
        if let Ok(existing) = std::fs::read_to_string(&path) {
            let trimmed = existing.trim();
            if trimmed.starts_with("urn:uuid:") && trimmed.len() > "urn:uuid:".len() {
                return Ok(trimmed.to_string());
            }
        }
        self.ensure_config_dir()?;
        let fresh = format!("urn:uuid:{}", Uuid::new_v4());
        atomic_write_private(&path, fresh.as_bytes())?;
        Ok(fresh)
    }

    fn load_session(&self) -> Result<Option<ChatGptSessionRecord>> {
        let _guard = self.io_lock.lock().unwrap_or_else(|e| e.into_inner());

        // 1. Check metadata file (paired with OS keychain secrets)
        if let Ok(raw) = std::fs::read_to_string(self.metadata_path())
            && let Ok(meta) = serde_json::from_str::<SessionMetadata>(&raw)
            && meta.secrets_in_keychain
            && self.use_keychain
        {
            let access_token = read_keychain_secret(KEYRING_ACCESS_TOKEN).ok().flatten();
            let refresh_token = read_keychain_secret(KEYRING_REFRESH_TOKEN).ok().flatten();
            let id_token = read_keychain_secret(KEYRING_ID_TOKEN).ok().flatten();
            return Ok(Some(ChatGptSessionRecord {
                issuer: meta.issuer,
                client_id: meta.client_id,
                ext_agent_host_id: meta.ext_agent_host_id,
                subject: meta.subject,
                email: meta.email,
                name: meta.name,
                id_token,
                access_token,
                refresh_token,
                token_type: meta.token_type,
                expires_in: meta.expires_in,
                access_token_expires_at: meta.access_token_expires_at,
                earliest_refresh_at: meta.earliest_refresh_at,
                scopes: meta.scopes,
                needs_reauth: meta.needs_reauth,
                saved_at: meta.saved_at,
            }));
        }

        // 2. Fallback private local JSON file
        let fallback_path = self.fallback_session_path();
        if fallback_path.exists() {
            let raw = std::fs::read_to_string(&fallback_path)
                .with_context(|| format!("reading {}", fallback_path.display()))?;
            let record: ChatGptSessionRecord = serde_json::from_str(&raw)
                .with_context(|| format!("parsing {}", fallback_path.display()))?;
            return Ok(Some(record));
        }

        Ok(None)
    }

    fn save_session(&self, record: &ChatGptSessionRecord) -> Result<()> {
        let _guard = self.io_lock.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure_config_dir()?;

        // Try storing secrets in OS keychain first when enabled.
        if self.use_keychain {
            let keychain_ok = write_or_clear_keychain_secret(
                KEYRING_ACCESS_TOKEN,
                record.access_token.as_deref(),
            )
            .and_then(|()| {
                write_or_clear_keychain_secret(
                    KEYRING_REFRESH_TOKEN,
                    record.refresh_token.as_deref(),
                )
            })
            .and_then(|()| {
                write_or_clear_keychain_secret(KEYRING_ID_TOKEN, record.id_token.as_deref())
            });

            if keychain_ok.is_ok() {
                let meta = SessionMetadata::from(record);
                let json = serde_json::to_vec_pretty(&meta)?;
                atomic_write_private(&self.metadata_path(), &json)?;
                // Remove any older fallback secret file so tokens don't live in two places.
                let _ = std::fs::remove_file(self.fallback_session_path());
                return Ok(());
            } else if let Err(err) = keychain_ok {
                tracing::warn!(
                    "OS keychain unavailable for ChatGPT session ({err}); using private local file fallback"
                );
            }
        }

        // Private owner-only local file fallback
        let json = serde_json::to_vec_pretty(record)?;
        atomic_write_private(&self.fallback_session_path(), &json)?;
        let _ = std::fs::remove_file(self.metadata_path());
        Ok(())
    }

    fn clear_session(&self) -> Result<()> {
        let _guard = self.io_lock.lock().unwrap_or_else(|e| e.into_inner());
        if self.use_keychain {
            let _ = write_or_clear_keychain_secret(KEYRING_ACCESS_TOKEN, None);
            let _ = write_or_clear_keychain_secret(KEYRING_REFRESH_TOKEN, None);
            let _ = write_or_clear_keychain_secret(KEYRING_ID_TOKEN, None);
        }
        let _ = std::fs::remove_file(self.metadata_path());
        let _ = std::fs::remove_file(self.fallback_session_path());
        Ok(())
    }
}

fn gemini_fallback_key_path() -> PathBuf {
    vanguard_config_dir().join("gemini_api_key")
}

/// Loads the user's saved Google Gemini API key from the OS keychain (or private local fallback).
pub fn load_gemini_api_key() -> Result<Option<String>> {
    if let Ok(Some(secret)) = read_keychain_secret(KEYRING_GEMINI_API_KEY) {
        return Ok(Some(secret));
    }
    let fallback = gemini_fallback_key_path();
    if let Ok(raw) = std::fs::read_to_string(&fallback) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_string()));
        }
    }
    Ok(None)
}

/// Persists the user's Google Gemini API key in the OS keychain, falling back to an
/// owner-only local file if the OS keychain is unavailable.
pub fn save_gemini_api_key(key: &str) -> Result<()> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return clear_gemini_api_key();
    }
    if write_or_clear_keychain_secret(KEYRING_GEMINI_API_KEY, Some(trimmed)).is_ok() {
        let _ = std::fs::remove_file(gemini_fallback_key_path());
        return Ok(());
    }
    let dir = vanguard_config_dir();
    std::fs::create_dir_all(&dir)?;
    atomic_write_private(&gemini_fallback_key_path(), trimmed.as_bytes())?;
    Ok(())
}

/// Removes the user's saved Google Gemini API key from keychain and fallback storage.
pub fn clear_gemini_api_key() -> Result<()> {
    let _ = write_or_clear_keychain_secret(KEYRING_GEMINI_API_KEY, None);
    let _ = std::fs::remove_file(gemini_fallback_key_path());
    Ok(())
}

/// In-memory credential store for deterministic unit & integration tests.
#[derive(Default)]
pub struct InMemoryChatGptStore {
    host_id: Mutex<Option<String>>,
    session: Mutex<Option<ChatGptSessionRecord>>,
}

impl InMemoryChatGptStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_session(record: ChatGptSessionRecord) -> Self {
        Self {
            host_id: Mutex::new(Some(record.ext_agent_host_id.clone())),
            session: Mutex::new(Some(record)),
        }
    }
}

impl ChatGptCredentialStore for InMemoryChatGptStore {
    fn load_or_create_host_id(&self) -> Result<String> {
        let mut lock = self.host_id.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(id) = lock.as_ref() {
            return Ok(id.clone());
        }
        let fresh = format!("urn:uuid:{}", Uuid::new_v4());
        *lock = Some(fresh.clone());
        Ok(fresh)
    }

    fn load_session(&self) -> Result<Option<ChatGptSessionRecord>> {
        Ok(self
            .session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone())
    }

    fn save_session(&self, record: &ChatGptSessionRecord) -> Result<()> {
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(record.clone());
        Ok(())
    }

    fn clear_session(&self) -> Result<()> {
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }
}

fn read_keychain_secret(account: &str) -> Result<Option<String>> {
    let entry = keyring::v1::Entry::new(KEYRING_SERVICE, account)
        .map_err(|err| anyhow::anyhow!("OS keychain unavailable: {err}"))?;
    match entry.get_password() {
        Ok(secret) if !secret.trim().is_empty() => Ok(Some(secret)),
        Ok(_) | Err(keyring::v1::Error::NoEntry) => Ok(None),
        Err(err) => Err(anyhow::anyhow!("reading OS keychain failed: {err}")),
    }
}

fn write_or_clear_keychain_secret(account: &str, secret: Option<&str>) -> Result<()> {
    let entry = keyring::v1::Entry::new(KEYRING_SERVICE, account)
        .map_err(|err| anyhow::anyhow!("OS keychain unavailable: {err}"))?;
    match secret.map(str::trim).filter(|s| !s.is_empty()) {
        Some(value) => entry
            .set_password(value)
            .with_context(|| format!("saving {account} to OS keychain")),
        None => match entry.delete_credential() {
            Ok(()) | Err(keyring::v1::Error::NoEntry) => Ok(()),
            Err(err) => Err(anyhow::anyhow!(
                "removing {account} from OS keychain failed: {err}"
            )),
        },
    }
}

/// Atomically writes `bytes` to `path` via a sibling temp file, setting `0600`
/// permissions on Unix so only the current OS user can read credentials.
fn atomic_write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp_name = format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("vanguardcv"),
        Uuid::new_v4()
    );
    let tmp_path = path.with_file_name(tmp_name);

    std::fs::write(&tmp_path, bytes)
        .with_context(|| format!("writing temp credential file {}", tmp_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600));
    }

    // On Windows, std::fs::rename fails if target exists; remove target first.
    #[cfg(windows)]
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }

    std::fs::rename(&tmp_path, path)
        .with_context(|| format!("committing credential file {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_id_persists_across_session_clears() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = LocalChatGptStore::file_only(dir.path().to_path_buf());

        let host_1 = store.load_or_create_host_id()?;
        assert!(host_1.starts_with("urn:uuid:"));

        let record = ChatGptSessionRecord {
            issuer: "https://auth.openai.com".to_string(),
            client_id: "oaiapp_test123".to_string(),
            ext_agent_host_id: host_1.clone(),
            subject: Some("user-sub-1".to_string()),
            email: Some("user@example.com".to_string()),
            name: Some("Alex Morgan".to_string()),
            id_token: Some("id.jwt.token".to_string()),
            access_token: Some("access.jwt.token".to_string()),
            refresh_token: Some("refresh_tok_1".to_string()),
            token_type: "Bearer".to_string(),
            expires_in: Some(3600),
            access_token_expires_at: Some(Utc::now() + chrono::Duration::hours(1)),
            earliest_refresh_at: None,
            scopes: vec!["openid".to_string(), CHATGPT_PLAN_USAGE_SCOPE.to_string()],
            needs_reauth: false,
            saved_at: Utc::now(),
        };

        store.save_session(&record)?;
        let loaded = store.load_session()?.expect("session should be persisted");
        assert_eq!(loaded.client_id, "oaiapp_test123");
        assert_eq!(loaded.refresh_token.as_deref(), Some("refresh_tok_1"));
        assert!(loaded.has_plan_usage_scope());
        assert!(loaded.to_auth_status().is_ready_for_inference());

        store.clear_session()?;
        assert!(store.load_session()?.is_none());

        // Host ID must survive sign-out / session clear.
        let host_2 = store.load_or_create_host_id()?;
        assert_eq!(host_1, host_2);
        Ok(())
    }
}
