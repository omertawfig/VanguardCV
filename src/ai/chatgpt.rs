//! Official OpenAI Sign in with ChatGPT (OAuth 2.0 / OIDC) + Responses API
//! streaming client and model discovery.
//!
//! Strictly implements OpenAI's official **Sign in with ChatGPT — Open-Source &
//! Local Apps (ChatGPT Plan Usage)** specification:
//! - Loopback OAuth 2.0 + PKCE (`S256`) on `http://127.0.0.1:<ephemeral>/auth/callback`
//! - Dynamic client registration (`client_id=dynamic_agent_client`,
//!   `agent_name_hint=VanguardCV`, and persistent `ext_agent_host_id`),
//!   saving and reusing the issued `client_id` (`oaiapp_...`) on reauthorization
//! - OIDC ID token verification against `https://auth.openai.com/.well-known/jwks.json`
//!   (`iss`, `aud`, `exp`, `nonce`) and `chatgpt.tokens.use.direct` scope verification
//! - Serialized rotating refresh token renewal (`grant_type=refresh_token`)
//! - OIDC discovery revocation on sign-out (`revocation_endpoint`)
//! - Official `GET https://api.openai.com/v1/models` and streaming
//!   `POST https://api.openai.com/v1/responses` adhering to all SIWC preview
//!   compatibility rules (`store: false`, `stream: true`, array `input`,
//!   top-level `instructions`, namespaced function tools, zero forbidden params)
//! - Zero-trust PII air-gap enforcement via FNV-1a [`ScrubbedPayloadToken`] check

use std::collections::HashSet;
use std::future::Future;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use eventsource_stream::Eventsource;
use futures::StreamExt;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use rand::RngCore;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex as AsyncMutex;
use url::Url;
use uuid::Uuid;

use crate::contracts::{
    AiConfig, AiEvent, AiEventSink, AiOrchestrator, AiProposal, AtsScore, CHATGPT_AGENT_NAME_HINT,
    CHATGPT_API_BASE_URL, CHATGPT_AUTH_ISSUER,
    CHATGPT_USAGE_SETTINGS_URL, ChatGptAuthStatus, DEFAULT_CHATGPT_MODEL, InlineRewriteRequest,
    ItemId, ModelInfo, SanitizedCvBody, ScrubbedPayload, SectionKind, SkillRegistry,
    VariationOutcome, VariationRequest,
};
use crate::credentials::{ChatGptCredentialStore, ChatGptSessionRecord, LocalChatGptStore};

/// OpenAI's official dynamic registration client ID for open-source/local apps.
pub const DYNAMIC_AGENT_CLIENT_ID: &str = "dynamic_agent_client";

/// Required OAuth scopes for Sign in with ChatGPT plan usage.
pub const SIWC_SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

/// Loopback callback path registered for SIWC dynamic agent clients.
pub const SIWC_CALLBACK_PATH: &str = "/auth/callback";

/// Tool namespace name used on `/v1/responses` (SIWC preview requires function
/// tools to be grouped inside a `namespace` tool rather than bare top-level functions).
pub const TOOL_NAMESPACE_NAME: &str = "vanguard_cv";

/// Default timeout while waiting for the user to finish signing in via browser.
const OAUTH_CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

/// Proactive refresh window before access token expiry.
const ACCESS_TOKEN_REFRESH_SKEW_SECS: i64 = 60;

/// Builds a resilient HTTP client for AI API streaming calls.
///
/// Disables idle keep-alive pooling (`pool_max_idle_per_host(0)`) so `POST`
/// requests never fail against a stale connection previously left open by
/// `GET /models`, and separates connection timeout from streaming read timeout.
pub(crate) fn build_chatgpt_http_client(request_timeout_secs: u64) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("VanguardCV/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(request_timeout_secs.max(180)))
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .pool_max_idle_per_host(0)
        .build()
        .context("building HTTP client")
}

/// Computes the 64-bit FNV-1a digest over a byte string, matching
/// [`crate::contracts::ScrubbedPayloadToken::mint`].
pub fn compute_fnv1a_digest(text: &str) -> u64 {
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        digest ^= u64::from(*byte);
        digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
    }
    digest
}

/// Resolves the OpenAI API base URL, falling back to [`CHATGPT_API_BASE_URL`].
fn resolve_api_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        CHATGPT_API_BASE_URL.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Resolves the OpenAI OAuth/OIDC issuer URL, falling back to [`CHATGPT_AUTH_ISSUER`].
fn resolve_auth_issuer(issuer: &str) -> String {
    let trimmed = issuer.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        CHATGPT_AUTH_ISSUER.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Path of the optional provider configuration file (`~/.config/vanguard-cv/ai.toml`).
pub fn provider_config_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from(".config"))
        .join("vanguard-cv")
        .join("ai.toml")
}

#[derive(Debug, Default, Deserialize)]
struct ProviderConfigFile {
    provider: Option<ProviderSection>,
}

#[derive(Debug, Default, Deserialize)]
struct ProviderSection {
    base_url: Option<String>,
    auth_issuer: Option<String>,
    model: Option<String>,
    request_timeout_secs: Option<u64>,
}

fn is_official_or_loopback_api_url(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url.trim()) else {
        return false;
    };
    match (parsed.scheme(), parsed.host_str()) {
        ("https", Some(host)) => host.eq_ignore_ascii_case("api.openai.com"),
        ("http", Some("127.0.0.1")) => true,
        _ => false,
    }
}

fn is_official_or_loopback_issuer(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url.trim()) else {
        return false;
    };
    match (parsed.scheme(), parsed.host_str()) {
        ("https", Some(host)) => host.eq_ignore_ascii_case("auth.openai.com"),
        ("http", Some("127.0.0.1")) => true,
        _ => false,
    }
}

fn load_provider_file(config: &mut AiConfig) {
    let path = provider_config_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    match toml::from_str::<ProviderConfigFile>(&text) {
        Ok(file) => {
            if let Some(provider) = file.provider {
                if let Some(base_url) = provider
                    .base_url
                    .filter(|v| is_official_or_loopback_api_url(v))
                {
                    config.base_url = base_url.trim().trim_end_matches('/').to_string();
                }
                if let Some(issuer) = provider
                    .auth_issuer
                    .filter(|v| is_official_or_loopback_issuer(v))
                {
                    config.auth_issuer = issuer.trim().trim_end_matches('/').to_string();
                }
                if let Some(model) = provider
                    .model
                    .map(|m| m.trim().to_string())
                    .filter(|m| !m.is_empty() && !m.to_ascii_lowercase().starts_with("gemini"))
                {
                    config.model = model;
                }
                if let Some(timeout) = provider.request_timeout_secs {
                    config.request_timeout_secs = timeout.max(10);
                }
            }
        }
        Err(err) => tracing::warn!("Ignoring invalid provider config {}: {err}", path.display()),
    }
}

/// Concrete implementation of [`AiOrchestrator`] using OpenAI's official Sign in
/// with ChatGPT (OAuth 2.0 + OIDC) and streaming Responses API (`/v1/responses`).
pub struct ChatGptClient {
    config: RwLock<AiConfig>,
    client: RwLock<reqwest::Client>,
    skills: RwLock<Option<Arc<dyn SkillRegistry>>>,
    store: Arc<dyn ChatGptCredentialStore>,
    session: RwLock<Option<ChatGptSessionRecord>>,
    /// Serializes token refresh calls so rotating refresh tokens never race.
    refresh_lock: AsyncMutex<()>,
}

impl ChatGptClient {
    /// Creates a new client with the provided configuration and credential store.
    pub fn with_store(mut config: AiConfig, store: Arc<dyn ChatGptCredentialStore>) -> Self {
        config.base_url = resolve_api_base_url(&config.base_url);
        config.auth_issuer = resolve_auth_issuer(&config.auth_issuer);
        if config.model.trim().is_empty() {
            config.model = DEFAULT_CHATGPT_MODEL.to_string();
        } else {
            config.model = config.model.trim().to_string();
        }

        let client = build_chatgpt_http_client(config.request_timeout_secs)
            .unwrap_or_default();

        let initial_session = match store.load_session() {
            Ok(session) => session,
            Err(err) => {
                tracing::warn!("Could not load persisted ChatGPT session: {err}");
                None
            }
        };

        Self {
            config: RwLock::new(config),
            client: RwLock::new(client),
            skills: RwLock::new(None),
            store,
            session: RwLock::new(initial_session),
            refresh_lock: AsyncMutex::new(()),
        }
    }

    /// Creates a new client backed by the default OS keychain / local credential store.
    pub fn new(config: AiConfig) -> Self {
        Self::with_store(config, Arc::new(LocalChatGptStore::default_store()))
    }

    /// Initializes a client from optional local config file + environment variables:
    /// - `OPENAI_MODEL` (fallback: `VANGUARDCV_AI_MODEL`)
    pub fn from_env() -> Self {
        let mut config = AiConfig::default();
        load_provider_file(&mut config);

        if let Ok(model) =
            std::env::var("OPENAI_MODEL").or_else(|_| std::env::var("VANGUARDCV_AI_MODEL"))
        {
            let trimmed = model.trim();
            if !trimmed.is_empty() && !trimmed.to_ascii_lowercase().starts_with("gemini") {
                config.model = trimmed.to_string();
            }
        }
        Self::new(config)
    }

    /// Attaches a skill registry used to compose system instructions for active skills.
    pub fn with_skills(self, skills: Arc<dyn SkillRegistry>) -> Self {
        if let Ok(mut lock) = self.skills.write() {
            *lock = Some(skills);
        }
        self
    }

    /// Dynamically sets or replaces the skill registry.
    pub fn set_skills(&self, skills: Arc<dyn SkillRegistry>) {
        if let Ok(mut lock) = self.skills.write() {
            *lock = Some(skills);
        }
    }

    fn http_client(&self) -> reqwest::Client {
        self.client.read().map(|c| c.clone()).unwrap_or_default()
    }

    fn current_session(&self) -> Option<ChatGptSessionRecord> {
        self.session.read().ok().and_then(|s| s.clone())
    }

    fn update_session(&self, next: Option<ChatGptSessionRecord>) -> Result<()> {
        match &next {
            Some(record) => self.store.save_session(record)?,
            None => self.store.clear_session()?,
        }
        if let Ok(mut lock) = self.session.write() {
            *lock = next;
        }
        Ok(())
    }

    /// Marks the active session as needing reauthorization while keeping the
    /// issued `client_id`, `ext_agent_host_id`, and `id_token` for clean reauth.
    fn mark_session_needs_reauth(&self) {
        if let Some(mut record) = self.current_session() {
            record.access_token = None;
            record.refresh_token = None;
            record.access_token_expires_at = None;
            record.earliest_refresh_at = None;
            record.needs_reauth = true;
            record.saved_at = Utc::now();
            let _ = self.update_session(Some(record));
        }
    }

    /// Re-verifies that `payload.token` was minted by a [`crate::contracts::PrivacyGuard`]
    /// and that `payload.json` has not been tampered with.
    pub fn verify_security_gate(&self, payload: &ScrubbedPayload) -> Result<()> {
        let computed = compute_fnv1a_digest(&payload.json);
        let expected = payload.token.payload_digest();

        if computed != expected {
            bail!(
                "Security gate violation: outbound payload digest mismatch (computed {computed:#018x}, expected {expected:#018x}). \
                 Payload has been modified or was not properly minted by PrivacyGuard."
            );
        }
        Ok(())
    }

    fn compose_skills(&self, skill_ids: &[String]) -> String {
        if let Ok(lock) = self.skills.read()
            && let Some(registry) = lock.as_ref()
        {
            registry
                .compose_system_prompt(skill_ids)
                .unwrap_or_default()
        } else {
            String::new()
        }
    }

    /// Runs the complete Sign in with ChatGPT loopback OAuth + PKCE + OIDC flow
    /// using a custom launcher closure (system browser in production, mock HTTP
    /// client in deterministic tests).
    pub async fn sign_in_with_launcher<F, Fut>(
        &self,
        force_consent: bool,
        fresh_registration: bool,
        callback_timeout: Duration,
        launcher: F,
    ) -> Result<ChatGptAuthStatus>
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let config = self.config();
        let issuer = resolve_auth_issuer(&config.auth_issuer);
        let api_base = resolve_api_base_url(&config.base_url);
        let host_id = self
            .store
            .load_or_create_host_id()
            .context("loading persistent ext_agent_host_id")?;

        // Bind strictly to 127.0.0.1 with an ephemeral OS port (never "localhost").
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("binding 127.0.0.1 loopback callback server for Sign in with ChatGPT")?;
        let port = listener.local_addr()?.port();
        let redirect_uri = format!("http://127.0.0.1:{port}{SIWC_CALLBACK_PATH}");

        let pkce = PkcePair::generate();
        let state = generate_urlsafe_secret(32);
        let nonce = generate_urlsafe_secret(32);

        let prior_session = if fresh_registration {
            None
        } else {
            self.current_session().filter(|s| {
                !s.client_id.trim().is_empty() && s.client_id != DYNAMIC_AGENT_CLIENT_ID
            })
        };

        let is_reauth = prior_session.is_some();
        let authorize_url = build_authorize_url(&AuthorizeUrlParams {
            issuer: &issuer,
            redirect_uri: &redirect_uri,
            resource: &api_base,
            ext_agent_host_id: &host_id,
            state: &state,
            nonce: &nonce,
            code_challenge: &pkce.challenge,
            existing_client_id: prior_session.as_ref().map(|s| s.client_id.as_str()),
            id_token_hint: prior_session.as_ref().and_then(|s| s.id_token.as_deref()),
            login_hint: prior_session.as_ref().and_then(|s| s.email.as_deref()),
            force_consent,
        })?;

        // Launch the browser (NEVER log `authorize_url` when `id_token_hint` is present).
        tracing::info!(
            is_reauth,
            force_consent,
            redirect_uri = %redirect_uri,
            "Launching Sign in with ChatGPT browser flow"
        );
        launcher(authorize_url).await?;

        // Wait for the loopback callback from auth.openai.com.
        let callback = wait_for_oauth_callback(listener, callback_timeout).await?;

        if let Some(error) = callback.error.as_deref() {
            if error == "access_denied" {
                bail!("Sign in with ChatGPT was canceled before authorization completed.");
            }
            let desc = callback
                .error_description
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|d| format!(": {d}"))
                .unwrap_or_default();
            bail!("Sign in with ChatGPT returned error `{error}`{desc}");
        }

        if callback.state.as_deref() != Some(state.as_str()) {
            bail!(
                "Sign in with ChatGPT callback rejected: OAuth `state` parameter mismatch (possible CSRF)."
            );
        }

        let code = callback
            .code
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .ok_or_else(|| {
                anyhow!("Sign in with ChatGPT callback omitted authorization `code`.")
            })?;

        let issued_client_id = match prior_session.as_ref() {
            Some(prior) => match callback
                .client_id
                .as_deref()
                .map(str::trim)
                .filter(|c| !c.is_empty())
            {
                Some(returned) if returned != prior.client_id => {
                    bail!(
                        "Sign in with ChatGPT callback returned unexpected client_id `{returned}` during reauthorization."
                    );
                }
                _ => prior.client_id.clone(),
            },
            None => {
                let returned = callback
                    .client_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                    .ok_or_else(|| {
                        anyhow!(
                            "Sign in with ChatGPT initial registration did not return an issued `client_id`."
                        )
                    })?;
                if returned == DYNAMIC_AGENT_CLIENT_ID {
                    bail!(
                        "Sign in with ChatGPT returned placeholder `dynamic_agent_client` instead of an issued client_id."
                    );
                }
                returned.to_string()
            }
        };

        // Exchange authorization code + PKCE verifier for tokens.
        let token_response = self
            .exchange_authorization_code(
                &issuer,
                &issued_client_id,
                code,
                &pkce.verifier,
                &redirect_uri,
                &api_base,
            )
            .await?;

        // Verify the OIDC ID token signature and claims (iss, aud, exp, nonce).
        let id_claims = self
            .verify_id_token(&issuer, &issued_client_id, &nonce, &token_response.id_token)
            .await?;

        let now = Utc::now();
        let scopes: Vec<String> = token_response
            .scope
            .as_deref()
            .or(callback.scope.as_deref())
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_string)
            .collect();

        let access_token_expires_at = token_response
            .expires_in
            .map(|secs| now + chrono::Duration::seconds(secs as i64));
        let earliest_refresh_at = token_response
            .earliest_refresh_at
            .as_deref()
            .and_then(parse_timestamp);

        let record = ChatGptSessionRecord {
            issuer,
            client_id: issued_client_id,
            ext_agent_host_id: host_id,
            subject: Some(id_claims.sub),
            email: id_claims.email,
            name: id_claims.name.or(id_claims.preferred_username),
            id_token: Some(token_response.id_token),
            access_token: Some(token_response.access_token),
            refresh_token: token_response.refresh_token,
            token_type: token_response
                .token_type
                .unwrap_or_else(|| "Bearer".to_string()),
            expires_in: token_response.expires_in,
            access_token_expires_at,
            earliest_refresh_at,
            scopes,
            needs_reauth: false,
            saved_at: now,
        };

        let status = record.to_auth_status();
        self.update_session(Some(record))?;
        Ok(status)
    }

    async fn exchange_authorization_code(
        &self,
        issuer: &str,
        client_id: &str,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
        resource: &str,
    ) -> Result<OAuthTokenResponse> {
        let endpoint = format!("{issuer}/api/accounts/oauth/token");
        let body = encode_form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code),
            ("code_verifier", code_verifier),
            ("redirect_uri", redirect_uri),
            ("resource", resource),
        ]);

        let response = self
            .http_client()
            .post(&endpoint)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .with_context(|| format!("exchanging authorization code at {endpoint}"))?;

        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("{}", format_oauth_error(status, &text));
        }

        let parsed: OAuthTokenResponse = serde_json::from_str(&text)
            .with_context(|| "parsing OAuth token exchange response JSON".to_string())?;
        if parsed.access_token.trim().is_empty() || parsed.id_token.trim().is_empty() {
            bail!("OAuth token response was missing required access_token or id_token.");
        }
        Ok(parsed)
    }

    async fn fetch_discovery(&self, issuer: &str) -> Result<OidcDiscovery> {
        let url = format!("{issuer}/.well-known/openid-configuration");
        let response = self
            .http_client()
            .get(&url)
            .send()
            .await
            .with_context(|| format!("fetching OpenID discovery document from {url}"))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("OpenID discovery returned HTTP {}: {text}", status.as_u16());
        }
        serde_json::from_str(&text).context("parsing OpenID discovery document")
    }

    async fn verify_id_token(
        &self,
        issuer: &str,
        expected_client_id: &str,
        expected_nonce: &str,
        id_token: &str,
    ) -> Result<IdTokenClaims> {
        let header = jsonwebtoken::decode_header(id_token)
            .context("decoding Sign in with ChatGPT ID token header")?;

        if !is_allowed_asymmetric_alg(header.alg) {
            bail!(
                "Rejecting ID token with unsupported signing algorithm {:?}",
                header.alg
            );
        }

        // Prefer jwks_uri from discovery if available, otherwise standard issuer JWKS path.
        let jwks_url = self
            .fetch_discovery(issuer)
            .await
            .ok()
            .and_then(|d| d.jwks_uri)
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| format!("{issuer}/.well-known/jwks.json"));

        let response = self
            .http_client()
            .get(&jwks_url)
            .send()
            .await
            .with_context(|| format!("fetching OpenAI JWKS from {jwks_url}"))?;
        let status = response.status();
        let jwks_text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("Fetching JWKS failed with HTTP {}", status.as_u16());
        }

        let jwks: JwkSet =
            serde_json::from_str(&jwks_text).context("parsing OpenAI JWKS key set")?;

        let jwk = match header.kid.as_deref() {
            Some(kid) => jwks
                .find(kid)
                .ok_or_else(|| anyhow!("ID token key id `{kid}` was not found in OpenAI JWKS"))?,
            None => jwks
                .keys
                .first()
                .ok_or_else(|| anyhow!("OpenAI JWKS contained no verification keys"))?,
        };

        let decoding_key =
            DecodingKey::from_jwk(jwk).context("constructing verification key from OpenAI JWK")?;

        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[issuer]);
        validation.set_audience(&[expected_client_id]);
        validation.validate_exp = true;

        let token_data =
            jsonwebtoken::decode::<IdTokenClaims>(id_token, &decoding_key, &validation)
                .context("verifying Sign in with ChatGPT ID token signature and claims")?;

        if token_data.claims.nonce.as_deref() != Some(expected_nonce) {
            bail!("Sign in with ChatGPT ID token `nonce` did not match this sign-in attempt.");
        }

        if token_data.claims.sub.trim().is_empty() {
            bail!("Sign in with ChatGPT ID token is missing required `sub` claim.");
        }

        Ok(token_data.claims)
    }

    /// Returns a valid Bearer access token, transparently refreshing via the
    /// serialized rotating refresh token mutex when expiring or after a 401.
    async fn ensure_valid_access_token(&self, force_refresh: bool) -> Result<String> {
        let now = Utc::now();
        if !force_refresh
            && let Some(session) = self.current_session()
            && !session.needs_reauth
            && session.has_plan_usage_scope()
            && let Some(token) = session
                .access_token
                .as_deref()
                .filter(|t| !t.trim().is_empty())
            && !token_needs_refresh(&session, now)
        {
            return Ok(token.to_string());
        }

        // Acquire the async mutex so concurrent requests never double-spend a
        // rotating refresh token.
        let _guard = self.refresh_lock.lock().await;
        let now = Utc::now();

        let Some(mut session) = self.current_session() else {
            bail!(
                "Sign in with ChatGPT is required. Open Settings and click \"Continue with ChatGPT\"."
            );
        };

        if session.needs_reauth {
            bail!(
                "Your ChatGPT session expired. Open Settings and click \"Continue with ChatGPT\" to sign in again."
            );
        }

        if !session.has_plan_usage_scope() {
            bail!(
                "ChatGPT plan usage is not enabled for VanguardCV. Open Settings and click \"Enable ChatGPT plan usage\" to grant access."
            );
        }

        // Double-check after acquiring lock in case another task just refreshed.
        if !force_refresh
            && let Some(token) = session
                .access_token
                .as_deref()
                .filter(|t| !t.trim().is_empty())
            && !token_needs_refresh(&session, now)
        {
            return Ok(token.to_string());
        }

        let Some(refresh_token) = session
            .refresh_token
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
        else {
            self.mark_session_needs_reauth();
            bail!(
                "No refresh token is stored for your ChatGPT session. Open Settings and click \"Continue with ChatGPT\"."
            );
        };

        let config = self.config();
        let issuer = resolve_auth_issuer(&config.auth_issuer);
        let api_base = resolve_api_base_url(&config.base_url);
        let endpoint = format!("{issuer}/api/accounts/oauth/token");

        // Per OpenAI SIWC docs: omit `scope` on refresh; send resource + client_id + refresh_token.
        let body = encode_form(&[
            ("grant_type", "refresh_token"),
            ("client_id", &session.client_id),
            ("refresh_token", &refresh_token),
            ("resource", &api_base),
        ]);

        let response = self
            .http_client()
            .post(&endpoint)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .with_context(|| format!("refreshing ChatGPT access token at {endpoint}"))?;

        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            if is_fatal_refresh_error(status, &text) {
                self.mark_session_needs_reauth();
                bail!(
                    "Your ChatGPT session expired or was revoked. Open Settings and click \"Continue with ChatGPT\" to sign in again."
                );
            }
            bail!("{}", format_oauth_error(status, &text));
        }

        let refreshed: OAuthRefreshResponse =
            serde_json::from_str(&text).context("parsing OAuth refresh response JSON")?;
        if refreshed.access_token.trim().is_empty() {
            bail!("OAuth refresh response returned an empty access_token.");
        }

        let refreshed_at = Utc::now();
        session.access_token = Some(refreshed.access_token.clone());
        if let Some(next_refresh) = refreshed
            .refresh_token
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
        {
            session.refresh_token = Some(next_refresh);
        }
        if let Some(next_id) = refreshed
            .id_token
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
        {
            session.id_token = Some(next_id);
        }
        if let Some(token_type) = refreshed.token_type {
            session.token_type = token_type;
        }
        let expires_in = refreshed.expires_in.unwrap_or(3600);
        session.expires_in = Some(expires_in);
        session.access_token_expires_at =
            Some(refreshed_at + chrono::Duration::seconds(expires_in as i64));
        if let Some(earliest) = refreshed
            .earliest_refresh_at
            .as_deref()
            .and_then(parse_timestamp)
        {
            session.earliest_refresh_at = Some(earliest);
        }
        if let Some(scope_str) = refreshed.scope {
            let parsed_scopes: Vec<String> =
                scope_str.split_whitespace().map(str::to_string).collect();
            if !parsed_scopes.is_empty() {
                session.scopes = parsed_scopes;
            }
        }
        session.needs_reauth = false;
        session.saved_at = refreshed_at;

        self.update_session(Some(session.clone()))?;

        if !session.has_plan_usage_scope() {
            bail!(
                "ChatGPT plan usage scope (`chatgpt.tokens.use.direct`) is no longer granted. Open Settings to re-enable plan usage."
            );
        }

        Ok(refreshed.access_token)
    }

    async fn fetch_models_with_token(
        &self,
        base_url: &str,
        token: &str,
    ) -> Result<reqwest::Response> {
        let endpoint = format!("{base_url}/models");
        self.http_client()
            .get(&endpoint)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| anyhow!("Failed to contact OpenAI API ({base_url}): {e}"))
    }
}

impl Default for ChatGptClient {
    fn default() -> Self {
        Self::from_env()
    }
}

#[async_trait]
impl AiOrchestrator for ChatGptClient {
    async fn generate_variation(
        &self,
        request: VariationRequest,
        payload: ScrubbedPayload,
        events: AiEventSink,
    ) -> Result<VariationOutcome> {
        let run_id = Uuid::new_v4();
        let _ = events.send(AiEvent::RunStarted { run_id });

        // 1. Hard Security Gate: Payload digest verification
        if let Err(err) = self.verify_security_gate(&payload) {
            let msg = err.to_string();
            let _ = events.send(AiEvent::RunFailed {
                run_id,
                message: msg,
            });
            return Err(err);
        }

        // 2. Ensure valid SIWC access token (transparently refreshing if needed)
        let access_token = match self.ensure_valid_access_token(false).await {
            Ok(token) => token,
            Err(err) => {
                let _ = events.send(AiEvent::RunFailed {
                    run_id,
                    message: err.to_string(),
                });
                return Err(err);
            }
        };

        let config = self.config();
        let model_id = config.model.trim().to_string();
        if model_id.is_empty() {
            let err = anyhow!("No ChatGPT model is selected. Pick a model in Settings.");
            let _ = events.send(AiEvent::RunFailed {
                run_id,
                message: err.to_string(),
            });
            return Err(err);
        }

        let _ = events.send(AiEvent::Status {
            message: format!("Streaming tailoring proposals via ChatGPT ({model_id})…"),
        });

        // 3. Compose SIWC-compliant Responses API request
        let skill_directives = self.compose_skills(&request.skill_ids);
        let instructions = build_system_prompt(&skill_directives);
        let user_prompt = format!(
            "Target Company: {}\nTarget Role: {}\n\nJob Description:\n{}\n\nCandidate Scrubbed CV Data (JSON):\n{}\n\n\
             Instructions:\n\
             1. Audit every section, timeline entry, bullet, and skill category against the target job description.\n\
             2. Proactively call `toggle_item_visibility(item_id, enabled=false, reason=...)` to REMOVE (hide from this tailored PDF) \
             any sections, roles, bullets, or skill categories that are irrelevant, redundant, off-domain, or could weaken/harm the candidate's fit for this role. Always explain clearly why in `reason`.\n\
             3. Call `propose_bullet_rewrite`, `propose_summary_rewrite`, and `reorder_skill_category_items` to strengthen high-signal items without inventing facts or metrics.\n\
             4. Call `score_ats_alignment` with matched/missing keywords and a 0-100 score.\n\
             5. After your tool calls, write a clear, concise reasoning summary for the user explaining: (a) what you tailored and why, (b) which parts you recommended removing/hiding and why they were unnecessary or could hurt fit, and (c) key remaining gaps or advice.",
            request.job_context.company_name,
            request.job_context.target_role,
            request.job_context.job_description_raw,
            payload.json
        );

        let req_body = ResponsesRequest::new(
            model_id.clone(),
            instructions,
            user_prompt,
            get_tool_declarations(),
        );

        // 4. Stream SSE from POST /v1/responses (retry once on 401 after forced token refresh)
        let outcome = self
            .execute_streaming_responses(
                &config,
                &access_token,
                &req_body,
                &payload.body,
                request.child_cv_id,
                None,
                &events,
            )
            .await;

        match outcome {
            Ok(outcome) => {
                let _ = events.send(AiEvent::RunCompleted {
                    run_id,
                    summary: outcome.summary.clone(),
                });
                Ok(outcome)
            }
            Err(err) => {
                let msg = format!("{err:#}");
                let _ = events.send(AiEvent::RunFailed {
                    run_id,
                    message: msg,
                });
                Err(err)
            }
        }
    }

    async fn rewrite_bullet(
        &self,
        request: InlineRewriteRequest,
        payload: ScrubbedPayload,
        events: AiEventSink,
    ) -> Result<Option<AiProposal>> {
        let run_id = Uuid::new_v4();
        let _ = events.send(AiEvent::RunStarted { run_id });

        // 1. Security Gate
        if let Err(err) = self.verify_security_gate(&payload) {
            let msg = err.to_string();
            let _ = events.send(AiEvent::RunFailed {
                run_id,
                message: msg,
            });
            return Err(err);
        }

        // 2. Ensure valid SIWC access token
        let access_token = match self.ensure_valid_access_token(false).await {
            Ok(token) => token,
            Err(err) => {
                let _ = events.send(AiEvent::RunFailed {
                    run_id,
                    message: err.to_string(),
                });
                return Err(err);
            }
        };

        let Some(bullet) = payload.body.find_bullet(request.bullet_id) else {
            let err = anyhow!("Bullet {} not found in scrubbed payload", request.bullet_id);
            let _ = events.send(AiEvent::RunFailed {
                run_id,
                message: err.to_string(),
            });
            return Err(err);
        };

        let config = self.config();
        let model_id = config.model.trim().to_string();
        if model_id.is_empty() {
            let err = anyhow!("No ChatGPT model is selected. Pick a model in Settings.");
            let _ = events.send(AiEvent::RunFailed {
                run_id,
                message: err.to_string(),
            });
            return Err(err);
        }
        let _ = events.send(AiEvent::Status {
            message: format!("Rewriting bullet via ChatGPT ({model_id})…"),
        });

        let skill_directives = self.compose_skills(&request.skill_ids);
        let instructions = format!(
            "{}\n\nYou are performing an inline single-bullet rewrite. You MUST call the `propose_bullet_rewrite` tool with your grounded proposal.",
            build_system_prompt(&skill_directives)
        );
        let user_prompt = format!(
            "Target Bullet ID: {}\nOriginal Bullet Text: \"{}\"\nUser Instruction: {}\n\nPropose an enhanced rewrite using the propose_bullet_rewrite tool.",
            request.bullet_id, bullet.text, request.instruction
        );

        let bullet_tool: Vec<NamespaceFunctionTool> = get_tool_declarations()
            .into_iter()
            .filter(|t| t.name == "propose_bullet_rewrite")
            .collect();
        let req_body = ResponsesRequest::new(model_id, instructions, user_prompt, bullet_tool);

        let outcome = self
            .execute_streaming_responses(
                &config,
                &access_token,
                &req_body,
                &payload.body,
                request.cv_id,
                Some(request.bullet_id),
                &events,
            )
            .await;

        match outcome {
            Ok(outcome) => {
                let first_proposal = outcome.proposals.into_iter().find(|p| {
                    matches!(
                        p,
                        AiProposal::BulletRewrite { bullet_id, .. } if *bullet_id == request.bullet_id
                    )
                });
                let _ = events.send(AiEvent::RunCompleted {
                    run_id,
                    summary: outcome.summary,
                });
                Ok(first_proposal)
            }
            Err(err) => {
                let msg = format!("{err:#}");
                let _ = events.send(AiEvent::RunFailed {
                    run_id,
                    message: msg,
                });
                Err(err)
            }
        }
    }

    async fn complete_structured_prompt(
        &self,
        system_prompt: String,
        user_prompt: String,
    ) -> Result<String> {
        let access_token = self.ensure_valid_access_token(false).await?;
        let config = self.config();
        let model_id = config.model.trim().to_string();
        if model_id.is_empty() {
            bail!("No ChatGPT model is selected. Pick a model in Settings.");
        }

        let req_body = ResponsesRequest::new(
            model_id,
            system_prompt,
            user_prompt,
            Vec::new(),
        );
        let empty_body = SanitizedCvBody::default();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AiEvent>();
        let outcome = self
            .execute_streaming_responses(
                &config,
                &access_token,
                &req_body,
                &empty_body,
                Uuid::nil(),
                None,
                &tx,
            )
            .await?;
        drop(tx);

        let mut collected = String::new();
        while let Ok(ev) = rx.try_recv() {
            if let AiEvent::TextDelta { delta } = ev {
                collected.push_str(&delta);
            }
        }
        if !collected.trim().is_empty() {
            return Ok(collected);
        }
        if !outcome.summary.trim().is_empty() {
            return Ok(outcome.summary);
        }
        bail!("ChatGPT returned an empty response for structured completion.")
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let config = self.config();
        let base_url = resolve_api_base_url(&config.base_url);
        let token = self.ensure_valid_access_token(false).await?;

        let mut response = self.fetch_models_with_token(&base_url, &token).await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            // Try a single forced refresh on 401 before giving up.
            let refreshed = self.ensure_valid_access_token(true).await?;
            response = self.fetch_models_with_token(&base_url, &refreshed).await?;
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            let (msg, needs_reauth) = format_siwc_api_error(status, &body, None);
            if needs_reauth {
                self.mark_session_needs_reauth();
            }
            bail!("{msg}");
        }

        parse_siwc_models_response(&body)
    }

    fn auth_status(&self) -> ChatGptAuthStatus {
        self.current_session()
            .map(|s| s.to_auth_status())
            .unwrap_or_default()
    }

    async fn sign_in_with_chatgpt(
        &self,
        force_consent: bool,
        fresh_registration: bool,
    ) -> Result<ChatGptAuthStatus> {
        self.sign_in_with_launcher(
            force_consent,
            fresh_registration,
            OAUTH_CALLBACK_TIMEOUT,
            |url| async move {
                crate::platform::open_external_url(&url).map_err(|err| {
                    anyhow!("Could not open your system browser for Sign in with ChatGPT: {err}")
                })
            },
        )
        .await
    }

    async fn sign_out(&self) -> Result<Option<String>> {
        let existing = self.current_session();
        let mut warning: Option<String> = None;

        if let Some(session) = existing.as_ref()
            && let Some(refresh_token) = session
                .refresh_token
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
            && !session.client_id.trim().is_empty()
        {
            let issuer = resolve_auth_issuer(&session.issuer);
            let revocation_endpoint = self
                .fetch_discovery(&issuer)
                .await
                .ok()
                .and_then(|d| d.revocation_endpoint)
                .filter(|u| !u.trim().is_empty())
                .unwrap_or_else(|| format!("{issuer}/oauth/revoke"));

            let form = encode_form(&[
                ("token", refresh_token),
                ("token_type_hint", "refresh_token"),
                ("client_id", &session.client_id),
            ]);

            let mut revoked = false;
            for attempt in 0..3 {
                match self
                    .http_client()
                    .post(&revocation_endpoint)
                    .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(form.clone())
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        revoked = true;
                        break;
                    }
                    Ok(resp) if resp.status().is_server_error() && attempt < 2 => {
                        tokio::time::sleep(Duration::from_millis(250 * (attempt + 1))).await;
                    }
                    Ok(resp) => {
                        tracing::warn!(
                            status = resp.status().as_u16(),
                            "OpenAI token revocation endpoint returned non-200"
                        );
                        break;
                    }
                    Err(err) if attempt < 2 => {
                        tracing::warn!("OpenAI token revocation attempt failed: {err}");
                        tokio::time::sleep(Duration::from_millis(250 * (attempt + 1))).await;
                    }
                    Err(err) => {
                        tracing::warn!("OpenAI token revocation failed: {err}");
                        break;
                    }
                }
            }

            if !revoked {
                warning = Some(
                    "Signed out locally, but OpenAI revocation could not be confirmed (you may be offline)."
                        .to_string(),
                );
            }
        }

        // Always clear local tokens (ext_agent_host_id is preserved by the store).
        self.update_session(None)?;
        Ok(warning)
    }

    fn configure(&self, mut config: AiConfig) -> Result<()> {
        config.base_url = resolve_api_base_url(&config.base_url);
        config.auth_issuer = resolve_auth_issuer(&config.auth_issuer);
        config.model = if config.model.trim().is_empty() {
            DEFAULT_CHATGPT_MODEL.to_string()
        } else {
            config.model.trim().to_string()
        };

        let new_client = build_chatgpt_http_client(config.request_timeout_secs)?;
        if let Ok(mut lock) = self.config.write() {
            *lock = config;
        }
        if let Ok(mut lock) = self.client.write() {
            *lock = new_client;
        }
        Ok(())
    }

    fn config(&self) -> AiConfig {
        self.config.read().map(|c| c.clone()).unwrap_or_default()
    }

    fn is_configured(&self) -> bool {
        self.auth_status().is_ready_for_inference() && self.config().is_configured()
    }
}

impl ChatGptClient {
    #[allow(clippy::too_many_arguments)]
    async fn execute_streaming_responses(
        &self,
        config: &AiConfig,
        initial_token: &str,
        req_body: &ResponsesRequest,
        body: &SanitizedCvBody,
        cv_id: Uuid,
        only_bullet_id: Option<Uuid>,
        events: &AiEventSink,
    ) -> Result<VariationOutcome> {
        let base_url = resolve_api_base_url(&config.base_url);
        let endpoint = format!("{base_url}/responses");

        let mut response = self
            .http_client()
            .post(&endpoint)
            .header(AUTHORIZATION, format!("Bearer {initial_token}"))
            .header("Accept", "text/event-stream")
            .json(req_body)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to contact OpenAI Responses API ({base_url}): {e}"))?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            let refreshed = self.ensure_valid_access_token(true).await?;
            response = self
                .http_client()
                .post(&endpoint)
                .header(AUTHORIZATION, format!("Bearer {refreshed}"))
                .header("Accept", "text/event-stream")
                .json(req_body)
                .send()
                .await
                .map_err(|e| anyhow!("Failed to contact OpenAI Responses API ({base_url}): {e}"))?;
        }

        let status = response.status();
        if !status.is_success() {
            let err_body = response.text().await.unwrap_or_default();
            let (msg, needs_reauth) =
                format_siwc_api_error(status, &err_body, Some(&req_body.model));
            if needs_reauth {
                self.mark_session_needs_reauth();
            }
            bail!("{msg}");
        }

        let outcome = consume_responses_sse(response, body, cv_id, only_bullet_id, events).await;
        if let Err(ref err) = outcome
            && err
                .to_string()
                .contains("authorization expired or was revoked")
        {
            self.mark_session_needs_reauth();
        }
        outcome
    }
}

// ---------------------------------------------------------------------------
// OAuth 2.0 / PKCE / Loopback callback helpers
// ---------------------------------------------------------------------------

struct PkcePair {
    verifier: String,
    challenge: String,
}

impl PkcePair {
    fn generate() -> Self {
        let verifier = generate_urlsafe_secret(32);
        let challenge = compute_pkce_s256_challenge(&verifier);
        Self {
            verifier,
            challenge,
        }
    }
}

/// Computes `BASE64URL-NO-PAD(SHA256(verifier))` per RFC 7636.
pub fn compute_pkce_s256_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn generate_urlsafe_secret(num_bytes: usize) -> String {
    let mut bytes = vec![0u8; num_bytes];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn encode_form(pairs: &[(&str, &str)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        serializer.append_pair(k, v);
    }
    serializer.finish()
}

pub struct AuthorizeUrlParams<'a> {
    pub issuer: &'a str,
    pub redirect_uri: &'a str,
    pub resource: &'a str,
    pub ext_agent_host_id: &'a str,
    pub state: &'a str,
    pub nonce: &'a str,
    pub code_challenge: &'a str,
    pub existing_client_id: Option<&'a str>,
    pub id_token_hint: Option<&'a str>,
    pub login_hint: Option<&'a str>,
    pub force_consent: bool,
}

/// Constructs the official OpenAI Sign in with ChatGPT authorization URL.
///
/// - On initial registration (`existing_client_id == None`): sends
///   `client_id=dynamic_agent_client` and `agent_name_hint=VanguardCV`.
/// - On reauthorization (`existing_client_id == Some(id)`): sends the issued
///   `client_id`, **omits `agent_name_hint`**, and optionally includes
///   `id_token_hint` / `login_hint`.
pub fn build_authorize_url(params: &AuthorizeUrlParams<'_>) -> Result<String> {
    let mut url = Url::parse(&format!(
        "{}/api/accounts/authorize",
        params.issuer.trim_end_matches('/')
    ))
    .context("building OpenAI authorize URL")?;

    {
        let mut q = url.query_pairs_mut();
        if let Some(client_id) = params.existing_client_id.filter(|c| !c.trim().is_empty()) {
            q.append_pair("client_id", client_id);
            if let Some(id_token) = params.id_token_hint.filter(|t| !t.trim().is_empty()) {
                q.append_pair("id_token_hint", id_token);
            }
            if let Some(email) = params.login_hint.filter(|e| !e.trim().is_empty()) {
                q.append_pair("login_hint", email);
            }
        } else {
            q.append_pair("client_id", DYNAMIC_AGENT_CLIENT_ID);
            q.append_pair("agent_name_hint", CHATGPT_AGENT_NAME_HINT);
        }

        q.append_pair("ext_agent_host_id", params.ext_agent_host_id);
        q.append_pair("response_type", "code");
        q.append_pair("redirect_uri", params.redirect_uri);
        q.append_pair("scope", SIWC_SCOPES);
        q.append_pair("resource", params.resource);
        q.append_pair("state", params.state);
        q.append_pair("nonce", params.nonce);
        q.append_pair("code_challenge_method", "S256");
        q.append_pair("code_challenge", params.code_challenge);

        if params.force_consent {
            q.append_pair("prompt", "consent");
        }
    }

    Ok(url.into())
}

#[derive(Debug, Default)]
struct OAuthCallbackParams {
    code: Option<String>,
    state: Option<String>,
    client_id: Option<String>,
    scope: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn wait_for_oauth_callback(
    listener: TcpListener,
    timeout: Duration,
) -> Result<OAuthCallbackParams> {
    tokio::time::timeout(timeout, async move {
        loop {
            let (mut socket, _) = listener
                .accept()
                .await
                .context("accepting loopback OAuth callback connection")?;

            let mut buf = Vec::with_capacity(4096);
            let mut chunk = [0u8; 2048];
            loop {
                let read_res =
                    tokio::time::timeout(Duration::from_secs(5), socket.read(&mut chunk)).await;
                let Ok(Ok(n)) = read_res else {
                    break;
                };
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() >= 16_384 {
                    break;
                }
            }
            if buf.is_empty() {
                continue;
            }

            let request_text = String::from_utf8_lossy(&buf);
            let first_line = request_text.lines().next().unwrap_or_default();
            let target = first_line.split_whitespace().nth(1).unwrap_or("/");

            let Ok(parsed_url) = Url::parse(&format!("http://127.0.0.1{target}")) else {
                write_http_html(
                    &mut socket,
                    "400 Bad Request",
                    "Invalid callback request.",
                )
                .await;
                continue;
            };

            if parsed_url.path() != SIWC_CALLBACK_PATH {
                write_http_html(
                    &mut socket,
                    "404 Not Found",
                    "Waiting for Sign in with ChatGPT callback…",
                )
                .await;
                continue;
            }

            let mut params = OAuthCallbackParams::default();
            for (k, v) in parsed_url.query_pairs() {
                match k.as_ref() {
                    "code" => params.code = Some(v.into_owned()),
                    "state" => params.state = Some(v.into_owned()),
                    "client_id" => params.client_id = Some(v.into_owned()),
                    "scope" => params.scope = Some(v.into_owned()),
                    "error" => params.error = Some(v.into_owned()),
                    "error_description" => params.error_description = Some(v.into_owned()),
                    _ => {}
                }
            }

            let (status_line, body_html) = if params.error.is_some() {
                (
                    "200 OK",
                    "<!doctype html><html><head><meta charset='utf-8'><title>VanguardCV — Sign in canceled</title></head>\
                     <body style='font-family:system-ui,-apple-system,sans-serif;background:#111317;color:#eef1f4;display:flex;align-items:center;justify-content:center;height:100vh;margin:0'>\
                     <div style='max-width:420px;padding:28px;border-radius:10px;background:#191c22;border:1px solid #2b303a;text-align:center'>\
                     <h2 style='margin:0 0 10px'>Sign in was not completed</h2>\
                     <p style='color:#9ba3b0;margin:0'>You can close this tab and return to VanguardCV.</p>\
                     </div></body></html>",
                )
            } else {
                (
                    "200 OK",
                    "<!doctype html><html><head><meta charset='utf-8'><title>VanguardCV — Connected to ChatGPT</title></head>\
                     <body style='font-family:system-ui,-apple-system,sans-serif;background:#111317;color:#eef1f4;display:flex;align-items:center;justify-content:center;height:100vh;margin:0'>\
                     <div style='max-width:420px;padding:28px;border-radius:10px;background:#191c22;border:1px solid #2b303a;text-align:center'>\
                     <h2 style='margin:0 0 10px'>Sign in complete</h2>\
                     <p style='color:#9ba3b0;margin:0'>VanguardCV received your ChatGPT authorization. You can close this browser tab and return to the app.</p>\
                     </div></body></html>",
                )
            };

            write_http_html(&mut socket, status_line, body_html).await;
            return Ok(params);
        }
    })
    .await
    .map_err(|_| anyhow!("Timed out waiting for Sign in with ChatGPT browser callback. Please try again."))?
}

async fn write_http_html(socket: &mut tokio::net::TcpStream, status: &str, html: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
    let _ = socket.shutdown().await;
}

fn is_allowed_asymmetric_alg(alg: Algorithm) -> bool {
    matches!(
        alg,
        Algorithm::RS256
            | Algorithm::RS384
            | Algorithm::RS512
            | Algorithm::PS256
            | Algorithm::PS384
            | Algorithm::PS512
            | Algorithm::ES256
            | Algorithm::ES384
            | Algorithm::EdDSA
    )
}

fn token_needs_refresh(session: &ChatGptSessionRecord, now: DateTime<Utc>) -> bool {
    let Some(expires_at) = session.access_token_expires_at else {
        return true;
    };
    if now >= expires_at {
        return true;
    }
    let within_skew = expires_at - now <= chrono::Duration::seconds(ACCESS_TOKEN_REFRESH_SKEW_SECS);
    if !within_skew {
        return false;
    }
    if let Some(earliest) = session.earliest_refresh_at
        && now < earliest
    {
        return false;
    }
    true
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    let trimmed = raw.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(epoch) = trimmed.parse::<i64>() {
        return DateTime::from_timestamp(epoch, 0);
    }
    None
}

// ---------------------------------------------------------------------------
// OAuth / OIDC wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct OidcDiscovery {
    #[serde(default)]
    jwks_uri: Option<String>,
    #[serde(default)]
    revocation_endpoint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    id_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    earliest_refresh_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthRefreshResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    earliest_refresh_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    sub: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthErrorBody {
    #[serde(default)]
    error: Option<serde_json::Value>,
    #[serde(default)]
    error_description: Option<String>,
    #[serde(default)]
    detail: Option<String>,
}

fn extract_oauth_error_code(body: &str) -> Option<String> {
    let parsed: OAuthErrorBody = serde_json::from_str(body).ok()?;
    match parsed.error {
        Some(serde_json::Value::String(code)) => Some(code),
        Some(serde_json::Value::Object(map)) => {
            map.get("code").and_then(|v| v.as_str()).map(str::to_string)
        }
        _ => None,
    }
}

fn is_fatal_refresh_error(status: reqwest::StatusCode, body: &str) -> bool {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return true;
    }
    let code = extract_oauth_error_code(body)
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        code.as_str(),
        "invalid_grant"
            | "invalid_refresh_token"
            | "token_expired"
            | "refresh_token_expired"
            | "refresh_token_invalidated"
            | "refresh_token_reused"
    )
}

fn format_oauth_error(status: reqwest::StatusCode, body: &str) -> String {
    let parsed: Option<OAuthErrorBody> = serde_json::from_str(body).ok();
    let code = extract_oauth_error_code(body);
    let desc = parsed
        .as_ref()
        .and_then(|e| e.error_description.as_deref().or(e.detail.as_deref()))
        .map(str::trim)
        .filter(|s| !s.is_empty());

    match (code.as_deref(), desc) {
        (Some(c), Some(d)) => format!("ChatGPT sign-in error ({c}): {d}"),
        (Some(c), None) => format!("ChatGPT sign-in error ({c}, HTTP {}).", status.as_u16()),
        (None, Some(d)) => format!("ChatGPT sign-in error (HTTP {}): {d}", status.as_u16()),
        (None, None) => format!("ChatGPT sign-in failed with HTTP {}.", status.as_u16()),
    }
}

// ---------------------------------------------------------------------------
// Models Catalog (`GET /v1/models`)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SiwcModelsResponse {
    #[serde(default)]
    models: Vec<SiwcModelRecord>,
    #[serde(default)]
    data: Vec<LegacyOpenAiModelRecord>,
}

#[derive(Debug, Deserialize)]
struct SiwcModelRecord {
    #[serde(default)]
    slug: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    visibility: Option<String>,
    #[serde(default)]
    context_window: Option<u64>,
    #[serde(default)]
    input_token_limit: Option<u64>,
    #[serde(default)]
    output_token_limit: Option<u64>,
    #[serde(default)]
    default_reasoning_level: Option<String>,
    #[serde(default)]
    supported_reasoning_levels: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct LegacyOpenAiModelRecord {
    id: String,
}

/// Parses OpenAI's SIWC `GET /v1/models` payload, filtering to
/// `visibility == "list"` and preserving exact server ordering.
pub fn parse_siwc_models_response(body: &str) -> Result<Vec<ModelInfo>> {
    let parsed: SiwcModelsResponse =
        serde_json::from_str(body).context("Invalid JSON from OpenAI /v1/models endpoint")?;

    let mut seen = HashSet::new();
    let mut models = Vec::new();

    if !parsed.models.is_empty() {
        for record in parsed.models {
            if let Some(vis) = record.visibility.as_deref()
                && !vis.eq_ignore_ascii_case("list")
            {
                continue;
            }
            let slug = if !record.slug.trim().is_empty() {
                record.slug.trim().to_string()
            } else {
                record.id.as_deref().unwrap_or_default().trim().to_string()
            };
            if slug.is_empty() || !seen.insert(slug.clone()) {
                continue;
            }
            let display_name = record
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| slug.clone());
            let supports_thinking = !record.supported_reasoning_levels.is_empty()
                || record
                    .default_reasoning_level
                    .as_deref()
                    .is_some_and(|l| !l.eq_ignore_ascii_case("none"));

            models.push(ModelInfo {
                id: slug,
                display_name,
                description: record.description.unwrap_or_default().trim().to_string(),
                input_token_limit: record.context_window.or(record.input_token_limit),
                output_token_limit: record.output_token_limit,
                supports_thinking,
            });
        }
        return Ok(models);
    }

    // Graceful fallback if an OpenAI-compatible response returns `data: [{ id }]`.
    for record in parsed.data {
        let id = record.id.trim().to_string();
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        models.push(ModelInfo {
            display_name: id.clone(),
            id,
            description: String::new(),
            input_token_limit: None,
            output_token_limit: None,
            supports_thinking: false,
        });
    }

    Ok(models)
}

// ---------------------------------------------------------------------------
// SIWC-Compliant Responses API Wire Format & Tool Declarations
// ---------------------------------------------------------------------------

/// Strictly SIWC-preview-compliant `POST /v1/responses` payload.
///
/// Intentionally omits all parameters rejected by SIWC preview (`temperature`,
/// `top_p`, `max_output_tokens`, `role: "system"` items, bare top-level
/// function tools, `store: true`, `stream: false`).
#[derive(Debug, Serialize)]
pub struct ResponsesRequest {
    pub model: String,
    pub instructions: String,
    pub input: Vec<ResponsesInputItem>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ResponsesNamespaceTool>,
    pub store: bool,
    pub stream: bool,
}

impl ResponsesRequest {
    pub fn new(
        model: String,
        instructions: String,
        user_prompt: String,
        functions: Vec<NamespaceFunctionTool>,
    ) -> Self {
        let tools = if functions.is_empty() {
            Vec::new()
        } else {
            vec![ResponsesNamespaceTool {
                tool_type: "namespace".to_string(),
                name: TOOL_NAMESPACE_NAME.to_string(),
                description: "Deterministic CV tailoring, visibility, skill reordering, and ATS scoring tools for VanguardCV."
                    .to_string(),
                tools: functions,
            }]
        };

        Self {
            model,
            instructions,
            input: vec![ResponsesInputItem {
                role: "user".to_string(),
                content: user_prompt,
            }],
            tools,
            store: false,
            stream: true,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ResponsesInputItem {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct ResponsesNamespaceTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub name: String,
    pub description: String,
    pub tools: Vec<NamespaceFunctionTool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceFunctionTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Builds the top-level `instructions` string with strict grounding and
/// anti-hallucination rules.
fn build_system_prompt(skill_directives: &str) -> String {
    format!(
        r#"You are VanguardCV AI Assistant, an elite executive career strategist and ATS optimization engine.
Your mission is to tailor the candidate's CV for a specific target job.

CRITICAL ANTI-HALLUCINATION & INTEGRITY DIRECTIVES:
1. NEVER invent, fabricate, or assume degrees, certifications, universities, organizations, job titles, or dates not present in the candidate's CV data.
2. NEVER fabricate numerical metrics or claim accomplishments not grounded in the candidate's provided text.
3. Align terminology and technical keywords from the job description ONLY when the candidate genuinely possesses that skill in their experience or skills section.
4. Interact with the CV exclusively through the provided deterministic tools in the `vanguard_cv` namespace.
5. PRUNE IRRELEVANT OR HARMFUL CONTENT: Actively evaluate every section, timeline entry, bullet, and skill category against the target job description. Use `toggle_item_visibility(item_id, enabled=false, reason=...)` to hide/remove items that are unnecessary, redundant, off-domain, or could dilute or harm the candidate's fit for this role. Always include a specific, constructive `reason`.
6. END-OF-RUN REASONING: After emitting your tool calls, always write a clear, concise final reasoning summary for the user explaining what you tailored, which parts you recommended removing/hiding and why, and any remaining gaps.

=== ACTIVE AI SKILLS ===
{skill_directives}
"#
    )
}

/// Returns the five deterministic CV tailoring function tools (wrapped in the
/// `vanguard_cv` namespace on every `/v1/responses` call).
pub fn get_tool_declarations() -> Vec<NamespaceFunctionTool> {
    vec![
        NamespaceFunctionTool {
            tool_type: "function".to_string(),
            name: "toggle_item_visibility".to_string(),
            description: "Removes (hides from the tailored PDF) any section, timeline entry, bullet, or skill category that is irrelevant, redundant, off-domain, or could harm the candidate's fit for this job description (or re-enables a hidden item that strengthens fit). Always explain why in `reason`."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "item_id": {
                        "type": "string",
                        "description": "UUID of the item to toggle."
                    },
                    "enabled": {
                        "type": "boolean",
                        "description": "True to show the item in this tailored variation, false to hide it."
                    },
                    "reason": {
                        "type": "string",
                        "description": "Short explanation of why this item was shown or hidden."
                    }
                },
                "required": ["item_id", "enabled", "reason"],
                "additionalProperties": false
            }),
        },
        NamespaceFunctionTool {
            tool_type: "function".to_string(),
            name: "propose_bullet_rewrite".to_string(),
            description: "Proposes an enhanced rewrite of a specific bullet point using action verbs and grounded terminology."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "bullet_id": {
                        "type": "string",
                        "description": "UUID of the bullet point to rewrite."
                    },
                    "proposed_text": {
                        "type": "string",
                        "description": "The enhanced bullet text. Never invent metrics or ungrounded technologies."
                    },
                    "rationale": {
                        "type": "string",
                        "description": "Rationale for the rewrite (e.g. alignment to JD requirement, XYZ formula)."
                    }
                },
                "required": ["bullet_id", "proposed_text", "rationale"],
                "additionalProperties": false
            }),
        },
        NamespaceFunctionTool {
            tool_type: "function".to_string(),
            name: "propose_summary_rewrite".to_string(),
            description: "Proposes a rewritten summary paragraph tailored to the target role."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "section_id": {
                        "type": "string",
                        "description": "UUID of the summary section."
                    },
                    "proposed_text": {
                        "type": "string",
                        "description": "The tailored summary text."
                    }
                },
                "required": ["section_id", "proposed_text"],
                "additionalProperties": false
            }),
        },
        NamespaceFunctionTool {
            tool_type: "function".to_string(),
            name: "reorder_skill_category_items".to_string(),
            description: "Reorders items in a skill category so matching high-priority skills appear first."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "category_id": {
                        "type": "string",
                        "description": "UUID of the skill category item."
                    },
                    "reordered_items": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "The items reordered with matching terms first. Do not add or remove skills."
                    }
                },
                "required": ["category_id", "reordered_items"],
                "additionalProperties": false
            }),
        },
        NamespaceFunctionTool {
            tool_type: "function".to_string(),
            name: "score_ats_alignment".to_string(),
            description: "Scores the ATS keyword alignment of the CV against the target job description."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "matched_keywords": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Keywords from the job description present in the CV."
                    },
                    "missing_keywords": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Keywords from the job description missing from the CV."
                    },
                    "score": {
                        "type": "integer",
                        "description": "ATS alignment score between 0 and 100."
                    }
                },
                "required": ["matched_keywords", "missing_keywords", "score"],
                "additionalProperties": false
            }),
        },
    ]
}

// ---------------------------------------------------------------------------
// Responses API SSE Streaming Consumer
// ---------------------------------------------------------------------------

async fn consume_responses_sse(
    response: reqwest::Response,
    body: &SanitizedCvBody,
    cv_id: Uuid,
    only_bullet_id: Option<Uuid>,
    events: &AiEventSink,
) -> Result<VariationOutcome> {
    let mut stream = response.bytes_stream().eventsource();
    let mut summary_text = String::new();
    let mut proposals = Vec::new();
    let mut ats_score: Option<AtsScore> = None;
    let mut processed_call_ids: HashSet<String> = HashSet::new();
    let mut saw_completed = false;

    while let Some(event_result) = stream.next().await {
        let ev = match event_result {
            Ok(ev) => ev,
            Err(err) => bail!("OpenAI SSE stream error: {err}"),
        };

        let raw_data = ev.data.trim();
        if raw_data.is_empty() || raw_data == "[DONE]" {
            continue;
        }

        let payload: serde_json::Value = match serde_json::from_str(raw_data) {
            Ok(val) => val,
            Err(err) => {
                tracing::debug!("Skipping non-JSON SSE event: {err}");
                continue;
            }
        };

        let event_type = payload
            .get("type")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(ev.event.as_str());

        match event_type {
            "response.output_text.delta" => {
                if let Some(delta) = payload.get("delta").and_then(|v| v.as_str())
                    && !delta.is_empty()
                {
                    summary_text.push_str(delta);
                    let _ = events.send(AiEvent::TextDelta {
                        delta: delta.to_string(),
                    });
                }
            }
            "response.output_item.done" => {
                if let Some(item) = payload.get("item") {
                    handle_output_item(
                        item,
                        body,
                        only_bullet_id,
                        &mut processed_call_ids,
                        &mut proposals,
                        &mut ats_score,
                        events,
                    );
                }
            }
            "response.completed" => {
                saw_completed = true;
                if let Some(output_items) = payload
                    .get("response")
                    .and_then(|r| r.get("output"))
                    .and_then(|o| o.as_array())
                {
                    for item in output_items {
                        handle_output_item(
                            item,
                            body,
                            only_bullet_id,
                            &mut processed_call_ids,
                            &mut proposals,
                            &mut ats_score,
                            events,
                        );
                    }
                }
            }
            "response.incomplete" => {
                let reason = payload
                    .get("response")
                    .and_then(|r| r.get("incomplete_details"))
                    .and_then(|d| d.get("reason"))
                    .and_then(|r| r.as_str())
                    .unwrap_or("unknown reason");
                bail!("ChatGPT response ended incomplete ({reason}). Please retry.");
            }
            "response.failed" => {
                let err_obj = payload
                    .get("response")
                    .and_then(|r| r.get("error"))
                    .or_else(|| payload.get("error"));
                let (msg, _) = format_siwc_structured_error(err_obj, None);
                bail!("{msg}");
            }
            "error" => {
                let err_obj = payload.get("error").or(Some(&payload));
                let (msg, _) = format_siwc_structured_error(err_obj, None);
                bail!("{msg}");
            }
            _ => {}
        }
    }

    // Per OpenAI SIWC spec: treat inference as successful ONLY after `response.completed`.
    if !saw_completed {
        bail!(
            "ChatGPT stream disconnected before completion (`response.completed` was not received). Please retry."
        );
    }

    let summary = if only_bullet_id.is_some() {
        if proposals.is_empty() {
            "ChatGPT completed without proposing a bullet rewrite.".to_string()
        } else {
            "Generated inline bullet rewrite via ChatGPT.".to_string()
        }
    } else {
        synthesize_tailoring_summary(
            &summary_text,
            &proposals,
            ats_score.as_ref(),
            body,
            "ChatGPT",
        )
    };

    Ok(VariationOutcome {
        child_cv_id: cv_id,
        proposals,
        ats_score,
        summary,
    })
}

#[allow(clippy::too_many_arguments)]
fn handle_output_item(
    item: &serde_json::Value,
    body: &SanitizedCvBody,
    only_bullet_id: Option<Uuid>,
    processed_call_ids: &mut HashSet<String>,
    proposals: &mut Vec<AiProposal>,
    ats_score: &mut Option<AtsScore>,
    events: &AiEventSink,
) {
    let item_type = item
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if item_type != "function_call" {
        return;
    }

    if let Some(call_id) = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        && !processed_call_ids.insert(call_id.to_string())
    {
        return;
    }

    let Some(raw_name) = item.get("name").and_then(|v| v.as_str()) else {
        return;
    };
    // Strip optional namespace prefix if a model qualifies `vanguard_cv.propose_bullet_rewrite`.
    let name = raw_name
        .strip_prefix(&format!("{TOOL_NAMESPACE_NAME}."))
        .or_else(|| raw_name.strip_prefix(&format!("{TOOL_NAMESPACE_NAME}__")))
        .unwrap_or(raw_name);

    let args: serde_json::Value = match item.get("arguments") {
        Some(serde_json::Value::String(raw_json)) => match serde_json::from_str(raw_json) {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!("Skipping malformed JSON arguments for tool `{name}`: {err}");
                return;
            }
        },
        Some(other) => other.clone(),
        None => serde_json::json!({}),
    };

    let _ = events.send(AiEvent::ToolCall {
        name: name.to_string(),
        arguments: args.clone(),
    });

    match parse_tool_call(name, &args, body) {
        Ok(ToolCallResult::Proposal(proposal)) => {
            if let Some(target_id) = only_bullet_id {
                let matches_target = matches!(
                    proposal,
                    AiProposal::BulletRewrite { bullet_id, .. } if bullet_id == target_id
                );
                if !matches_target {
                    return;
                }
            }
            let _ = events.send(AiEvent::Proposal {
                proposal: proposal.clone(),
            });
            proposals.push(proposal);
        }
        Ok(ToolCallResult::Score(score)) => {
            let _ = events.send(AiEvent::AtsScore {
                score: score.clone(),
            });
            *ats_score = Some(score);
        }
        Err(err) => {
            tracing::warn!("Skipping invalid ChatGPT tool call `{name}`: {err}");
        }
    }
}

// ---------------------------------------------------------------------------
// Tool call validation & proposal mapping
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ToolCallResult {
    Proposal(AiProposal),
    Score(AtsScore),
}

/// Maps a validated tool call into an [`AiProposal`] or [`AtsScore`].
pub fn parse_tool_call(
    name: &str,
    args: &serde_json::Value,
    body: &SanitizedCvBody,
) -> Result<ToolCallResult> {
    match name {
        "toggle_item_visibility" => {
            let item_id_str = args
                .get("item_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing or invalid item_id in toggle_item_visibility"))?;
            let item_id = Uuid::parse_str(item_id_str)
                .map_err(|e| anyhow!("invalid UUID for item_id: {e}"))?;
            let enabled = args
                .get("enabled")
                .and_then(|v| v.as_bool())
                .ok_or_else(|| {
                    anyhow!("missing or invalid enabled boolean in toggle_item_visibility")
                })?;
            let reason = args
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("No reason provided")
                .to_string();

            if !body_contains_item(body, item_id) {
                bail!("toggle_item_visibility targeted an item that is not in the CV: {item_id}");
            }

            Ok(ToolCallResult::Proposal(AiProposal::ToggleVisibility {
                item_id,
                enabled,
                reason,
            }))
        }
        "propose_bullet_rewrite" => {
            let bullet_id_str = args
                .get("bullet_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing or invalid bullet_id in propose_bullet_rewrite"))?;
            let bullet_id = Uuid::parse_str(bullet_id_str)
                .map_err(|e| anyhow!("invalid UUID for bullet_id: {e}"))?;
            let proposed = args
                .get("proposed_text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing proposed_text in propose_bullet_rewrite"))?
                .to_string();
            let rationale = args
                .get("rationale")
                .and_then(|v| v.as_str())
                .unwrap_or("Tailored for role")
                .to_string();

            let original = body
                .find_bullet(bullet_id)
                .map(|b| b.text.clone())
                .ok_or_else(|| {
                    anyhow!("propose_bullet_rewrite targeted unknown bullet {bullet_id}")
                })?;

            if proposed.contains("[REDACTED_PII_") {
                bail!(
                    "propose_bullet_rewrite returned a redaction token; refusing to apply it to the local CV"
                );
            }

            Ok(ToolCallResult::Proposal(AiProposal::BulletRewrite {
                bullet_id,
                original,
                proposed,
                rationale,
            }))
        }
        "propose_summary_rewrite" => {
            let section_id_str =
                args.get("section_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        anyhow!("missing or invalid section_id in propose_summary_rewrite")
                    })?;
            let section_id = Uuid::parse_str(section_id_str)
                .map_err(|e| anyhow!("invalid UUID for section_id: {e}"))?;
            let proposed = args
                .get("proposed_text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing proposed_text in propose_summary_rewrite"))?
                .to_string();

            let original = body
                .section(section_id)
                .and_then(|s| match &s.kind {
                    SectionKind::Summary { text } => Some(text.clone()),
                    _ => None,
                })
                .ok_or_else(|| {
                    anyhow!("propose_summary_rewrite targeted unknown summary section {section_id}")
                })?;

            if proposed.contains("[REDACTED_PII_") {
                bail!(
                    "propose_summary_rewrite returned a redaction token; refusing to apply it to the local CV"
                );
            }

            Ok(ToolCallResult::Proposal(AiProposal::SummaryRewrite {
                section_id,
                original,
                proposed,
            }))
        }
        "reorder_skill_category_items" => {
            let category_id_str = args
                .get("category_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    anyhow!("missing or invalid category_id in reorder_skill_category_items")
                })?;
            let category_id = Uuid::parse_str(category_id_str)
                .map_err(|e| anyhow!("invalid UUID for category_id: {e}"))?;
            let items: Vec<String> = args
                .get("reordered_items")
                .and_then(|v| v.as_array())
                .ok_or_else(|| {
                    anyhow!(
                        "missing or invalid reordered_items array in reorder_skill_category_items"
                    )
                })?
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();

            let mut existing_items: Option<&Vec<String>> = None;
            for section in &body.sections {
                if let SectionKind::CategorizedInlineList { categories } = &section.kind
                    && let Some(category) = categories
                        .iter()
                        .find(|category| category.id == category_id)
                {
                    existing_items = Some(&category.items);
                    break;
                }
            }
            let Some(existing_items) = existing_items else {
                bail!("reorder_skill_category_items targeted unknown skill category {category_id}");
            };

            let mut reordered = items.clone();
            reordered.sort();
            let mut expected = existing_items.clone();
            expected.sort();
            if reordered != expected {
                bail!(
                    "reorder_skill_category_items must be a permutation of the existing skills (no additions or removals)"
                );
            }

            if items.iter().any(|item| item.contains("[REDACTED_PII_")) {
                bail!(
                    "reorder_skill_category_items returned a redaction token; refusing to apply it to the local CV"
                );
            }

            Ok(ToolCallResult::Proposal(AiProposal::SkillReorder {
                category_id,
                items,
            }))
        }
        "score_ats_alignment" => {
            let matched_keywords = args
                .get("matched_keywords")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            let missing_keywords = args
                .get("missing_keywords")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            let score = args
                .get("score")
                .and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_f64().map(|f| f.round().clamp(0.0, 100.0) as u64))
                })
                .map(|v| v.min(100) as u8)
                .unwrap_or(0);

            Ok(ToolCallResult::Score(AtsScore {
                matched_keywords,
                missing_keywords,
                score,
            }))
        }
        other => bail!("Unknown tool call: {other}"),
    }
}

fn body_contains_item(body: &SanitizedCvBody, id: ItemId) -> bool {
    body.sections.iter().any(|section| {
        if section.id == id {
            return true;
        }
        match &section.kind {
            SectionKind::Summary { .. } => false,
            SectionKind::TimelineEntries { entries } => entries
                .iter()
                .any(|entry| entry.id == id || entry.bullets.iter().any(|bullet| bullet.id == id)),
            SectionKind::CategorizedInlineList { categories } => {
                categories.iter().any(|category| category.id == id)
            }
            SectionKind::SimpleBulletList { items } => items.iter().any(|item| item.id == id),
        }
    })
}

/// Returns a concise human-readable label for any item UUID in a scrubbed CV body.
pub fn describe_item_in_body(body: &SanitizedCvBody, id: ItemId) -> String {
    for section in &body.sections {
        if section.id == id {
            return format!("Section \"{}\"", section.title);
        }
        match &section.kind {
            SectionKind::Summary { .. } => {}
            SectionKind::TimelineEntries { entries } => {
                for entry in entries {
                    if entry.id == id {
                        let role = entry
                            .secondary_heading
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty());
                        return match role {
                            Some(r) => format!("Entry \"{} — {r}\"", entry.primary_heading),
                            None => format!("Entry \"{}\"", entry.primary_heading),
                        };
                    }
                    for bullet in &entry.bullets {
                        if bullet.id == id {
                            return format!("Bullet \"{}\"", truncate_summary_text(&bullet.text, 76));
                        }
                    }
                }
            }
            SectionKind::CategorizedInlineList { categories } => {
                for cat in categories {
                    if cat.id == id {
                        return format!("Skill category \"{}\"", cat.category_label);
                    }
                }
            }
            SectionKind::SimpleBulletList { items } => {
                for item in items {
                    if item.id == id {
                        return format!("Item \"{}\"", truncate_summary_text(&item.text, 76));
                    }
                }
            }
        }
    }
    format!("Item {id}")
}

fn truncate_summary_text(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_string()
    } else {
        let mut s: String = trimmed.chars().take(max_chars).collect();
        s.push('…');
        s
    }
}

/// Builds a clear, user-facing end-of-run reasoning summary combining the model's
/// narrative explanation with a structured breakdown of proposed removals,
/// rewrites, and ATS keyword gaps when needed.
pub fn synthesize_tailoring_summary(
    model_text: &str,
    proposals: &[AiProposal],
    ats_score: Option<&AtsScore>,
    body: &SanitizedCvBody,
    provider_label: &str,
) -> String {
    let trimmed_model = model_text.trim();
    let removals: Vec<(String, String)> = proposals
        .iter()
        .filter_map(|p| match p {
            AiProposal::ToggleVisibility {
                item_id,
                enabled: false,
                reason,
            } => Some((describe_item_in_body(body, *item_id), reason.trim().to_string())),
            _ => None,
        })
        .collect();
    let restores: Vec<(String, String)> = proposals
        .iter()
        .filter_map(|p| match p {
            AiProposal::ToggleVisibility {
                item_id,
                enabled: true,
                reason,
            } => Some((describe_item_in_body(body, *item_id), reason.trim().to_string())),
            _ => None,
        })
        .collect();
    let rewrite_count = proposals
        .iter()
        .filter(|p| {
            matches!(
                p,
                AiProposal::BulletRewrite { .. } | AiProposal::SummaryRewrite { .. }
            )
        })
        .count();
    let reorder_count = proposals
        .iter()
        .filter(|p| matches!(p, AiProposal::SkillReorder { .. }))
        .count();

    if !trimmed_model.is_empty() {
        // Keep the model's own reasoning; if removals were proposed, append a concise
        // itemized removal checklist so every hidden part and its rationale is unmistakable.
        if removals.is_empty() {
            return trimmed_model.to_string();
        }
        let mut out = String::from(trimmed_model);
        out.push_str("\n\nRecommended removals (hidden from tailored PDF):");
        for (label, reason) in &removals {
            if reason.is_empty() {
                out.push_str(&format!("\n• {label}"));
            } else {
                out.push_str(&format!("\n• {label} — {reason}"));
            }
        }
        return out;
    }

    // Fallback synthesis when the model emitted deterministic tool calls without trailing prose.
    let mut sections: Vec<String> = Vec::new();
    let mut parts: Vec<String> = Vec::new();
    if rewrite_count > 0 {
        parts.push(format!(
            "{rewrite_count} {}",
            if rewrite_count == 1 { "rewrite" } else { "rewrites" }
        ));
    }
    if !removals.is_empty() {
        parts.push(format!(
            "{} recommended {}",
            removals.len(),
            if removals.len() == 1 { "removal" } else { "removals" }
        ));
    }
    if !restores.is_empty() {
        parts.push(format!(
            "{} restored {}",
            restores.len(),
            if restores.len() == 1 { "item" } else { "items" }
        ));
    }
    if reorder_count > 0 {
        parts.push(format!(
            "{reorder_count} skill {}",
            if reorder_count == 1 { "reorder" } else { "reorders" }
        ));
    }
    let header = if parts.is_empty() {
        format!("Tailoring analysis complete via {provider_label} (no changes required for this role).")
    } else {
        let score_suffix = ats_score
            .map(|s| format!(" · Estimated ATS alignment: {}%", s.score))
            .unwrap_or_default();
        format!(
            "Tailored via {provider_label}: {}.{score_suffix}",
            parts.join(", ")
        )
    };
    sections.push(header);

    if !removals.is_empty() {
        let mut block = String::from("Recommended removals (hidden from tailored PDF):");
        for (label, reason) in &removals {
            if reason.is_empty() {
                block.push_str(&format!("\n• {label}"));
            } else {
                block.push_str(&format!("\n• {label} — {reason}"));
            }
        }
        sections.push(block);
    }

    if !restores.is_empty() {
        let mut block = String::from("Recommended items to re-include:");
        for (label, reason) in &restores {
            if reason.is_empty() {
                block.push_str(&format!("\n• {label}"));
            } else {
                block.push_str(&format!("\n• {label} — {reason}"));
            }
        }
        sections.push(block);
    }

    let rewrite_reasons: Vec<String> = proposals
        .iter()
        .filter_map(|p| match p {
            AiProposal::BulletRewrite { rationale, .. } if !rationale.trim().is_empty() => {
                Some(rationale.trim().to_string())
            }
            _ => None,
        })
        .take(5)
        .collect();
    if !rewrite_reasons.is_empty() {
        let mut block = String::from("Key tailoring improvements:");
        for reason in rewrite_reasons {
            block.push_str(&format!("\n• {reason}"));
        }
        sections.push(block);
    }

    if let Some(score) = ats_score
        && !score.missing_keywords.is_empty()
    {
        let gaps: Vec<&str> = score
            .missing_keywords
            .iter()
            .take(8)
            .map(String::as_str)
            .collect();
        sections.push(format!(
            "Remaining job-description keywords not present in your CV: {}.",
            gaps.join(", ")
        ));
    }

    sections.join("\n\n")
}

// ---------------------------------------------------------------------------
// SIWC Error Mapping (HTTP + mid-stream `response.failed`)
// ---------------------------------------------------------------------------

/// Translates SIWC HTTP error bodies (`{"error": ...}` or `{"detail": ...}`) into
/// actionable messages plus a boolean indicating whether local tokens need reauth.
pub fn format_siwc_api_error(
    status: reqwest::StatusCode,
    body: &str,
    model: Option<&str>,
) -> (String, bool) {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(err_obj) = json.get("error") {
            let (msg, needs_reauth) = format_siwc_structured_error(Some(err_obj), model);
            return (
                msg,
                needs_reauth || status == reqwest::StatusCode::UNAUTHORIZED,
            );
        }
        if let Some(detail) = json
            .get("detail")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if status == reqwest::StatusCode::UNAUTHORIZED {
                return (
                    format!(
                        "Your ChatGPT authorization expired or was revoked ({detail}). Open Settings and click \"Continue with ChatGPT\" to sign in again."
                    ),
                    true,
                );
            }
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS
                || crate::ai::is_ai_limit_error(detail)
            {
                return (
                    format!(
                        "API / Usage limit reached ({detail}). Wait briefly, switch models in Settings, or review your plan at {CHATGPT_USAGE_SETTINGS_URL}."
                    ),
                    false,
                );
            }
            return (
                format!("OpenAI API error (HTTP {}): {detail}", status.as_u16()),
                false,
            );
        }
    }

    match status {
        reqwest::StatusCode::UNAUTHORIZED => (
            "Your ChatGPT authorization expired or was revoked. Open Settings and click \"Continue with ChatGPT\" to sign in again."
                .to_string(),
            true,
        ),
        reqwest::StatusCode::FORBIDDEN => (
            "Your ChatGPT account is not eligible for Sign in with ChatGPT plan usage (requires an eligible ChatGPT Plus, Pro, Go, or Business plan)."
                .to_string(),
            false,
        ),
        reqwest::StatusCode::TOO_MANY_REQUESTS => (
            format!(
                "API / Usage limit reached. Wait briefly, switch models in Settings, or review your plan at {CHATGPT_USAGE_SETTINGS_URL}."
            ),
            false,
        ),
        reqwest::StatusCode::SERVICE_UNAVAILABLE => (
            "ChatGPT plan usage is temporarily unavailable. Your sign-in is intact — please retry in a moment."
                .to_string(),
            false,
        ),
        reqwest::StatusCode::NOT_FOUND => (
            match model {
                Some(m) => format!(
                    "Model '{m}' is not available on your ChatGPT plan. Refresh and pick another model in Settings."
                ),
                None => "Requested OpenAI endpoint or model was not found (HTTP 404).".to_string(),
            },
            false,
        ),
        _ => (
            format!("OpenAI API returned HTTP {}.", status.as_u16()),
            false,
        ),
    }
}

fn format_siwc_structured_error(
    err_obj: Option<&serde_json::Value>,
    model: Option<&str>,
) -> (String, bool) {
    let code = err_obj
        .and_then(|e| e.get("code"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let message = err_obj
        .and_then(|e| e.get("message"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let param = err_obj
        .and_then(|e| e.get("param"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    match code {
        "subscription_sharing_usage_limit_exceeded"
        | "rate_limit_exceeded"
        | "insufficient_quota" => {
            let extra = message.map(|m| format!(" ({m})")).unwrap_or_default();
            (
                format!(
                    "API / Usage limit reached{extra}. Wait briefly, switch models in Settings, or review your plan at {CHATGPT_USAGE_SETTINGS_URL}."
                ),
                false,
            )
        }
        "subscription_sharing_usage_unavailable" => (
            "ChatGPT plan usage is temporarily unavailable. Your sign-in is intact — please retry in a moment."
                .to_string(),
            false,
        ),
        "subscription_sharing_user_not_eligible" => (
            "Your ChatGPT account is not eligible for Sign in with ChatGPT plan usage (requires an eligible ChatGPT Plus, Pro, Go, or Business plan)."
                .to_string(),
            false,
        ),
        "subscription_sharing_invalid_user" | "chatpass_v2_scope_not_authorized" => (
            "Your ChatGPT authorization expired or was revoked. Open Settings and click \"Continue with ChatGPT\" to sign in again."
                .to_string(),
            true,
        ),
        "subscription_sharing_unsupported_capability" => {
            let detail = match (param, message) {
                (Some(p), Some(m)) => format!("unsupported parameter `{p}`: {m}"),
                (Some(p), None) => format!("unsupported parameter `{p}`"),
                (None, Some(m)) => m.to_string(),
                (None, None) => "unsupported request parameter".to_string(),
            };
            (
                format!("OpenAI SIWC rejected the request ({detail})."),
                false,
            )
        }
        "model_not_found" => (
            match model {
                Some(m) => format!(
                    "Model '{m}' is not available on your ChatGPT plan. Refresh and select another model in Settings."
                ),
                None => "Selected model is not available on your ChatGPT plan.".to_string(),
            },
            false,
        ),
        _ if message.is_some_and(crate::ai::is_ai_limit_error) => {
            let m = message.unwrap_or_default();
            (
                format!(
                    "API / Usage limit reached ({m}). Wait briefly, switch models in Settings, or review your plan at {CHATGPT_USAGE_SETTINGS_URL}."
                ),
                false,
            )
        }
        _ => match (code, message) {
            (c, Some(m)) if !c.is_empty() => (format!("OpenAI error ({c}): {m}"), false),
            (_, Some(m)) => (format!("OpenAI error: {m}"), false),
            (c, None) if !c.is_empty() => (format!("OpenAI error ({c})."), false),
            _ => ("OpenAI Responses API returned an unexpected error.".to_string(), false),
        },
    }
}

// ---------------------------------------------------------------------------
// Comprehensive Unit & Local Mock Server Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        CHATGPT_PLAN_USAGE_SCOPE,
        BulletItem, CvSection, JobContext, PiiContactHeader, PrivacyGuard, SkillCategoryItem,
        TimelineEntry,
    };
    use crate::credentials::InMemoryChatGptStore;
    use crate::privacy::scrubber::PiiScrubber;
    use jsonwebtoken::{EncodingKey, Header};
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use tokio::sync::mpsc::unbounded_channel;

    /// Helper: reads a full HTTP request (headers + Content-Length body) before
    /// responding so Windows TCP sockets never reset while `reqwest` writes.
    async fn read_http_request(socket: &mut tokio::net::TcpStream) -> (String, String, String) {
        let mut buf = Vec::with_capacity(8192);
        let mut chunk = [0u8; 4096];
        let mut header_end = None;
        let mut content_length = 0usize;

        loop {
            let n = socket.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if header_end.is_none()
                && let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n")
            {
                header_end = Some(pos + 4);
                let header_text = String::from_utf8_lossy(&buf[..pos]);
                for line in header_text.lines() {
                    if let Some((k, v)) = line.split_once(':')
                        && k.trim().eq_ignore_ascii_case("content-length")
                    {
                        content_length = v.trim().parse().unwrap_or(0);
                    }
                }
            }
            if let Some(end) = header_end
                && buf.len() >= end + content_length
            {
                break;
            }
        }

        let end = header_end.unwrap_or(buf.len());
        let header_str = String::from_utf8_lossy(&buf[..end]).to_string();
        let first_line = header_str.lines().next().unwrap_or_default().to_string();
        let body = String::from_utf8_lossy(&buf[end..]).to_string();
        (first_line, header_str, body)
    }

    fn test_body_and_pii() -> (SanitizedCvBody, PiiContactHeader, Uuid, Uuid, Uuid) {
        let bullet_id = Uuid::new_v4();
        let summary_id = Uuid::new_v4();
        let skill_cat_id = Uuid::new_v4();

        let pii = PiiContactHeader {
            full_name: "Alex Morgan".to_string(),
            email: Some("alex.morgan@example.com".to_string()),
            phone: Some("+1 555 010 4242".to_string()),
            ..Default::default()
        };

        let body = SanitizedCvBody {
            sections: vec![
                CvSection {
                    id: summary_id,
                    origin_item_id: None,
                    title: "SUMMARY".to_string(),
                    enabled: true,
                    kind: SectionKind::Summary {
                        text: "Offensive security researcher focusing on cloud security."
                            .to_string(),
                    },
                },
                CvSection {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    title: "EXPERIENCE".to_string(),
                    enabled: true,
                    kind: SectionKind::TimelineEntries {
                        entries: vec![TimelineEntry {
                            id: Uuid::new_v4(),
                            origin_item_id: None,
                            enabled: true,
                            primary_heading: "CrowdStrike".to_string(),
                            secondary_heading: Some("Security Analyst".to_string()),
                            start_date: "Apr 2020".to_string(),
                            end_date: Some("Feb 2021".to_string()),
                            is_current: false,
                            bullets: vec![BulletItem {
                                id: bullet_id,
                                origin_item_id: None,
                                enabled: true,
                                text: "Automated detection triage in Python.".to_string(),
                            }],
                            tags: vec![],
                        }],
                    },
                },
                CvSection {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    title: "SKILLS".to_string(),
                    enabled: true,
                    kind: SectionKind::CategorizedInlineList {
                        categories: vec![SkillCategoryItem {
                            id: skill_cat_id,
                            origin_item_id: None,
                            enabled: true,
                            category_label: "Languages".to_string(),
                            separator: ", ".to_string(),
                            items: vec!["Rust".to_string(), "Python".to_string(), "Go".to_string()],
                        }],
                    },
                },
            ],
        };

        (body, pii, bullet_id, summary_id, skill_cat_id)
    }

    #[test]
    fn authorize_url_distinguishes_initial_registration_from_reauth() -> Result<()> {
        let initial = build_authorize_url(&AuthorizeUrlParams {
            issuer: CHATGPT_AUTH_ISSUER,
            redirect_uri: "http://127.0.0.1:43210/auth/callback",
            resource: CHATGPT_API_BASE_URL,
            ext_agent_host_id: "urn:uuid:11111111-2222-4333-8444-555555555555",
            state: "st-1",
            nonce: "nn-1",
            code_challenge: "cc-1",
            existing_client_id: None,
            id_token_hint: None,
            login_hint: None,
            force_consent: false,
        })?;
        let parsed_initial = Url::parse(&initial)?;
        let q_init: std::collections::HashMap<_, _> = parsed_initial.query_pairs().collect();
        assert_eq!(
            q_init.get("client_id").map(|v| v.as_ref()),
            Some(DYNAMIC_AGENT_CLIENT_ID)
        );
        assert_eq!(
            q_init.get("agent_name_hint").map(|v| v.as_ref()),
            Some(CHATGPT_AGENT_NAME_HINT)
        );
        assert_eq!(
            q_init.get("redirect_uri").map(|v| v.as_ref()),
            Some("http://127.0.0.1:43210/auth/callback")
        );
        assert_eq!(
            q_init.get("code_challenge_method").map(|v| v.as_ref()),
            Some("S256")
        );
        assert!(!q_init.contains_key("prompt"));

        // Reauthorization MUST reuse issued client_id and OMIT agent_name_hint.
        let reauth = build_authorize_url(&AuthorizeUrlParams {
            issuer: CHATGPT_AUTH_ISSUER,
            redirect_uri: "http://127.0.0.1:43210/auth/callback",
            resource: CHATGPT_API_BASE_URL,
            ext_agent_host_id: "urn:uuid:11111111-2222-4333-8444-555555555555",
            state: "st-2",
            nonce: "nn-2",
            code_challenge: "cc-2",
            existing_client_id: Some("oaiapp_abc123"),
            id_token_hint: Some("old.id.token"),
            login_hint: Some("alex@example.com"),
            force_consent: true,
        })?;
        let parsed_reauth = Url::parse(&reauth)?;
        let q_reauth: std::collections::HashMap<_, _> = parsed_reauth.query_pairs().collect();
        assert_eq!(
            q_reauth.get("client_id").map(|v| v.as_ref()),
            Some("oaiapp_abc123")
        );
        assert!(!q_reauth.contains_key("agent_name_hint"));
        assert_eq!(
            q_reauth.get("id_token_hint").map(|v| v.as_ref()),
            Some("old.id.token")
        );
        assert_eq!(
            q_reauth.get("login_hint").map(|v| v.as_ref()),
            Some("alex@example.com")
        );
        assert_eq!(q_reauth.get("prompt").map(|v| v.as_ref()), Some("consent"));
        Ok(())
    }

    #[test]
    fn responses_request_strictly_obeys_siwc_preview_limitations() {
        let req = ResponsesRequest::new(
            "gpt-5.4".to_string(),
            "System instructions".to_string(),
            "User prompt".to_string(),
            get_tool_declarations(),
        );
        let value = serde_json::to_value(&req).unwrap();
        assert_eq!(value["store"], serde_json::json!(false));
        assert_eq!(value["stream"], serde_json::json!(true));
        assert_eq!(
            value["instructions"],
            serde_json::json!("System instructions")
        );
        assert!(value["input"].is_array());
        assert_eq!(value["input"][0]["role"], serde_json::json!("user"));
        assert_eq!(value["tools"][0]["type"], serde_json::json!("namespace"));
        assert_eq!(
            value["tools"][0]["name"],
            serde_json::json!(TOOL_NAMESPACE_NAME)
        );
        assert_eq!(value["tools"][0]["tools"].as_array().unwrap().len(), 5);

        // Forbidden parameters MUST NOT exist in serialized JSON
        for forbidden in [
            "temperature",
            "top_p",
            "max_output_tokens",
            "max_tool_calls",
            "background",
            "conversation",
            "metadata",
            "moderation",
            "previous_response_id",
            "prompt_cache_retention",
            "truncation",
            "user",
        ] {
            assert!(
                value.get(forbidden).is_none(),
                "SIWC preview forbids parameter `{forbidden}`"
            );
        }
    }

    #[test]
    fn parse_siwc_models_filters_visibility_and_preserves_order() -> Result<()> {
        let raw = r#"{
            "models": [
                {
                    "slug": "gpt-5.4",
                    "display_name": "GPT-5.4",
                    "description": "Flagship frontier model.",
                    "visibility": "list",
                    "context_window": 272000,
                    "default_reasoning_level": "medium"
                },
                {
                    "slug": "internal-hidden-model",
                    "display_name": "Hidden",
                    "visibility": "hide"
                },
                {
                    "slug": "gpt-5.4-mini",
                    "display_name": "GPT-5.4 Mini",
                    "description": "Fast everyday model.",
                    "visibility": "list",
                    "context_window": 272000,
                    "default_reasoning_level": "none"
                }
            ]
        }"#;

        let models = parse_siwc_models_response(raw)?;
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-5.4");
        assert_eq!(models[0].display_name, "GPT-5.4");
        assert!(models[0].supports_thinking);
        assert_eq!(models[0].input_token_limit, Some(272_000));
        assert_eq!(models[1].id, "gpt-5.4-mini");
        assert!(!models[1].supports_thinking);
        Ok(())
    }

    #[tokio::test]
    async fn full_oauth_pkce_jwks_signin_refresh_and_revocation_flow() -> Result<()> {
        // Generate an ephemeral Ed25519 keypair to sign real OIDC ID tokens + serve real JWKS.
        let rng = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let keypair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let pub_b64 = URL_SAFE_NO_PAD.encode(keypair.public_key().as_ref());
        let encoding_key = EncodingKey::from_ed_der(pkcs8.as_ref());

        let auth_listener = TcpListener::bind("127.0.0.1:0").await?;
        let auth_port = auth_listener.local_addr()?.port();
        let issuer = format!("http://127.0.0.1:{auth_port}");
        let issuer_clone = issuer.clone();

        let captured_nonce = Arc::new(std::sync::Mutex::new(String::new()));
        let captured_nonce_srv = captured_nonce.clone();
        let revoked_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let revoked_flag_srv = revoked_flag.clone();

        let server = tokio::spawn(async move {
            for _ in 0..8 {
                let Ok((mut socket, _)) = auth_listener.accept().await else {
                    break;
                };
                let (first_line, _headers, body) = read_http_request(&mut socket).await;
                let path = first_line.split_whitespace().nth(1).unwrap_or("/");

                let (status, content_type, resp_body) = if path
                    == "/.well-known/openid-configuration"
                {
                    (
                        "200 OK",
                        "application/json",
                        serde_json::json!({
                            "issuer": issuer_clone,
                            "jwks_uri": format!("{issuer_clone}/.well-known/jwks.json"),
                            "revocation_endpoint": format!("{issuer_clone}/oauth/revoke")
                        })
                        .to_string(),
                    )
                } else if path == "/.well-known/jwks.json" {
                    (
                        "200 OK",
                        "application/json",
                        serde_json::json!({
                            "keys": [{
                                "kty": "OKP",
                                "crv": "Ed25519",
                                "alg": "EdDSA",
                                "use": "sig",
                                "kid": "test-ed-1",
                                "x": pub_b64
                            }]
                        })
                        .to_string(),
                    )
                } else if path == "/api/accounts/oauth/token" {
                    let form: std::collections::HashMap<String, String> =
                        url::form_urlencoded::parse(body.as_bytes())
                            .into_owned()
                            .collect();
                    let grant = form.get("grant_type").map(String::as_str).unwrap_or("");
                    if grant == "authorization_code" {
                        let nonce = captured_nonce_srv.lock().unwrap().clone();
                        let mut header = Header::new(Algorithm::EdDSA);
                        header.kid = Some("test-ed-1".to_string());
                        let claims = serde_json::json!({
                            "iss": issuer_clone,
                            "aud": "oaiapp_issued_999",
                            "sub": "user-sub-42",
                            "email": "candidate@example.com",
                            "name": "Alex Morgan",
                            "nonce": nonce,
                            "exp": (Utc::now() + chrono::Duration::hours(1)).timestamp()
                        });
                        let id_jwt = jsonwebtoken::encode(&header, &claims, &encoding_key).unwrap();
                        (
                            "200 OK",
                            "application/json",
                            serde_json::json!({
                                "access_token": "access_token_v1",
                                "refresh_token": "refresh_token_v1",
                                "id_token": id_jwt,
                                "token_type": "Bearer",
                                "expires_in": 3600,
                                "scope": SIWC_SCOPES
                            })
                            .to_string(),
                        )
                    } else if grant == "refresh_token" {
                        assert_eq!(
                            form.get("refresh_token").map(String::as_str),
                            Some("refresh_token_v1")
                        );
                        assert!(!form.contains_key("scope"));
                        (
                            "200 OK",
                            "application/json",
                            serde_json::json!({
                                "access_token": "access_token_v2",
                                "refresh_token": "refresh_token_v2",
                                "token_type": "Bearer",
                                "expires_in": 3600,
                                "scope": SIWC_SCOPES
                            })
                            .to_string(),
                        )
                    } else {
                        ("400 Bad Request", "application/json", "{}".to_string())
                    }
                } else if path == "/oauth/revoke" {
                    let form: std::collections::HashMap<String, String> =
                        url::form_urlencoded::parse(body.as_bytes())
                            .into_owned()
                            .collect();
                    assert_eq!(
                        form.get("token").map(String::as_str),
                        Some("refresh_token_v2")
                    );
                    assert_eq!(
                        form.get("client_id").map(String::as_str),
                        Some("oaiapp_issued_999")
                    );
                    revoked_flag_srv.store(true, std::sync::atomic::Ordering::SeqCst);
                    ("200 OK", "application/json", "{}".to_string())
                } else {
                    ("404 Not Found", "text/plain", "not found".to_string())
                };

                let http = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp_body}",
                    resp_body.len()
                );
                let _ = socket.write_all(http.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        let store = Arc::new(InMemoryChatGptStore::new());
        let client = ChatGptClient::with_store(
            AiConfig {
                base_url: CHATGPT_API_BASE_URL.to_string(),
                auth_issuer: issuer.clone(),
                model: "gpt-5.4".to_string(),
                request_timeout_secs: 15,
                ..AiConfig::default()
            },
            store.clone(),
        );

        assert!(!client.is_configured());

        // Simulate browser navigating to authorize URL and redirecting back to 127.0.0.1 callback.
        let captured_nonce_cb = captured_nonce.clone();
        let status = client
            .sign_in_with_launcher(false, false, Duration::from_secs(10), move |auth_url| async move {
                let parsed = Url::parse(&auth_url)?;
                let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
                assert_eq!(q.get("client_id").map(String::as_str), Some(DYNAMIC_AGENT_CLIENT_ID));
                assert_eq!(q.get("agent_name_hint").map(String::as_str), Some(CHATGPT_AGENT_NAME_HINT));
                *captured_nonce_cb.lock().unwrap() = q.get("nonce").cloned().unwrap_or_default();

                let redirect_uri = q.get("redirect_uri").cloned().unwrap();
                let state = q.get("state").cloned().unwrap();
                let callback_url = format!(
                    "{redirect_uri}?code=auth_code_777&state={state}&client_id=oaiapp_issued_999&scope={}",
                    url::form_urlencoded::byte_serialize(SIWC_SCOPES.as_bytes()).collect::<String>()
                );

                tokio::spawn(async move {
                    let _ = reqwest::get(&callback_url).await;
                });
                Ok(())
            })
            .await?;

        assert!(status.is_ready_for_inference());
        assert_eq!(status.email.as_deref(), Some("candidate@example.com"));
        assert_eq!(status.name.as_deref(), Some("Alex Morgan"));
        assert_eq!(status.client_id.as_deref(), Some("oaiapp_issued_999"));
        assert!(client.is_configured());

        // Force rotating token refresh and verify replacement refresh token persisted.
        let new_access = client.ensure_valid_access_token(true).await?;
        assert_eq!(new_access, "access_token_v2");
        let saved = store.load_session()?.unwrap();
        assert_eq!(saved.refresh_token.as_deref(), Some("refresh_token_v2"));

        // Sign out and verify revocation endpoint received the rotated refresh token.
        let warn = client.sign_out().await?;
        assert!(warn.is_none());
        assert!(revoked_flag.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!client.is_configured());
        assert!(store.load_session()?.is_none());

        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn streaming_responses_parses_namespaced_tool_calls_and_enforces_pii_gate() -> Result<()>
    {
        let (body, pii, bullet_id, _summary_id, _skill_cat_id) = test_body_and_pii();
        let scrubbed = PiiScrubber.scrub(&body, &pii)?;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let base_url = format!("http://127.0.0.1:{port}/v1");

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (first_line, headers, req_body) = read_http_request(&mut socket).await;
            assert!(first_line.starts_with("POST /v1/responses "));
            assert!(headers.contains("Bearer test_valid_access_token"));

            let parsed_req: serde_json::Value = serde_json::from_str(&req_body).unwrap();
            assert_eq!(parsed_req["store"], serde_json::json!(false));
            assert_eq!(parsed_req["stream"], serde_json::json!(true));
            assert!(parsed_req.get("temperature").is_none());
            assert_eq!(
                parsed_req["tools"][0]["type"],
                serde_json::json!("namespace")
            );

            let rewrite_args = serde_json::json!({
                "bullet_id": bullet_id.to_string(),
                "proposed_text": "Automated Python detection triage workflows, cutting manual alert review.",
                "rationale": "Highlights Python automation for the target role."
            })
            .to_string();

            let score_args = serde_json::json!({
                "matched_keywords": ["Python", "detection"],
                "missing_keywords": ["Kubernetes"],
                "score": 84
            })
            .to_string();

            let ev1 = serde_json::json!({
                "type": "response.output_text.delta",
                "delta": "Tailored your experience bullets for the role."
            });
            let ev2 = serde_json::json!({
                "type": "response.output_item.done",
                "item": {
                    "type": "function_call",
                    "call_id": "call_1",
                    "namespace": TOOL_NAMESPACE_NAME,
                    "name": "propose_bullet_rewrite",
                    "arguments": rewrite_args
                }
            });
            let ev3 = serde_json::json!({
                "type": "response.output_item.done",
                "item": {
                    "type": "function_call",
                    "call_id": "call_2",
                    "namespace": TOOL_NAMESPACE_NAME,
                    "name": "score_ats_alignment",
                    "arguments": score_args
                }
            });
            let ev4 = serde_json::json!({
                "type": "response.completed",
                "response": { "id": "resp_1", "status": "completed", "output": [] }
            });

            let sse = format!(
                "event: response.output_text.delta\ndata: {ev1}\n\n\
                 event: response.output_item.done\ndata: {ev2}\n\n\
                 event: response.output_item.done\ndata: {ev3}\n\n\
                 event: response.completed\ndata: {ev4}\n\n"
            );

            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse}",
                sse.len()
            );
            let _ = socket.write_all(resp.as_bytes()).await;
            let _ = socket.shutdown().await;
        });

        let session = ChatGptSessionRecord {
            issuer: CHATGPT_AUTH_ISSUER.to_string(),
            client_id: "oaiapp_test".to_string(),
            ext_agent_host_id: format!("urn:uuid:{}", Uuid::new_v4()),
            subject: Some("sub-1".to_string()),
            email: Some("alex@example.com".to_string()),
            name: Some("Alex".to_string()),
            id_token: Some("id.jwt".to_string()),
            access_token: Some("test_valid_access_token".to_string()),
            refresh_token: Some("test_refresh".to_string()),
            token_type: "Bearer".to_string(),
            expires_in: Some(3600),
            access_token_expires_at: Some(Utc::now() + chrono::Duration::hours(1)),
            earliest_refresh_at: None,
            scopes: vec!["openid".to_string(), CHATGPT_PLAN_USAGE_SCOPE.to_string()],
            needs_reauth: false,
            saved_at: Utc::now(),
        };

        let client = ChatGptClient::with_store(
            AiConfig {
                base_url,
                auth_issuer: CHATGPT_AUTH_ISSUER.to_string(),
                model: "gpt-5.4".to_string(),
                request_timeout_secs: 15,
                ..AiConfig::default()
            },
            Arc::new(InMemoryChatGptStore::with_session(session)),
        );

        let (tx, mut rx) = unbounded_channel();
        let outcome = client
            .generate_variation(
                VariationRequest {
                    child_cv_id: Uuid::new_v4(),
                    job_context: JobContext {
                        company_name: "Acme".to_string(),
                        target_role: "Detection Engineer".to_string(),
                        job_description_raw: "Looking for Python detection expertise.".to_string(),
                        ..Default::default()
                    },
                    skill_ids: vec![],
                },
                scrubbed.clone(),
                tx,
            )
            .await?;

        assert_eq!(outcome.proposals.len(), 1);
        assert_eq!(outcome.ats_score.as_ref().map(|s| s.score), Some(84));
        assert!(outcome.summary.contains("Tailored your experience bullets"));

        let mut saw_completed_event = false;
        while let Ok(ev) = rx.try_recv() {
            if matches!(ev, AiEvent::RunCompleted { .. }) {
                saw_completed_event = true;
            }
        }
        assert!(saw_completed_event);

        // Verify tampered payload is hard-blocked by the zero-trust PII security gate.
        let mut tampered = scrubbed;
        tampered.json.push_str(" alex.morgan@example.com");
        assert!(client.verify_security_gate(&tampered).is_err());

        server.await?;
        Ok(())
    }

    #[tokio::test]
    async fn mid_stream_usage_limit_error_surfaces_manage_usage_guidance() -> Result<()> {
        let (body, pii, _bullet_id, _summary_id, _skill_cat_id) = test_body_and_pii();
        let scrubbed = PiiScrubber.scrub(&body, &pii)?;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let base_url = format!("http://127.0.0.1:{port}/v1");

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = read_http_request(&mut socket).await;
            let failed = serde_json::json!({
                "type": "response.failed",
                "response": {
                    "error": {
                        "code": "subscription_sharing_usage_limit_exceeded",
                        "message": "Usage limit reached."
                    }
                }
            });
            let sse = format!("event: response.failed\ndata: {failed}\n\n");
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse}",
                sse.len()
            );
            let _ = socket.write_all(resp.as_bytes()).await;
            let _ = socket.shutdown().await;
        });

        let session = ChatGptSessionRecord {
            issuer: CHATGPT_AUTH_ISSUER.to_string(),
            client_id: "oaiapp_test".to_string(),
            ext_agent_host_id: format!("urn:uuid:{}", Uuid::new_v4()),
            subject: Some("sub-1".to_string()),
            email: Some("alex@example.com".to_string()),
            name: None,
            id_token: None,
            access_token: Some("valid_tok".to_string()),
            refresh_token: None,
            token_type: "Bearer".to_string(),
            expires_in: Some(3600),
            access_token_expires_at: Some(Utc::now() + chrono::Duration::hours(1)),
            earliest_refresh_at: None,
            scopes: vec![CHATGPT_PLAN_USAGE_SCOPE.to_string()],
            needs_reauth: false,
            saved_at: Utc::now(),
        };

        let client = ChatGptClient::with_store(
            AiConfig {
                base_url,
                auth_issuer: CHATGPT_AUTH_ISSUER.to_string(),
                model: "gpt-5.4".to_string(),
                request_timeout_secs: 15,
                ..AiConfig::default()
            },
            Arc::new(InMemoryChatGptStore::with_session(session)),
        );

        let (tx, _rx) = unbounded_channel();
        let err = client
            .generate_variation(
                VariationRequest {
                    child_cv_id: Uuid::new_v4(),
                    job_context: JobContext::default(),
                    skill_ids: vec![],
                },
                scrubbed,
                tx,
            )
            .await
            .unwrap_err();

        let msg = err.to_string();
        assert!(msg.contains("Usage limit reached"), "got: {msg}");
        assert!(msg.contains(CHATGPT_USAGE_SETTINGS_URL), "got: {msg}");

        server.await?;
        Ok(())
    }

    #[test]
    fn chatgpt_rate_limit_and_quota_errors_are_detected_clearly() {
        let rate_body = r#"{"error":{"code":"rate_limit_exceeded","message":"Rate limit reached for gpt-5.4."}}"#;
        let (msg, _) = format_siwc_api_error(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            rate_body,
            Some("gpt-5.4"),
        );
        assert!(msg.contains("limit reached"), "got: {msg}");
        assert!(crate::ai::is_ai_limit_error(&msg));

        let quota_obj = serde_json::json!({
            "code": "insufficient_quota",
            "message": "You exceeded your current quota."
        });
        let (quota_msg, _) = format_siwc_structured_error(Some(&quota_obj), Some("gpt-5.4"));
        assert!(quota_msg.contains("limit reached"), "got: {quota_msg}");
        assert!(crate::ai::is_ai_limit_error(&quota_msg));
    }
}
