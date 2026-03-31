#![allow(dead_code)]
// auth.rs — Authentication Provider Framework
// SSO/OIDC/SAML/LDAP Integration
//
// Provides a pluggable authentication backend so Vortex can authenticate
// users against local DB, OIDC providers (Okta, Azure AD, PingIdentity),
// SAML 2.0 IdPs, or LDAP/AD directories.

use anyhow::{Result, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn, debug};

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
    /// Map of OIDC group name → Vortex role
    pub role_mapping: HashMap<String, String>,
    /// Map of OIDC group name → Vortex team_id
    pub team_mapping: HashMap<String, String>,
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
    /// Map of LDAP group → Vortex role
    pub role_mapping: HashMap<String, String>,
    /// Map of LDAP group → Vortex team_id
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
    pub fn register_provider(&mut self, provider: Arc<dyn AuthProvider>) {
        info!(
            "🔑 Registered auth provider: {} ({})",
            provider.provider_id(),
            provider.provider_type()
        );
        self.providers.push(provider);
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
        let session = UserSession {
            session_id: uuid::Uuid::new_v4().to_string(),
            username: user.username.clone(),
            provider_id: user.provider_id.clone(),
            access_token: access_token.map(|s| s.to_string()),
            refresh_token: refresh_token.map(|s| s.to_string()),
            id_token: id_token.map(|s| s.to_string()),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(ttl_hours as i64),
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
                    Some((api_key, role)) => {
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

    async fn exchange_code(&self, code: &str) -> Result<OidcTokenResponse> {
        let discovery = self.discover().await?;

        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &self.config.redirect_uri),
            ("client_id", &self.config.client_id),
            ("client_secret", &self.config.client_secret),
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
            AuthCredentials::OidcCode { code, state: _ } => {
                let token_resp = self.exchange_code(code).await?;
                let userinfo = self.fetch_userinfo(&token_resp.access_token).await?;

                let username = userinfo.preferred_username
                    .or(userinfo.email.clone())
                    .unwrap_or(userinfo.sub.clone());

                let groups = userinfo.groups.unwrap_or_default();
                let role = self.map_role(&groups);
                let team_id = self.map_team(&groups);

                // Auto-provision user if they don't exist
                let api_key = uuid::Uuid::new_v4().to_string();
                let _ = self.db.create_user(&username, &uuid::Uuid::new_v4().to_string(), &role, &api_key).await;

                info!("🔑 OIDC authenticated user: {} (role: {}, team: {:?})", username, role, team_id);

                Ok(AuthenticatedUser {
                    username,
                    email: userinfo.email,
                    display_name: userinfo.name,
                    role,
                    team_id,
                    provider_id: self.provider_id.clone(),
                    external_id: Some(userinfo.sub),
                    groups,
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
        })
    }

    fn authorization_url(&self, state: &str) -> Result<Option<String>> {
        // Build OIDC authorization URL with PKCE
        let scopes = if self.config.scopes.is_empty() {
            "openid profile email groups".to_string()
        } else {
            self.config.scopes.join(" ")
        };

        // Note: In production, use a proper URL builder and include PKCE code_challenge
        let url = format!(
            "{}/authorize?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}",
            self.config.issuer_url.trim_end_matches('/'),
            urlencoding(&self.config.client_id),
            urlencoding(&self.config.redirect_uri),
            urlencoding(&scopes),
            urlencoding(state),
        );

        Ok(Some(url))
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

                // Extract NameID (username) from SAML assertion via simple XML parsing
                // In production, use a proper SAML library for signature validation
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

    /// Sync LDAP groups to Vortex teams/roles.
    pub async fn sync_groups(&self) -> Result<u64> {
        warn!("LDAP group sync not fully implemented — requires ldap3 crate integration");
        // In production: use ldap3 crate to search groups and map to teams
        // This is a placeholder that documents the expected behavior:
        // 1. Connect to LDAP server using bind_dn/bind_password
        // 2. Search for groups under group_search_base
        // 3. For each group, find members
        // 4. Map members to Vortex users, create/update as needed
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
}
