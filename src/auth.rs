//! Livy OAuth login and MCP authorization.

use axum::{
    Router,
    body::Body,
    extract::{Query, Request},
    http::{
        HeaderMap, HeaderValue, Method, StatusCode,
        header::{AUTHORIZATION, COOKIE, SET_COOKIE, WWW_AUTHENTICATE},
    },
    response::{IntoResponse, Json, Redirect, Response},
    routing::{get, post},
};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, TokenUrl,
    basic::BasicClient,
};
use rand::{Rng, distr::Alphanumeric};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashMap,
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tower::{Layer, Service};

use crate::errors::FetchError;

#[derive(Debug)]
pub struct AuthState {
    http: reqwest::Client,
    client_id: String,
    client_secret: String,
    auth_url: String,
    token_url: String,
    redirect_url: String,
    introspection_url: Option<String>,
    scopes: Vec<String>,
    cookie_name: String,
    cookie_secure: bool,
    session_ttl: Duration,
    state_ttl: Duration,
    introspection_cache_ttl: Duration,
    pending: Mutex<HashMap<String, PendingLogin>>,
    sessions: Mutex<HashMap<String, Session>>,
    bearer_cache: Mutex<HashMap<String, BearerCacheEntry>>,
}

#[derive(Debug)]
struct PendingLogin {
    pkce_verifier: String,
    next: String,
    expires_at: Instant,
}

#[derive(Clone, Debug)]
struct Session {
    subject: Option<String>,
    expires_at: Instant,
}

#[derive(Clone, Debug)]
struct BearerCacheEntry {
    active: bool,
    expires_at: Instant,
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoginQuery {
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IntrospectionResponse {
    active: bool,
    sub: Option<String>,
    username: Option<String>,
    email: Option<String>,
}

impl AuthState {
    pub fn from_env() -> Result<Option<Arc<Self>>, FetchError> {
        let configured = env_present("LIVY_OAUTH_CLIENT_ID")
            || env_present("LIVY_OAUTH_AUTH_URL")
            || env_present("LIVY_OAUTH_ENABLED");
        let enabled = env_bool("LIVY_OAUTH_ENABLED")?.unwrap_or(configured);
        if !enabled {
            return Ok(None);
        }

        let client_id = required_env("LIVY_OAUTH_CLIENT_ID")?;
        let client_secret = required_env("LIVY_OAUTH_CLIENT_SECRET")?;
        let auth_url = required_env("LIVY_OAUTH_AUTH_URL")?;
        let token_url = required_env("LIVY_OAUTH_TOKEN_URL")?;
        let redirect_url = required_env("LIVY_OAUTH_REDIRECT_URL")?;
        let introspection_url = optional_env("LIVY_OAUTH_INTROSPECTION_URL");
        let scopes = env_or("LIVY_OAUTH_SCOPES", "openid profile email")
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let cookie_name = env_or("LIVY_OAUTH_COOKIE_NAME", "livy_resolver_session");
        let cookie_secure = env_bool("LIVY_OAUTH_COOKIE_SECURE")?.unwrap_or(false);
        let session_ttl =
            Duration::from_secs(env_u64("LIVY_OAUTH_SESSION_TTL_SECS")?.unwrap_or(28_800));
        let state_ttl = Duration::from_secs(env_u64("LIVY_OAUTH_STATE_TTL_SECS")?.unwrap_or(600));
        let introspection_cache_ttl =
            Duration::from_secs(env_u64("LIVY_OAUTH_INTROSPECTION_CACHE_SECS")?.unwrap_or(60));

        validate_url("LIVY_OAUTH_AUTH_URL", &auth_url, AuthUrl::new)?;
        validate_url("LIVY_OAUTH_TOKEN_URL", &token_url, TokenUrl::new)?;
        validate_url("LIVY_OAUTH_REDIRECT_URL", &redirect_url, RedirectUrl::new)?;

        let http = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(FetchError::UnableFetch)?;

        Ok(Some(Arc::new(Self {
            http,
            client_id,
            client_secret,
            auth_url,
            token_url,
            redirect_url,
            introspection_url,
            scopes,
            cookie_name,
            cookie_secure,
            session_ttl,
            state_ttl,
            introspection_cache_ttl,
            pending: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            bearer_cache: Mutex::new(HashMap::new()),
        })))
    }

    async fn login(self: Arc<Self>, Query(query): Query<LoginQuery>) -> Response {
        self.prune_expired();

        let next = sanitize_next(query.next.as_deref());
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let client = self.oauth_client();
        let mut authorize = client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge);
        for scope in &self.scopes {
            authorize = authorize.add_scope(Scope::new(scope.clone()));
        }
        let (auth_url, csrf) = authorize.url();

        self.pending
            .lock()
            .expect("oauth pending mutex poisoned")
            .insert(
                csrf.secret().to_string(),
                PendingLogin {
                    pkce_verifier: pkce_verifier.secret().to_string(),
                    next,
                    expires_at: Instant::now() + self.state_ttl,
                },
            );

        Redirect::temporary(auth_url.as_ref()).into_response()
    }

    async fn callback(self: Arc<Self>, Query(query): Query<CallbackQuery>) -> Response {
        if let Some(error) = query.error {
            let description = query.error_description.unwrap_or_default();
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": error, "error_description": description })),
            )
                .into_response();
        }

        let Some(code) = query.code else {
            return bad_auth_request("missing OAuth code");
        };
        let Some(state) = query.state else {
            return bad_auth_request("missing OAuth state");
        };

        let pending = {
            let mut pending = self.pending.lock().expect("oauth pending mutex poisoned");
            pending.remove(&state)
        };
        let Some(pending) = pending else {
            return bad_auth_request("invalid OAuth state");
        };
        if pending.expires_at <= Instant::now() {
            return bad_auth_request("expired OAuth state");
        }

        let token = match self
            .oauth_client()
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(PkceCodeVerifier::new(pending.pkce_verifier))
            .request_async(&self.http)
            .await
        {
            Ok(token) => token,
            Err(err) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": format!("Livy token exchange failed: {err}") })),
                )
                    .into_response();
            }
        };

        let access_token = token.access_token().secret().to_string();
        let subject = match self.introspect_token(&access_token).await {
            Ok(Some(subject)) => Some(subject),
            Ok(None) => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "Livy token is not active" })),
                )
                    .into_response();
            }
            Err(err) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": format!("Livy token validation failed: {err}") })),
                )
                    .into_response();
            }
        };

        let session_id = random_token(48);
        let ttl = token
            .expires_in()
            .unwrap_or(self.session_ttl)
            .min(self.session_ttl);
        self.sessions
            .lock()
            .expect("oauth sessions mutex poisoned")
            .insert(
                session_id.clone(),
                Session {
                    subject,
                    expires_at: Instant::now() + ttl,
                },
            );

        let mut response = Redirect::temporary(&pending.next).into_response();
        response.headers_mut().insert(
            SET_COOKIE,
            HeaderValue::from_str(&self.session_cookie(&session_id, ttl))
                .expect("session cookie should be valid"),
        );
        response
    }

    async fn logout(self: Arc<Self>, headers: HeaderMap) -> Response {
        if let Some(session_id) = self.session_id_from_headers(&headers) {
            self.sessions
                .lock()
                .expect("oauth sessions mutex poisoned")
                .remove(&session_id);
        }

        let mut response = Json(json!({ "ok": true })).into_response();
        response.headers_mut().insert(
            SET_COOKIE,
            HeaderValue::from_str(&self.expired_session_cookie())
                .expect("expired session cookie should be valid"),
        );
        response
    }

    async fn me(self: Arc<Self>, headers: HeaderMap) -> Response {
        match self.session_from_headers(&headers) {
            Some(session) => Json(json!({
                "authenticated": true,
                "subject": session.subject,
            }))
            .into_response(),
            None => (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "authenticated": false })),
            )
                .into_response(),
        }
    }

    async fn authorize_parts(
        &self,
        method: Method,
        headers: HeaderMap,
    ) -> Result<(), AuthDecision> {
        self.prune_expired();

        if self.session_from_headers(&headers).is_some() {
            return Ok(());
        }

        if let Some(token) = bearer_token(&headers).map(str::to_string) {
            match self.authorize_bearer(&token).await {
                Ok(true) => return Ok(()),
                Ok(false) => return Err(AuthDecision::Unauthorized),
                Err(_) => return Err(AuthDecision::Unavailable),
            }
        }

        if method == Method::GET {
            Err(AuthDecision::RedirectToLogin)
        } else {
            Err(AuthDecision::Unauthorized)
        }
    }

    async fn authorize_bearer(&self, token: &str) -> Result<bool, reqwest::Error> {
        if self.introspection_url.is_none() {
            return Ok(false);
        }

        let now = Instant::now();
        if let Some(cached) = self
            .bearer_cache
            .lock()
            .expect("oauth bearer cache mutex poisoned")
            .get(token)
            .cloned()
            .filter(|entry| entry.expires_at > now)
        {
            return Ok(cached.active);
        }

        let active = self.introspect_token(token).await?.is_some();
        self.bearer_cache
            .lock()
            .expect("oauth bearer cache mutex poisoned")
            .insert(
                token.to_string(),
                BearerCacheEntry {
                    active,
                    expires_at: Instant::now() + self.introspection_cache_ttl,
                },
            );
        Ok(active)
    }

    async fn introspect_token(&self, token: &str) -> Result<Option<String>, reqwest::Error> {
        let Some(url) = self.introspection_url.as_ref() else {
            return Ok(Some("livy-oauth-user".to_string()));
        };
        let response = self
            .http
            .post(url)
            .basic_auth(&self.client_id, Some(&self.client_secret))
            .form(&[("token", token)])
            .send()
            .await?
            .error_for_status()?
            .json::<IntrospectionResponse>()
            .await?;

        if !response.active {
            return Ok(None);
        }

        Ok(response
            .sub
            .or(response.username)
            .or(response.email)
            .or_else(|| Some("livy-oauth-user".to_string())))
    }

    fn session_from_headers(&self, headers: &HeaderMap) -> Option<Session> {
        let session_id = self.session_id_from_headers(headers)?;
        self.sessions
            .lock()
            .expect("oauth sessions mutex poisoned")
            .get(&session_id)
            .cloned()
            .filter(|session| session.expires_at > Instant::now())
    }

    fn session_id_from_headers(&self, headers: &HeaderMap) -> Option<String> {
        let cookie = headers.get(COOKIE)?.to_str().ok()?;
        parse_cookie(cookie, &self.cookie_name)
    }

    fn session_cookie(&self, session_id: &str, ttl: Duration) -> String {
        let secure = if self.cookie_secure { "; Secure" } else { "" };
        format!(
            "{}={}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax{}",
            self.cookie_name,
            session_id,
            ttl.as_secs(),
            secure
        )
    }

    fn expired_session_cookie(&self) -> String {
        let secure = if self.cookie_secure { "; Secure" } else { "" };
        format!(
            "{}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax{}",
            self.cookie_name, secure
        )
    }

    fn prune_expired(&self) {
        let now = Instant::now();
        self.pending
            .lock()
            .expect("oauth pending mutex poisoned")
            .retain(|_, login| login.expires_at > now);
        self.sessions
            .lock()
            .expect("oauth sessions mutex poisoned")
            .retain(|_, session| session.expires_at > now);
        self.bearer_cache
            .lock()
            .expect("oauth bearer cache mutex poisoned")
            .retain(|_, entry| entry.expires_at > now);
    }

    fn oauth_client(
        &self,
    ) -> BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet> {
        BasicClient::new(ClientId::new(self.client_id.clone()))
            .set_client_secret(ClientSecret::new(self.client_secret.clone()))
            .set_auth_uri(AuthUrl::new(self.auth_url.clone()).expect("validated auth url"))
            .set_token_uri(TokenUrl::new(self.token_url.clone()).expect("validated token url"))
            .set_redirect_uri(
                RedirectUrl::new(self.redirect_url.clone()).expect("validated redirect url"),
            )
    }
}

pub fn router(auth: Arc<AuthState>) -> Router {
    Router::new()
        .route(
            "/auth/livy/login",
            get({
                let auth = auth.clone();
                move |query| auth.clone().login(query)
            }),
        )
        .route(
            "/auth/livy/callback",
            get({
                let auth = auth.clone();
                move |query| auth.clone().callback(query)
            }),
        )
        .route(
            "/auth/livy/logout",
            post({
                let auth = auth.clone();
                move |headers| auth.clone().logout(headers)
            }),
        )
        .route(
            "/auth/livy/me",
            get({
                let auth = auth.clone();
                move |headers| auth.clone().me(headers)
            }),
        )
}

#[derive(Clone)]
pub struct AuthLayer {
    auth: Arc<AuthState>,
}

impl AuthLayer {
    pub fn new(auth: Arc<AuthState>) -> Self {
        Self { auth }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthService {
            inner,
            auth: self.auth.clone(),
        }
    }
}

#[derive(Clone)]
pub struct AuthService<S> {
    inner: S,
    auth: Arc<AuthState>,
}

impl<S> Service<Request<Body>> for AuthService<S>
where
    S: Service<Request<Body>, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let auth = self.auth.clone();
        let method = req.method().clone();
        let headers = req.headers().clone();
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        Box::pin(async move {
            match auth.authorize_parts(method, headers).await {
                Ok(()) => inner.call(req).await,
                Err(AuthDecision::RedirectToLogin) => {
                    Ok(Redirect::temporary("/auth/livy/login?next=/mcp").into_response())
                }
                Err(AuthDecision::Unauthorized) => Ok(unauthorized_response()),
                Err(AuthDecision::Unavailable) => Ok((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({ "error": "Livy token validation is unavailable" })),
                )
                    .into_response()),
            }
        })
    }
}

enum AuthDecision {
    RedirectToLogin,
    Unauthorized,
    Unavailable,
}

fn unauthorized_response() -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": "Livy login required",
            "login_url": "/auth/livy/login?next=/mcp"
        })),
    )
        .into_response();
    response.headers_mut().insert(
        WWW_AUTHENTICATE,
        HeaderValue::from_static("Bearer realm=\"livy-resolver-mcp\""),
    );
    response
}

fn bad_auth_request(message: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": message }))).into_response()
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .filter(|token| !token.trim().is_empty())
}

fn parse_cookie(header: &str, name: &str) -> Option<String> {
    header.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key == name && !value.is_empty()).then(|| value.to_string())
    })
}

fn sanitize_next(next: Option<&str>) -> String {
    match next {
        Some(value) if value.starts_with('/') && !value.starts_with("//") => value.to_string(),
        _ => "/mcp".to_string(),
    }
}

fn random_token(len: usize) -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn env_present(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

fn optional_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn required_env(name: &'static str) -> Result<String, FetchError> {
    optional_env(name).ok_or_else(|| FetchError::BadRequest(format!("{name} must be set")))
}

fn env_or(name: &str, default: &str) -> String {
    optional_env(name).unwrap_or_else(|| default.to_string())
}

fn env_bool(name: &str) -> Result<Option<bool>, FetchError> {
    optional_env(name)
        .map(|value| match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => Err(FetchError::BadRequest(format!(
                "{name} must be true or false"
            ))),
        })
        .transpose()
}

fn env_u64(name: &str) -> Result<Option<u64>, FetchError> {
    optional_env(name)
        .map(|value| {
            value
                .parse()
                .map_err(|err| FetchError::BadRequest(format!("{name} must be a number: {err}")))
        })
        .transpose()
}

fn validate_url<T, E, F>(name: &'static str, value: &str, parse: F) -> Result<(), FetchError>
where
    F: FnOnce(String) -> Result<T, E>,
    E: std::fmt::Display,
{
    parse(value.to_string())
        .map(|_| ())
        .map_err(|err| FetchError::BadRequest(format!("invalid {name}: {err}")))
}
