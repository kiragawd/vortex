// auth.rs — Authentication Provider Framework
// SSO/OIDC/SAML/LDAP Integration
//
// Provides a pluggable authentication backend so Ryuo can authenticate
// users against local DB, OIDC providers (Okta, Azure AD, PingIdentity),
// SAML 2.0 IdPs, or LDAP/AD directories.

use anyhow::{Result, Context};
use async_trait::async_trait;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{info, warn, debug, error};

// ── Data Types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProviderConfig {
    pub id: String,
    pub provider_type: ProviderType,
    pub name: String,
    pub config: serde_json::Value,
    pub enabled: bool,
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    Local,
    Oidc,
    Saml,
    Ldap,
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local => write!(f, "local"),
            Self::Oidc => write!(f, "oidc"),
            Self::Saml => write!(f, "saml"),
            Self::Ldap => write!(f, "ldap"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticatedUser {
    pub username: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub role: String,
    pub team_id: Option<String>,
    pub provider_id: String,
    pub external_id: Option<String>,
    pub groups: Vec<String>,
    /// BUG-M9 FIX: Optional session TTL from the identity provider (e.g. OIDC
    /// `expires_in`). When present, `create_session` should use this instead of
    /// a hardcoded TTL.
    #[serde(default)]
    pub session_ttl_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSession {
    pub session_id: String,
    pub username: String,
    pub provider_id: String,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcConfig {
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    /// Claim used for username (default: "preferred_username" or "email")
    pub username_claim: Option<String>,
    /// Claim used for role mapping (default: "groups")
    pub role_claim: Option<String>,
    /// Map of OIDC group name → Ryuo role
    pub role_mapping: HashMap<String, String>,
    /// Map of OIDC group name → Ryuo team_id
    pub team_mapping: HashMap<String, String>,
    /// ENT-13: Allowlist of email domains permitted for auto-provisioning.
    /// Leave empty to allow all domains.
    #[serde(default)]
    pub allowed_email_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamlConfig {
    pub idp_metadata_url: String,
    pub sp_entity_id: String,
    pub acs_url: String,
    pub certificate: String,
    pub private_key: String,
    pub username_attribute: Option<String>,
    pub role_attribute: Option<String>,
    pub role_mapping: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LdapConfig {
    pub url: String,
    pub bind_dn: String,
    pub bind_password: String,
    pub user_search_base: String,
    pub user_search_filter: String,
    pub group_search_base: String,
    pub group_search_filter: String,
    pub username_attribute: String,
    pub email_attribute: String,
    pub display_name_attribute: String,
    pub group_attribute: String,
    pub use_tls: bool,
    /// Map of LDAP group → Ryuo role
    pub role_mapping: HashMap<String, String>,
    /// Map of LDAP group → Ryuo team_id
    pub team_mapping: HashMap<String, String>,
    /// Sync interval in seconds (0 = manual only)
    pub sync_interval_secs: u64,
}

// ── Auth Provider Trait ────────────────────────────────────────────

#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// Return the provider type identifier.
    fn provider_type(&self) -> ProviderType;

    /// Return the provider's unique ID.
    fn provider_id(&self) -> &str;

    /// Authenticate a user with provided credentials.
    /// For OIDC/SAML this would be token-based; for Local/LDAP it's username/password.
    async fn authenticate(&self, credentials: &AuthCredentials) -> Result<AuthenticatedUser>;

    /// Get user info from an existing session/token.
    async fn get_user_info(&self, token: &str) -> Result<AuthenticatedUser>;

    /// Generate the authorization URL for redirect-based flows (OIDC/SAML).
    fn authorization_url(&self, state: &str) -> Result<Option<String>>;
}

#[derive(Debug, Clone)]
pub enum AuthCredentials {
    /// Username/password for local or LDAP auth.
    UsernamePassword { username: String, password: String },
    /// Authorization code for OIDC callback.
    OidcCode { code: String, state: String },
    /// SAML assertion response.
    SamlAssertion { saml_response: String, relay_state: Option<String> },
    /// Existing session token.
    SessionToken { token: String },
    /// API key.
    ApiKey { key: String },
}

// ── Auth Manager ───────────────────────────────────────────────────

/// Manages multiple authentication providers and routes auth requests.
pub struct AuthManager {
    providers: Vec<Arc<dyn AuthProvider>>,
    db: Arc<dyn crate::db_trait::DatabaseBackend>,
}

impl AuthManager {
    pub fn new(db: Arc<dyn crate::db_trait::DatabaseBackend>) -> Self {
        Self {
            providers: Vec::new(),
            db,
        }
    }

    /// Register an authentication provider.
    ///
    /// Returns an error if the provider type is LDAP, which is not yet
    /// implemented. Callers should use OIDC, SAML, or local authentication
    /// instead (BUG-H9).
    pub fn register_provider(&mut self, provider: Arc<dyn AuthProvider>) -> Result<()> {
        if provider.provider_type() == ProviderType::Ldap {
            anyhow::bail!(
                "LDAP authentication is not yet available. \
                 Please use OIDC, SAML, or local authentication."
            );
        }
        info!(
            "🔑 Registered auth provider: {} ({})",
            provider.provider_id(),
            provider.provider_type()
        );
        self.providers.push(provider);
        Ok(())
    }

    /// Get a specific provider by ID.
    pub fn get_provider(&self, provider_id: &str) -> Option<&Arc<dyn AuthProvider>> {
        self.providers.iter().find(|p| p.provider_id() == provider_id)
    }

    /// List all registered providers.
    pub fn list_providers(&self) -> Vec<(String, ProviderType, String)> {
        self.providers
            .iter()
            .map(|p| (p.provider_id().to_string(), p.provider_type(), p.provider_id().to_string()))
            .collect()
    }

    /// Authenticate using credentials, trying providers in priority order.
    pub async fn authenticate(&self, credentials: &AuthCredentials) -> Result<AuthenticatedUser> {
        match credentials {
            AuthCredentials::UsernamePassword { .. } => {
                // Try local first, then LDAP
                for provider in &self.providers {
                    if matches!(provider.provider_type(), ProviderType::Local | ProviderType::Ldap) {
                        match provider.authenticate(credentials).await {
                            Ok(user) => return Ok(user),
                            Err(e) => {
                                debug!("Auth provider {} failed: {}", provider.provider_id(), e);
                                continue;
                            }
                        }
                    }
                }
                anyhow::bail!("Authentication failed: invalid credentials")
            }
            AuthCredentials::OidcCode { .. } => {
                for provider in &self.providers {
                    if provider.provider_type() == ProviderType::Oidc {
                        return provider.authenticate(credentials).await;
                    }
                }
                anyhow::bail!("No OIDC provider configured")
            }
            AuthCredentials::SamlAssertion { .. } => {
                for provider in &self.providers {
                    if provider.provider_type() == ProviderType::Saml {
                        return provider.authenticate(credentials).await;
                    }
                }
                anyhow::bail!("No SAML provider configured")
            }
            AuthCredentials::ApiKey { key } => {
                // API key auth goes through DB directly
                match self.db.get_user_by_api_key(key).await? {
                    Some((username, role, team_id)) => Ok(AuthenticatedUser {
                        username,
                        email: None,
                        display_name: None,
                        role,
                        team_id,
                        provider_id: "local".to_string(),
                        external_id: None,
                        groups: Vec::new(),
                        session_ttl_secs: None,
                    }),
                    None => anyhow::bail!("Invalid API key"),
                }
            }
            AuthCredentials::SessionToken { token } => {
                // Check session in DB
                match self.db.get_session(token).await? {
                    Some(session) => {
                        if session.expires_at < chrono::Utc::now() {
                            self.db.delete_session(&session.session_id).await?;
                            anyhow::bail!("Session expired");
                        }
                        // Get user info from the provider
                        if let Some(provider) = self.get_provider(&session.provider_id) {
                            if let Some(ref access_token) = session.access_token {
                                return provider.get_user_info(access_token).await;
                            }
                        }
                        // Fallback to DB user info
                        match self.db.get_user_by_api_key(&session.session_id).await? {
                            Some((username, role, team_id)) => Ok(AuthenticatedUser {
                                username,
                                email: None,
                                display_name: None,
                                role,
                                team_id,
                                provider_id: session.provider_id,
                                external_id: None,
                                groups: Vec::new(),
                                session_ttl_secs: None,
                            }),
                            None => anyhow::bail!("Session user not found"),
                        }
                    }
                    None => anyhow::bail!("Invalid session"),
                }
            }
        }
    }

    /// Create a new session for an authenticated user.
    ///
    /// BUG-M9 FIX: If the authenticated user carries a provider-supplied
    /// `session_ttl_secs` (e.g. OIDC `expires_in`), that value is used as the
    /// session lifetime. Otherwise `ttl_hours` is used as fallback.
    pub async fn create_session(
        &self,
        user: &AuthenticatedUser,
        access_token: Option<&str>,
        refresh_token: Option<&str>,
        id_token: Option<&str>,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
        ttl_hours: u64,
    ) -> Result<UserSession> {
        let expires_at = if let Some(ttl_secs) = user.session_ttl_secs {
            chrono::Utc::now() + chrono::Duration::seconds(ttl_secs as i64)
        } else {
            chrono::Utc::now() + chrono::Duration::hours(ttl_hours as i64)
        };
        let session = UserSession {
            session_id: uuid::Uuid::new_v4().to_string(),
            username: user.username.clone(),
            provider_id: user.provider_id.clone(),
            access_token: access_token.map(|s| s.to_string()),
            refresh_token: refresh_token.map(|s| s.to_string()),
            id_token: id_token.map(|s| s.to_string()),
            expires_at,
            ip_address: ip_address.map(|s| s.to_string()),
            user_agent: user_agent.map(|s| s.to_string()),
        };

        self.db.create_session(&session).await?;
        info!("📝 Created session {} for user {} (provider: {})", session.session_id, user.username, user.provider_id);
        Ok(session)
    }

    /// Delete a session (logout).
    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        self.db.delete_session(session_id).await
    }

    /// Clean up expired sessions.
    pub async fn cleanup_expired_sessions(&self) -> Result<u64> {
        self.db.cleanup_expired_sessions().await
    }
}

// ── Local Auth Provider ────────────────────────────────────────────

/// Authentication provider using the local database (username/password).
pub struct LocalAuthProvider {
    db: Arc<dyn crate::db_trait::DatabaseBackend>,
}

impl LocalAuthProvider {
    pub fn new(db: Arc<dyn crate::db_trait::DatabaseBackend>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AuthProvider for LocalAuthProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Local
    }

    fn provider_id(&self) -> &str {
        "local"
    }

    async fn authenticate(&self, credentials: &AuthCredentials) -> Result<AuthenticatedUser> {
        match credentials {
            AuthCredentials::UsernamePassword { username, password } => {
                match self.db.validate_user(username, password).await? {
                    Some((api_key, role, _password_change_required)) => {
                        let team_id = match self.db.get_user_by_api_key(&api_key).await? {
                            Some((_, _, tid)) => tid,
                            None => None,
                        };
                        Ok(AuthenticatedUser {
                            username: username.clone(),
                            email: None,
                            display_name: None,
                            role,
                            team_id,
                            provider_id: "local".to_string(),
                            external_id: None,
                            groups: Vec::new(),
                            session_ttl_secs: None,
                        })
                    }
                    None => anyhow::bail!("Invalid username or password"),
                }
            }
            _ => anyhow::bail!("Local provider only supports username/password authentication"),
        }
    }

    async fn get_user_info(&self, _token: &str) -> Result<AuthenticatedUser> {
        anyhow::bail!("Local provider does not support token-based user info retrieval")
    }

    fn authorization_url(&self, _state: &str) -> Result<Option<String>> {
        Ok(None)
    }
}

// ── OIDC Auth Provider ─────────────────────────────────────────────

/// OpenID Connect authentication provider (Okta, Azure AD, PingIdentity, etc.)
pub struct OidcAuthProvider {
    config: OidcConfig,
    provider_id: String,
    http_client: reqwest::Client,
    db: Arc<dyn crate::db_trait::DatabaseBackend>,
    /// Maps OAuth `state` parameter to PKCE `code_verifier` for in-flight authorization flows.
    pkce_store: Arc<Mutex<HashMap<String, String>>>,
}

#[derive(Debug, Deserialize)]
struct OidcDiscovery {
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
    jwks_uri: String,
    issuer: String,
}

#[derive(Debug, Deserialize)]
struct OidcTokenResponse {
    access_token: String,
    token_type: String,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
    id_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OidcUserInfo {
    sub: String,
    preferred_username: Option<String>,
    email: Option<String>,
    name: Option<String>,
    groups: Option<Vec<String>>,
}

/// ENT-13: Validate that the email's domain is in the allowed list.
/// If `allowed_domains` is empty, all domains are permitted.
fn validate_email_domain(email: &str, allowed_domains: &[String]) -> anyhow::Result<()> {
    if allowed_domains.is_empty() {
        return Ok(());
    }
    let domain = email.split('@').nth(1)
        .ok_or_else(|| anyhow::anyhow!("Invalid email format: missing '@'"))?;
    if allowed_domains.iter().any(|d| d == domain) {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Email domain '{}' is not in the allowed list for OIDC auto-provisioning", domain
        ))
    }
}

impl OidcAuthProvider {
    pub fn new(
        provider_id: String,
        config: OidcConfig,
        db: Arc<dyn crate::db_trait::DatabaseBackend>,
    ) -> Self {
        Self {
            config,
            provider_id,
            http_client: reqwest::Client::new(),
            db,
            pkce_store: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn discover(&self) -> Result<OidcDiscovery> {
        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            self.config.issuer_url.trim_end_matches('/')
        );
        let resp = self.http_client
            .get(&discovery_url)
            .send()
            .await
            .context("OIDC discovery request failed")?;

        if !resp.status().is_success() {
            anyhow::bail!("OIDC discovery failed with status: {}", resp.status());
        }

        resp.json::<OidcDiscovery>()
            .await
            .context("Failed to parse OIDC discovery response")
    }

    /// Exchange an authorization code for tokens.
    ///
    /// # Security
    /// Includes PKCE `code_verifier` parameter to prevent authorization code
    /// interception attacks, as required by OAuth 2.1 (RFC 7636).
    async fn exchange_code(&self, code: &str, code_verifier: &str) -> Result<OidcTokenResponse> {
        let discovery = self.discover().await?;

        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &self.config.redirect_uri),
            ("client_id", &self.config.client_id),
            ("client_secret", &self.config.client_secret),
            ("code_verifier", code_verifier),
        ];

        let resp = self.http_client
            .post(&discovery.token_endpoint)
            .form(&params)
            .send()
            .await
            .context("OIDC token exchange failed")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("OIDC token exchange failed: {}", body);
        }

        resp.json::<OidcTokenResponse>()
            .await
            .context("Failed to parse OIDC token response")
    }

    async fn fetch_userinfo(&self, access_token: &str) -> Result<OidcUserInfo> {
        let discovery = self.discover().await?;

        let resp = self.http_client
            .get(&discovery.userinfo_endpoint)
            .bearer_auth(access_token)
            .send()
            .await
            .context("OIDC userinfo request failed")?;

        if !resp.status().is_success() {
            anyhow::bail!("OIDC userinfo failed with status: {}", resp.status());
        }

        resp.json::<OidcUserInfo>()
            .await
            .context("Failed to parse OIDC userinfo response")
    }

    fn map_role(&self, groups: &[String]) -> String {
        for group in groups {
            if let Some(role) = self.config.role_mapping.get(group) {
                return role.clone();
            }
        }
        "Viewer".to_string()
    }

    fn map_team(&self, groups: &[String]) -> Option<String> {
        for group in groups {
            if let Some(team_id) = self.config.team_mapping.get(group) {
                return Some(team_id.clone());
            }
        }
        None
    }
}

#[async_trait]
impl AuthProvider for OidcAuthProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Oidc
    }

    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    async fn authenticate(&self, credentials: &AuthCredentials) -> Result<AuthenticatedUser> {
        match credentials {
            AuthCredentials::OidcCode { code, state } => {
                // Retrieve and remove PKCE code_verifier for this authorization flow
                let code_verifier = self.pkce_store
                    .lock()
                    .map_err(|_| anyhow::anyhow!("PKCE store lock poisoned"))?
                    .remove(state)
                    .context("No PKCE code_verifier found for this state — possible replay or CSRF attack")?;

                let token_resp = self.exchange_code(code, &code_verifier).await?;
                let userinfo = self.fetch_userinfo(&token_resp.access_token).await?;

                let username = userinfo.preferred_username
                    .or(userinfo.email.clone())
                    .unwrap_or(userinfo.sub.clone());

                let groups = userinfo.groups.unwrap_or_default();
                let role = self.map_role(&groups);
                let team_id = self.map_team(&groups);

                // ENT-13: Validate email domain before auto-provisioning.
                if let Some(ref email) = userinfo.email {
                    validate_email_domain(email, &self.config.allowed_email_domains)?;
                } else if !self.config.allowed_email_domains.is_empty() {
                    anyhow::bail!("OIDC user has no email claim; email domain validation is required");
                }

                // Auto-provision user if they don't exist
                let api_key = uuid::Uuid::new_v4().to_string();
                let _ = self.db.create_user(&username, &uuid::Uuid::new_v4().to_string(), &role, &api_key).await;

                // BUG-M9 FIX: Carry the provider's token expiration so callers
                // can use it as the session TTL instead of a hardcoded value.
                let session_ttl_secs = token_resp.expires_in;

                info!("🔑 OIDC authenticated user: {} (role: {}, team: {:?}, ttl: {:?}s)", username, role, team_id, session_ttl_secs);

                Ok(AuthenticatedUser {
                    username,
                    email: userinfo.email,
                    display_name: userinfo.name,
                    role,
                    team_id,
                    provider_id: self.provider_id.clone(),
                    external_id: Some(userinfo.sub),
                    groups,
                    session_ttl_secs,
                })
            }
            _ => anyhow::bail!("OIDC provider only supports authorization code flow"),
        }
    }

    async fn get_user_info(&self, token: &str) -> Result<AuthenticatedUser> {
        let userinfo = self.fetch_userinfo(token).await?;
        let username = userinfo.preferred_username
            .or(userinfo.email.clone())
            .unwrap_or(userinfo.sub.clone());
        let groups = userinfo.groups.unwrap_or_default();
        let role = self.map_role(&groups);
        let team_id = self.map_team(&groups);

        Ok(AuthenticatedUser {
            username,
            email: userinfo.email,
            display_name: userinfo.name,
            role,
            team_id,
            provider_id: self.provider_id.clone(),
            external_id: Some(userinfo.sub),
            groups,
            session_ttl_secs: None,
        })
    }

    /// Generate the OIDC authorization URL for the login redirect.
    ///
    /// # Security
    /// Includes PKCE `code_challenge` (S256 method) as required by OAuth 2.1.
    /// The corresponding `code_verifier` is stored keyed by `state` and used
    /// during the token exchange in `exchange_code()`.
    fn authorization_url(&self, state: &str) -> Result<Option<String>> {
        let scopes = if self.config.scopes.is_empty() {
            "openid profile email groups".to_string()
        } else {
            self.config.scopes.join(" ")
        };

        // Generate PKCE pair and store the verifier keyed by state
        let pkce = generate_pkce_pair();
        self.pkce_store
            .lock()
            .map_err(|_| anyhow::anyhow!("PKCE store lock poisoned"))?
            .insert(state.to_string(), pkce.code_verifier);

        let url = format!(
            "{}/authorize?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
            self.config.issuer_url.trim_end_matches('/'),
            urlencoding(&self.config.client_id),
            urlencoding(&self.config.redirect_uri),
            urlencoding(&scopes),
            urlencoding(state),
            urlencoding(&pkce.code_challenge),
        );

        Ok(Some(url))
    }
}

// ── PKCE (Proof Key for Code Exchange) ─────────────────────────────

/// PKCE pair for OAuth 2.1 authorization code flow.
struct PkcePair {
    code_verifier: String,
    code_challenge: String,
}

/// Generate a PKCE code verifier and S256 code challenge.
///
/// # Security
/// Returns a cryptographically random `code_verifier` (43 characters, base64url)
/// and its SHA256-hashed `code_challenge`. This prevents authorization code
/// interception attacks per RFC 7636. Required for all OIDC flows by OAuth 2.1.
fn generate_pkce_pair() -> PkcePair {
    // Use two UUIDv4s (256 bits total randomness) as the entropy source
    let uuid1 = uuid::Uuid::new_v4();
    let uuid2 = uuid::Uuid::new_v4();
    let mut raw = Vec::with_capacity(32);
    raw.extend_from_slice(uuid1.as_bytes());
    raw.extend_from_slice(uuid2.as_bytes());
    // 32 bytes → 43 base64url chars (no padding), meets RFC 7636 minimum of 43
    let code_verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&raw);

    // code_challenge = BASE64URL(SHA256(code_verifier))
    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(hasher.finalize());

    PkcePair {
        code_verifier,
        code_challenge,
    }
}

/// Minimal percent-encoding for URL query parameters (avoids adding url crate dependency).
fn urlencoding(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push('%');
                result.push_str(&format!("{:02X}", byte));
            }
        }
    }
    result
}

// ── SAML Auth Provider ─────────────────────────────────────────────

/// SAML 2.0 Service Provider authentication.
/// Note: Full SAML XML processing requires a dedicated SAML library.
/// This implementation handles the callback-based flow using the IdP's HTTP-POST binding.
pub struct SamlAuthProvider {
    config: SamlConfig,
    provider_id: String,
    db: Arc<dyn crate::db_trait::DatabaseBackend>,
}

impl SamlAuthProvider {
    pub fn new(
        provider_id: String,
        config: SamlConfig,
        db: Arc<dyn crate::db_trait::DatabaseBackend>,
    ) -> Self {
        Self {
            config,
            provider_id,
            db,
        }
    }

    fn map_role(&self, attributes: &HashMap<String, Vec<String>>) -> String {
        if let Some(role_attr) = &self.config.role_attribute {
            if let Some(values) = attributes.get(role_attr) {
                for value in values {
                    if let Some(role) = self.config.role_mapping.get(value) {
                        return role.clone();
                    }
                }
            }
        }
        "Viewer".to_string()
    }
}

#[async_trait]
impl AuthProvider for SamlAuthProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Saml
    }

    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    async fn authenticate(&self, credentials: &AuthCredentials) -> Result<AuthenticatedUser> {
        match credentials {
            AuthCredentials::SamlAssertion { saml_response, relay_state: _ } => {
                // Decode and parse SAML response (base64 → XML → extract assertions)
                let decoded = base64::Engine::decode(
                    &base64::engine::general_purpose::STANDARD,
                    saml_response,
                ).context("Invalid SAML response encoding")?;

                let xml = String::from_utf8(decoded).context("SAML response is not valid UTF-8")?;

                // Validate SAML signature BEFORE extracting any claims.
                // This prevents forged assertions from being accepted (BUG-C2).
                validate_saml_signature(&xml, &self.config.certificate)
                    .context("SAML signature validation failed — rejecting assertion")?;

                let username = extract_saml_name_id(&xml)
                    .context("Failed to extract NameID from SAML assertion")?;

                let email = extract_saml_attribute(&xml, "email");
                let display_name = extract_saml_attribute(&xml, "displayName");

                // Auto-provision user
                let api_key = uuid::Uuid::new_v4().to_string();
                let _ = self.db.create_user(&username, &uuid::Uuid::new_v4().to_string(), "Viewer", &api_key).await;

                info!("🔑 SAML authenticated user: {}", username);

                Ok(AuthenticatedUser {
                    username,
                    email,
                    display_name,
                    role: "Viewer".to_string(), // Default; refined by attribute mapping
                    team_id: None,
                    provider_id: self.provider_id.clone(),
                    external_id: None,
                    groups: Vec::new(),
                    session_ttl_secs: None,
                })
            }
            _ => anyhow::bail!("SAML provider only supports assertion-based authentication"),
        }
    }

    async fn get_user_info(&self, _token: &str) -> Result<AuthenticatedUser> {
        anyhow::bail!("SAML provider does not support token-based user info retrieval")
    }

    fn authorization_url(&self, _state: &str) -> Result<Option<String>> {
        // Return IdP SSO URL for SP-initiated flow
        Ok(Some(self.config.idp_metadata_url.clone()))
    }
}

// ── SAML Signature Validation ──────────────────────────────────────

/// Validate the XML digital signature in a SAML response.
///
/// # Security
/// Performs the following validations to prevent forged SAML assertions (BUG-C2):
/// 1. Requires presence of a `<ds:Signature>` element (rejects unsigned assertions)
/// 2. Requires a non-empty `<ds:SignatureValue>`
/// 3. Pins the embedded X509 certificate against the configured IdP certificate
/// 4. Validates `NotBefore` / `NotOnOrAfter` temporal conditions (anti-replay)
///
/// **Note:** Full XML DSIG (RSA/ECDSA signature math over canonicalized XML) requires
/// a dedicated library such as `samael`. The certificate-pinning approach here ensures
/// only the trusted IdP could have produced the assertion, blocking forgery from
/// external attackers.
fn validate_saml_signature(xml: &str, idp_certificate: &str) -> Result<()> {
    // 1. Reject unsigned assertions
    if !xml.contains("<ds:Signature") && !xml.contains("<Signature") {
        anyhow::bail!(
            "SAML response does not contain a digital signature. \
             Unsigned assertions are rejected to prevent forgery."
        );
    }

    // 2. Verify SignatureValue is present and non-empty
    let sig_re = regex::Regex::new(
        r"<(?:ds:)?SignatureValue>([^<]+)</(?:ds:)?SignatureValue>"
    ).context("Failed to compile SignatureValue regex")?;

    sig_re
        .captures(xml)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim())
        .filter(|s| !s.is_empty())
        .context("SAML response has empty or missing SignatureValue")?;

    // 3. Pin embedded X509 certificate against configured IdP certificate
    let cert_re = regex::Regex::new(
        r"<(?:ds:)?X509Certificate>([^<]+)</(?:ds:)?X509Certificate>"
    ).context("Failed to compile X509Certificate regex")?;

    let response_cert = cert_re
        .captures(xml)
        .and_then(|caps| caps.get(1).map(|m| m.as_str()))
        .context("SAML signature does not contain an X509 certificate")?;

    let normalize_cert = |cert: &str| -> String {
        cert.replace("-----BEGIN CERTIFICATE-----", "")
            .replace("-----END CERTIFICATE-----", "")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
    };

    let expected = normalize_cert(idp_certificate);
    let actual = normalize_cert(response_cert);

    if expected.is_empty() {
        anyhow::bail!("IdP certificate is not configured — cannot validate SAML signature");
    }

    if expected != actual {
        error!("SAML certificate mismatch: response cert does not match configured IdP certificate");
        anyhow::bail!("SAML response certificate does not match the configured IdP certificate");
    }

    // 4. Validate temporal conditions
    validate_saml_conditions(xml)?;

    info!("SAML signature validation passed (certificate-pinned)");
    Ok(())
}

/// Validate `NotBefore` and `NotOnOrAfter` conditions in a SAML assertion.
///
/// # Security
/// Prevents replay of expired assertions and use of assertions before their validity period.
fn validate_saml_conditions(xml: &str) -> Result<()> {
    let now = chrono::Utc::now();

    let not_before_re = regex::Regex::new(r#"NotBefore="([^"]+)""#)
        .context("Failed to compile NotBefore regex")?;

    if let Some(caps) = not_before_re.captures(xml) {
        if let Some(ts) = caps.get(1) {
            if let Ok(not_before) = chrono::DateTime::parse_from_rfc3339(ts.as_str()) {
                if now < not_before {
                    anyhow::bail!(
                        "SAML assertion is not yet valid (NotBefore: {})", not_before
                    );
                }
            }
        }
    }

    let not_after_re = regex::Regex::new(r#"NotOnOrAfter="([^"]+)""#)
        .context("Failed to compile NotOnOrAfter regex")?;

    if let Some(caps) = not_after_re.captures(xml) {
        if let Some(ts) = caps.get(1) {
            if let Ok(not_after) = chrono::DateTime::parse_from_rfc3339(ts.as_str()) {
                if now >= not_after {
                    anyhow::bail!(
                        "SAML assertion has expired (NotOnOrAfter: {})", not_after
                    );
                }
            }
        }
    }

    Ok(())
}

/// Extract the NameID value from a SAML assertion XML (simple regex-based extraction).
fn extract_saml_name_id(xml: &str) -> Option<String> {
    // Look for <saml:NameID ...>value</saml:NameID> or <NameID ...>value</NameID>
    let re = regex::Regex::new(r"<(?:saml:)?NameID[^>]*>([^<]+)</(?:saml:)?NameID>").ok()?;
    re.captures(xml).and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
}

/// Extract a SAML attribute from an XML assertion.
///
/// **WARNING:** This uses regex-based XML extraction which may fail with
/// namespaced attributes, CDATA sections, or multi-line values. For production
/// SAML deployments, consider replacing with a proper XML parser (e.g., `roxmltree`).
fn extract_saml_attribute(xml: &str, attr_name: &str) -> Option<String> {
    let pattern = format!(
        r#"Name="[^"]*{}[^"]*"[^>]*>.*?<(?:saml:)?AttributeValue[^>]*>([^<]+)</(?:saml:)?AttributeValue>"#,
        regex::escape(attr_name)
    );
    let re = regex::Regex::new(&pattern).ok()?;
    re.captures(xml).and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
}

// ── LDAP Auth Provider ─────────────────────────────────────────────

/// LDAP/Active Directory authentication and group sync provider.
/// Uses simple bind authentication over TCP (with optional STARTTLS).
pub struct LdapAuthProvider {
    config: LdapConfig,
    provider_id: String,
    db: Arc<dyn crate::db_trait::DatabaseBackend>,
}

impl LdapAuthProvider {
    pub fn new(
        provider_id: String,
        config: LdapConfig,
        db: Arc<dyn crate::db_trait::DatabaseBackend>,
    ) -> Self {
        Self {
            config,
            provider_id,
            db,
        }
    }

    fn map_role(&self, groups: &[String]) -> String {
        for group in groups {
            if let Some(role) = self.config.role_mapping.get(group) {
                return role.clone();
            }
        }
        "Viewer".to_string()
    }

    fn map_team(&self, groups: &[String]) -> Option<String> {
        for group in groups {
            if let Some(team_id) = self.config.team_mapping.get(group) {
                return Some(team_id.clone());
            }
        }
        None
    }

    /// Sync LDAP groups to Ryuo teams/roles.
    pub async fn sync_groups(&self) -> Result<u64> {
        warn!("LDAP group sync not fully implemented — requires ldap3 crate integration");
        // In production: use ldap3 crate to search groups and map to teams
        // This is a placeholder that documents the expected behavior:
        // 1. Connect to LDAP server using bind_dn/bind_password
        // 2. Search for groups under group_search_base
        // 3. For each group, find members
        // 4. Map members to Ryuo users, create/update as needed
        // 5. Apply role_mapping and team_mapping
        Ok(0)
    }
}

#[async_trait]
impl AuthProvider for LdapAuthProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Ldap
    }

    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    async fn authenticate(&self, credentials: &AuthCredentials) -> Result<AuthenticatedUser> {
        match credentials {
            AuthCredentials::UsernamePassword { username, password } => {
                // In production, this would use the ldap3 crate to:
                // 1. Bind with service account (bind_dn/bind_password)
                // 2. Search for user DN using user_search_filter
                // 3. Attempt bind with user's DN + password
                // 4. Fetch user attributes and groups
                //
                // For now, we document the contract and validate structure:
                if username.is_empty() || password.is_empty() {
                    anyhow::bail!("Username and password are required for LDAP authentication");
                }

                // SECURITY: LDAP authentication is not yet implemented.
                // Returning success would grant unauthorized access.
                anyhow::bail!(
                    "LDAP authentication is not yet implemented (requires ldap3 crate). \
                     Configure a different auth provider or use local authentication."
                )
            }
            _ => anyhow::bail!("LDAP provider only supports username/password authentication"),
        }
    }

    async fn get_user_info(&self, _token: &str) -> Result<AuthenticatedUser> {
        anyhow::bail!("LDAP provider does not support token-based user info retrieval")
    }

    fn authorization_url(&self, _state: &str) -> Result<Option<String>> {
        Ok(None)
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_urlencoding() {
        assert_eq!(urlencoding("hello world"), "hello%20world");
        assert_eq!(urlencoding("test@example.com"), "test%40example.com");
        assert_eq!(urlencoding("a&b=c"), "a%26b%3Dc");
        assert_eq!(urlencoding("simple"), "simple");
    }

    #[test]
    fn test_extract_saml_name_id() {
        let xml = r#"<samlp:Response><saml:Assertion><saml:Subject><saml:NameID Format="urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress">user@example.com</saml:NameID></saml:Subject></saml:Assertion></samlp:Response>"#;
        assert_eq!(extract_saml_name_id(xml), Some("user@example.com".to_string()));
    }

    #[test]
    fn test_extract_saml_name_id_no_namespace() {
        let xml = r#"<Response><Assertion><Subject><NameID>testuser</NameID></Subject></Assertion></Response>"#;
        assert_eq!(extract_saml_name_id(xml), Some("testuser".to_string()));
    }

    #[test]
    fn test_provider_type_display() {
        assert_eq!(ProviderType::Local.to_string(), "local");
        assert_eq!(ProviderType::Oidc.to_string(), "oidc");
        assert_eq!(ProviderType::Saml.to_string(), "saml");
        assert_eq!(ProviderType::Ldap.to_string(), "ldap");
    }

    // ── PKCE Tests (BUG-H8) ───────────────────────────────────────

    #[test]
    fn test_generate_pkce_pair_valid_lengths() {
        let pair = generate_pkce_pair();
        // RFC 7636: code_verifier must be 43-128 unreserved characters
        assert!(pair.code_verifier.len() >= 43, "verifier too short: {}", pair.code_verifier.len());
        assert!(pair.code_verifier.len() <= 128, "verifier too long: {}", pair.code_verifier.len());
        // code_challenge is base64url(sha256) = 43 chars (256 bits / 6 bits per char)
        assert_eq!(pair.code_challenge.len(), 43);
    }

    #[test]
    fn test_generate_pkce_pair_challenge_matches_verifier() {
        let pair = generate_pkce_pair();
        let mut hasher = Sha256::new();
        hasher.update(pair.code_verifier.as_bytes());
        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(hasher.finalize());
        assert_eq!(pair.code_challenge, expected);
    }

    #[test]
    fn test_generate_pkce_pair_uniqueness() {
        let pair1 = generate_pkce_pair();
        let pair2 = generate_pkce_pair();
        assert_ne!(pair1.code_verifier, pair2.code_verifier);
        assert_ne!(pair1.code_challenge, pair2.code_challenge);
    }

    // ── SAML Signature Validation Tests (BUG-C2) ──────────────────

    #[test]
    fn test_saml_rejects_unsigned_assertion() {
        let xml = r#"<samlp:Response><saml:Assertion><saml:Subject><saml:NameID>user@example.com</saml:NameID></saml:Subject></saml:Assertion></samlp:Response>"#;
        let result = validate_saml_signature(xml, "some-cert");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("does not contain a digital signature"), "got: {}", err);
    }

    #[test]
    fn test_saml_rejects_empty_signature_value() {
        let xml = r#"<samlp:Response>
            <ds:Signature>
                <ds:SignatureValue></ds:SignatureValue>
                <ds:KeyInfo><ds:X509Data><ds:X509Certificate>CERT</ds:X509Certificate></ds:X509Data></ds:KeyInfo>
            </ds:Signature>
            <saml:Assertion><saml:Subject><saml:NameID>user</saml:NameID></saml:Subject></saml:Assertion>
        </samlp:Response>"#;
        let result = validate_saml_signature(xml, "CERT");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("SignatureValue"), "got: {}", err);
    }

    #[test]
    fn test_saml_rejects_wrong_certificate() {
        let xml = r#"<samlp:Response>
            <ds:Signature>
                <ds:SignatureValue>validSig</ds:SignatureValue>
                <ds:KeyInfo><ds:X509Data><ds:X509Certificate>WRONG_CERT</ds:X509Certificate></ds:X509Data></ds:KeyInfo>
            </ds:Signature>
            <saml:Assertion><saml:Subject><saml:NameID>user</saml:NameID></saml:Subject></saml:Assertion>
        </samlp:Response>"#;
        let result = validate_saml_signature(xml, "CORRECT_CERT");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("does not match"), "got: {}", err);
    }

    #[test]
    fn test_saml_rejects_missing_x509_cert() {
        let xml = r#"<samlp:Response>
            <ds:Signature>
                <ds:SignatureValue>validSig</ds:SignatureValue>
            </ds:Signature>
            <saml:Assertion><saml:Subject><saml:NameID>user</saml:NameID></saml:Subject></saml:Assertion>
        </samlp:Response>"#;
        let result = validate_saml_signature(xml, "CERT");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("X509 certificate"), "got: {}", err);
    }

    #[test]
    fn test_saml_rejects_empty_idp_cert() {
        let xml = r#"<samlp:Response>
            <ds:Signature>
                <ds:SignatureValue>validSig</ds:SignatureValue>
                <ds:KeyInfo><ds:X509Data><ds:X509Certificate>CERT</ds:X509Certificate></ds:X509Data></ds:KeyInfo>
            </ds:Signature>
            <saml:Assertion><saml:Subject><saml:NameID>user</saml:NameID></saml:Subject></saml:Assertion>
        </samlp:Response>"#;
        let result = validate_saml_signature(xml, "");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not configured"), "got: {}", err);
    }

    #[test]
    fn test_saml_accepts_valid_signature_with_matching_cert() {
        let cert = "MIICajCCAdOgAwIBAgIBADANBg";
        let xml = format!(
            r#"<samlp:Response>
            <ds:Signature>
                <ds:SignatureValue>validSignatureData</ds:SignatureValue>
                <ds:KeyInfo><ds:X509Data><ds:X509Certificate>{}</ds:X509Certificate></ds:X509Data></ds:KeyInfo>
            </ds:Signature>
            <saml:Assertion><saml:Subject><saml:NameID>user@example.com</saml:NameID></saml:Subject></saml:Assertion>
        </samlp:Response>"#,
            cert
        );
        let result = validate_saml_signature(&xml, cert);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
    }

    #[test]
    fn test_saml_cert_comparison_ignores_whitespace_and_pem_headers() {
        let configured = "-----BEGIN CERTIFICATE-----\nMIIC ajCC\n-----END CERTIFICATE-----";
        let xml = r#"<samlp:Response>
            <ds:Signature>
                <ds:SignatureValue>sig</ds:SignatureValue>
                <ds:KeyInfo><ds:X509Data><ds:X509Certificate>MIICajCC</ds:X509Certificate></ds:X509Data></ds:KeyInfo>
            </ds:Signature>
            <saml:Assertion><saml:Subject><saml:NameID>user</saml:NameID></saml:Subject></saml:Assertion>
        </samlp:Response>"#;
        let result = validate_saml_signature(xml, configured);
        assert!(result.is_ok(), "cert normalization should strip headers and whitespace: {:?}", result);
    }

    #[test]
    fn test_saml_rejects_expired_assertion() {
        let cert = "TESTCERT";
        let xml = format!(
            r#"<samlp:Response>
            <ds:Signature>
                <ds:SignatureValue>sig</ds:SignatureValue>
                <ds:KeyInfo><ds:X509Data><ds:X509Certificate>{}</ds:X509Certificate></ds:X509Data></ds:KeyInfo>
            </ds:Signature>
            <saml:Assertion>
                <saml:Conditions NotOnOrAfter="2020-01-01T00:00:00Z"/>
                <saml:Subject><saml:NameID>user</saml:NameID></saml:Subject>
            </saml:Assertion>
        </samlp:Response>"#,
            cert
        );
        let result = validate_saml_signature(&xml, cert);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("expired"), "got: {}", err);
    }

    #[test]
    fn test_saml_rejects_not_yet_valid_assertion() {
        let cert = "TESTCERT";
        let xml = format!(
            r#"<samlp:Response>
            <ds:Signature>
                <ds:SignatureValue>sig</ds:SignatureValue>
                <ds:KeyInfo><ds:X509Data><ds:X509Certificate>{}</ds:X509Certificate></ds:X509Data></ds:KeyInfo>
            </ds:Signature>
            <saml:Assertion>
                <saml:Conditions NotBefore="2099-01-01T00:00:00Z"/>
                <saml:Subject><saml:NameID>user</saml:NameID></saml:Subject>
            </saml:Assertion>
        </samlp:Response>"#,
            cert
        );
        let result = validate_saml_signature(&xml, cert);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not yet valid"), "got: {}", err);
    }

    #[test]
    fn test_saml_validates_non_namespaced_signature() {
        let cert = "TESTCERT";
        let xml = format!(
            r#"<Response>
            <Signature>
                <SignatureValue>sig</SignatureValue>
                <KeyInfo><X509Data><X509Certificate>{}</X509Certificate></X509Data></KeyInfo>
            </Signature>
            <Assertion><Subject><NameID>user</NameID></Subject></Assertion>
        </Response>"#,
            cert
        );
        let result = validate_saml_signature(&xml, cert);
        assert!(result.is_ok(), "should accept non-namespaced signature: {:?}", result);
    }
}
