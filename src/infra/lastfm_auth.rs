//! Last.fm desktop authentication API: application keys, request tokens, browser authorization URLs, and session polling.
//!
//! Implements the Last.fm Desktop Application Authentication flow
//! (<https://www.last.fm/api/desktopauth>).

use anyhow::Context;

use super::scrobble_lastfm::{LASTFM_API_URL, api_signature};

/// Last.fm desktop authentication web URL base.
pub const LASTFM_AUTH_URL: &str = "https://www.last.fm/api/auth/";

/// Compile-time environment variable name for the Last.fm application API key.
///
/// Read at compile time by `LastFmAppKeys::from_build_env`.
pub const LASTFM_API_KEY_ENV: &str = "MOGGYT_LASTFM_API_KEY";

/// Compile-time environment variable name for the Last.fm application shared secret.
///
/// Read at compile time by `LastFmAppKeys::from_build_env`.
pub const LASTFM_SECRET_ENV: &str = "MOGGYT_LASTFM_SECRET";

/// Build-time Last.fm application credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastFmAppKeys {
    pub api_key: String,
    pub secret: String,
}

impl LastFmAppKeys {
    /// Constructs application keys from optional raw string parts.
    ///
    /// Returns `None` when either part is missing or blank after trimming.
    pub fn from_parts(api_key: Option<&str>, secret: Option<&str>) -> Option<Self> {
        let key = api_key?.trim();
        let sec = secret?.trim();
        let is_valid = !key.is_empty() && !sec.is_empty();
        is_valid.then(|| Self { api_key: key.to_string(), secret: sec.to_string() })
    }

    /// Reads application keys embedded at compile time via environment variables.
    pub fn from_build_env() -> Option<Self> {
        Self::from_parts(option_env!("MOGGYT_LASTFM_API_KEY"), option_env!("MOGGYT_LASTFM_SECRET"))
    }
}

/// Resolves Last.fm application keys from settings, falling back to compile-time build environment.
pub fn resolve_app_keys(settings_key: &str, settings_secret: &str) -> Option<LastFmAppKeys> {
    LastFmAppKeys::from_parts(Some(settings_key), Some(settings_secret))
        .or_else(LastFmAppKeys::from_build_env)
}

/// Builds the web browser authorization URL for an unauthorized desktop request token.
pub fn authorize_url(api_key: &str, token: &str) -> String {
    format!("{LASTFM_AUTH_URL}?api_key={api_key}&token={token}")
}

/// Authenticated Last.fm user session information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastFmSession {
    pub username: String,
    pub session_key: String,
}

/// Result of polling Last.fm for an authenticated desktop session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionPoll {
    /// User authorized the desktop token; session key is ready.
    Ready(LastFmSession),
    /// User has not yet completed authorization in the browser (error 14).
    Pending,
    /// Authorization failed or an unrecoverable error occurred.
    Failed(String),
}

fn extract_lastfm_error(val: &serde_json::Value) -> Option<String> {
    let err = val.get("error")?;
    let code = match err.as_i64() {
        Some(n) => n.to_string(),
        None => err.as_str().unwrap_or("unknown").to_string(),
    };
    let msg = val.get("message").and_then(|v| v.as_str()).unwrap_or("unknown error");
    Some(format!("Last.fm error {code}: {msg}"))
}

/// Parses the JSON response body from `auth.getToken`.
pub fn parse_token_response(body: &str) -> anyhow::Result<String> {
    let val: serde_json::Value = serde_json::from_str(body)?;
    if let Some(token) = val.get("token").and_then(|v| v.as_str()) {
        return Ok(token.to_string());
    }
    if let Some(err_msg) = extract_lastfm_error(&val) {
        anyhow::bail!("{err_msg}");
    }
    anyhow::bail!("missing token in Last.fm response")
}

fn parse_ready_session(val: &serde_json::Value) -> Option<LastFmSession> {
    let session = val.get("session")?;
    let username = session.get("name")?.as_str()?;
    let session_key = session.get("key")?.as_str()?;
    Some(LastFmSession { username: username.to_string(), session_key: session_key.to_string() })
}

fn is_pending_error(val: &serde_json::Value) -> bool {
    let Some(err) = val.get("error") else {
        return false;
    };
    err.as_i64() == Some(14) || err.as_str() == Some("14")
}

/// Parses the JSON response body from `auth.getSession`.
pub fn parse_session_response(body: &str) -> SessionPoll {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(body) else {
        return SessionPoll::Failed("failed to parse Last.fm response as JSON".to_string());
    };
    if let Some(session) = parse_ready_session(&val) {
        return SessionPoll::Ready(session);
    }
    if is_pending_error(&val) {
        return SessionPoll::Pending;
    }
    let err_msg = extract_lastfm_error(&val)
        .unwrap_or_else(|| "missing session object in Last.fm response".to_string());
    SessionPoll::Failed(err_msg)
}

/// Builds the signed query parameters for the `auth.getToken` API method.
pub fn token_request_params(keys: &LastFmAppKeys) -> Vec<(String, String)> {
    let params = [("api_key", keys.api_key.as_str()), ("method", "auth.getToken")];
    let sig = api_signature(&params, &keys.secret);
    vec![
        ("api_key".to_string(), keys.api_key.clone()),
        ("method".to_string(), "auth.getToken".to_string()),
        ("api_sig".to_string(), sig),
        ("format".to_string(), "json".to_string()),
    ]
}

/// Builds the signed query parameters for the `auth.getSession` API method.
pub fn session_request_params(keys: &LastFmAppKeys, token: &str) -> Vec<(String, String)> {
    let params =
        [("api_key", keys.api_key.as_str()), ("method", "auth.getSession"), ("token", token)];
    let sig = api_signature(&params, &keys.secret);
    vec![
        ("api_key".to_string(), keys.api_key.clone()),
        ("method".to_string(), "auth.getSession".to_string()),
        ("token".to_string(), token.to_string()),
        ("api_sig".to_string(), sig),
        ("format".to_string(), "json".to_string()),
    ]
}

// NOTE(clippy): tracing macro expansion inflates cognitive complexity.
#[allow(clippy::cognitive_complexity)]
fn auth_http_client() -> reqwest::blocking::Client {
    match reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(10)).build() {
        Ok(client) => client,
        Err(err) => {
            tracing::warn!("failed to build Last.fm HTTP client: {err}; using fallback");
            reqwest::blocking::Client::new()
        }
    }
}

/// Requests an unauthorized request token from Last.fm.
///
/// Sends a blocking HTTP GET request to `LASTFM_API_URL`. Must be called
/// from a background worker thread, not the UI thread.
pub fn request_token(keys: &LastFmAppKeys) -> anyhow::Result<String> {
    let client = auth_http_client();
    let params = token_request_params(keys);
    let resp = client
        .get(LASTFM_API_URL)
        .query(&params)
        .send()
        .map_err(reqwest::Error::without_url)
        .context("Last.fm token request transport error")?;
    let body = resp
        .text()
        .map_err(reqwest::Error::without_url)
        .context("failed to read Last.fm token response body")?;
    parse_token_response(&body)
}

/// Polls Last.fm for an authenticated session using an authorized request token.
///
/// Sends a blocking HTTP GET request to `LASTFM_API_URL`. Must be called
/// from a background worker thread, not the UI thread.
pub fn request_session(keys: &LastFmAppKeys, token: &str) -> SessionPoll {
    let client = auth_http_client();
    let params = session_request_params(keys, token);
    let resp = match client.get(LASTFM_API_URL).query(&params).send() {
        Ok(r) => r,
        Err(err) => {
            let err = err.without_url();
            return SessionPoll::Failed(format!("Last.fm session transport error: {err}"));
        }
    };
    let body = match resp.text() {
        Ok(b) => b,
        Err(err) => {
            let err = err.without_url();
            return SessionPoll::Failed(format!("failed to read Last.fm session response: {err}"));
        }
    };
    parse_session_response(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_time_env_constants() {
        assert_eq!(LASTFM_API_KEY_ENV, "MOGGYT_LASTFM_API_KEY");
        assert_eq!(LASTFM_SECRET_ENV, "MOGGYT_LASTFM_SECRET");
        assert_eq!(LASTFM_AUTH_URL, "https://www.last.fm/api/auth/");
    }

    #[test]
    fn test_app_keys_from_parts() {
        assert_eq!(LastFmAppKeys::from_parts(None, None), None);
        assert_eq!(LastFmAppKeys::from_parts(Some("key"), None), None);
        assert_eq!(LastFmAppKeys::from_parts(None, Some("sec")), None);
        assert_eq!(LastFmAppKeys::from_parts(Some(""), Some("sec")), None);
        assert_eq!(LastFmAppKeys::from_parts(Some("key"), Some("   ")), None);

        let parsed = LastFmAppKeys::from_parts(Some("  my_key  "), Some("  my_secret  "));
        assert_eq!(
            parsed,
            Some(LastFmAppKeys { api_key: "my_key".to_string(), secret: "my_secret".to_string() })
        );
    }

    #[test]
    fn test_app_keys_from_build_env_runs() {
        // Option_env! may be None in local dev without env vars set, but must not panic.
        let _ = LastFmAppKeys::from_build_env();
    }

    #[test]
    fn test_authorize_url() {
        let url = authorize_url("api_key_123", "tok_456");
        assert_eq!(url, "https://www.last.fm/api/auth/?api_key=api_key_123&token=tok_456");
    }

    #[test]
    fn test_token_request_params_content_and_signature() {
        let keys =
            LastFmAppKeys { api_key: "test_key".to_string(), secret: "test_secret".to_string() };
        let params = token_request_params(&keys);

        assert!(params.contains(&("api_key".to_string(), "test_key".to_string())));
        assert!(params.contains(&("method".to_string(), "auth.getToken".to_string())));
        assert!(params.contains(&("format".to_string(), "json".to_string())));

        let sig = params
            .iter()
            .find(|(k, _)| k == "api_sig")
            .map(|(_, v)| v.as_str())
            .expect("api_sig parameter present");

        let non_format: Vec<(&str, &str)> = params
            .iter()
            .filter(|(k, _)| k != "format" && k != "api_sig")
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let expected_sig = api_signature(&non_format, &keys.secret);
        assert_eq!(sig, expected_sig);
    }

    #[test]
    fn test_session_request_params_content_and_signature() {
        let keys =
            LastFmAppKeys { api_key: "test_key".to_string(), secret: "test_secret".to_string() };
        let params = session_request_params(&keys, "authorized_token");

        assert!(params.contains(&("api_key".to_string(), "test_key".to_string())));
        assert!(params.contains(&("method".to_string(), "auth.getSession".to_string())));
        assert!(params.contains(&("token".to_string(), "authorized_token".to_string())));
        assert!(params.contains(&("format".to_string(), "json".to_string())));

        let sig = params
            .iter()
            .find(|(k, _)| k == "api_sig")
            .map(|(_, v)| v.as_str())
            .expect("api_sig parameter present");

        let non_format: Vec<(&str, &str)> = params
            .iter()
            .filter(|(k, _)| k != "format" && k != "api_sig")
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let expected_sig = api_signature(&non_format, &keys.secret);
        assert_eq!(sig, expected_sig);
    }

    #[test]
    fn test_parse_token_response_ok() {
        let json = r#"{"token":"0123456789abcdef"}"#;
        assert_eq!(parse_token_response(json).unwrap(), "0123456789abcdef");
    }

    #[test]
    fn test_parse_token_response_error() {
        let json = r#"{"error":4,"message":"Invalid API key - You must be granted a valid key"}"#;
        let err = parse_token_response(json).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Last.fm error 4: Invalid API key - You must be granted a valid key"
        );
    }

    #[test]
    fn test_parse_token_response_garbage_and_missing() {
        assert!(parse_token_response("not-json").is_err());
        assert!(parse_token_response("{}").is_err());
    }

    #[test]
    fn test_parse_session_response_ready() {
        let json = r#"{"session":{"name":"testuser","key":"session_key_abc","subscriber":0}}"#;
        assert_eq!(
            parse_session_response(json),
            SessionPoll::Ready(LastFmSession {
                username: "testuser".to_string(),
                session_key: "session_key_abc".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_session_response_pending_error_14() {
        let json = r#"{"error":14,"message":"This token has not been authorized"}"#;
        assert_eq!(parse_session_response(json), SessionPoll::Pending);
    }

    #[test]
    fn test_parse_session_response_failed_other_error() {
        let json = r#"{"error":4,"message":"Invalid API key"}"#;
        assert_eq!(
            parse_session_response(json),
            SessionPoll::Failed("Last.fm error 4: Invalid API key".to_string())
        );
    }

    #[test]
    fn test_parse_session_response_garbage_and_missing() {
        assert!(matches!(parse_session_response("not-valid-json"), SessionPoll::Failed(_)));
        assert!(matches!(parse_session_response("{}"), SessionPoll::Failed(_)));
    }

    #[test]
    fn test_resolve_app_keys_prefers_custom_and_trims() {
        let keys = resolve_app_keys(" my_key ", " my_secret ").expect("resolved keys");
        assert_eq!(keys.api_key, "my_key");
        assert_eq!(keys.secret, "my_secret");

        // Blank settings key/secret falls back to build env (which is None in test environment)
        assert_eq!(resolve_app_keys("", "sec"), LastFmAppKeys::from_build_env());
        assert_eq!(resolve_app_keys("key", "   "), LastFmAppKeys::from_build_env());
        assert_eq!(resolve_app_keys("", ""), LastFmAppKeys::from_build_env());
    }
}
