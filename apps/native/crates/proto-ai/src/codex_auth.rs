//! The Codex CLI login bridge (reference `codex-credentials.ts` + pi-ai's OpenAI Codex OAuth refresh).
//! The ChatGPT OAuth credential lives in `$CODEX_HOME/auth.json`; it is re-read on every use (a `codex login` is
//! picked up live) and a refresh is written back in the CLI's own format so both never hold diverging refresh
//! tokens. Tokens never appear in logs or error messages.

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{Map, Value};

use crate::AiError;

/// pi-ai's OpenAI Codex OAuth client.
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
/// Refresh when the access token expires within this window (pi-ai's minimum validity).
pub const MIN_VALIDITY_MS: i64 = 5 * 60 * 1000;
const JWT_CLAIM_PATH: &str = "https://api.openai.com/auth";

/// What a request needs from the login. Not `Debug`: it holds secrets.
#[derive(Clone)]
pub struct Credential {
    pub access: String,
    pub refresh: String,
    pub account_id: String,
    /// Access token expiry, epoch ms (0 = unknown → refresh now).
    pub expires_ms: i64,
}

impl Credential {
    pub fn needs_refresh(&self, now_ms: i64) -> bool {
        now_ms + MIN_VALIDITY_MS >= self.expires_ms
    }
}

pub fn auth_file(codex_home: &Path) -> PathBuf {
    codex_home.join("auth.json")
}

/// `$CODEX_HOME`, else `~/.codex`.
pub fn default_codex_home() -> PathBuf {
    if let Some(h) = std::env::var_os("CODEX_HOME").filter(|h| !h.is_empty()) {
        return PathBuf::from(h);
    }
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".codex")
}

fn jwt_payload(token: &str) -> Option<Value> {
    let part = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(part.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The JWT's `exp` in epoch ms; 0 when unreadable.
pub fn jwt_expiry_ms(token: &str) -> i64 {
    jwt_payload(token).and_then(|p| p.get("exp").and_then(Value::as_f64)).map(|s| (s * 1000.0) as i64).unwrap_or(0)
}

fn jwt_account_id(token: &str) -> Option<String> {
    let p = jwt_payload(token)?;
    p.get(JWT_CLAIM_PATH)?.get("chatgpt_account_id")?.as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

fn read_file(codex_home: &Path) -> Option<Map<String, Value>> {
    let txt = std::fs::read_to_string(auth_file(codex_home)).ok()?;
    match serde_json::from_str::<Value>(&txt).ok()? {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

fn token<'a>(file: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    file.get("tokens")?.get(name)?.as_str().filter(|s| !s.is_empty())
}

/// Is a ChatGPT login present in the Codex CLI's auth file?
pub fn logged_in(codex_home: &Path) -> bool {
    read_file(codex_home).is_some_and(|f| token(&f, "access_token").is_some() && token(&f, "refresh_token").is_some())
}

/// The current credential, or None when not logged in. Errors when the account id can't be found.
pub fn read_credential(codex_home: &Path) -> Result<Option<Credential>, AiError> {
    let Some(file) = read_file(codex_home) else { return Ok(None) };
    let (Some(access), Some(refresh)) = (token(&file, "access_token"), token(&file, "refresh_token")) else { return Ok(None) };
    let account_id = token(&file, "account_id")
        .map(str::to_string)
        .or_else(|| jwt_account_id(access))
        .ok_or_else(|| AiError::Llm("Failed to extract accountId from the Codex login. Run `codex login` again.".into()))?;
    Ok(Some(Credential { access: access.into(), refresh: refresh.into(), account_id, expires_ms: jwt_expiry_ms(access) }))
}

/// Persist a refreshed credential in the CLI's format (other fields kept), atomically (tmp file 0600 + rename).
pub fn write_credential(codex_home: &Path, access: &str, refresh: &str, account_id: Option<&str>) -> std::io::Result<()> {
    let file = auth_file(codex_home);
    let mut current = read_file(codex_home).unwrap_or_default();
    let mut tokens = match current.remove("tokens") {
        Some(Value::Object(t)) => t,
        _ => Map::new(),
    };
    tokens.insert("access_token".into(), Value::String(access.into()));
    tokens.insert("refresh_token".into(), Value::String(refresh.into()));
    if let Some(id) = account_id {
        tokens.insert("account_id".into(), Value::String(id.into()));
    }
    current.insert("tokens".into(), Value::Object(tokens));
    current.insert("last_refresh".into(), Value::String(crate::iso_now()));
    let json = serde_json::to_string_pretty(&Value::Object(current)).map_err(std::io::Error::other)?;
    let tmp = file.with_file_name(format!("auth.json.{}.tmp", std::process::id()));
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(json.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &file)
}

/// The token endpoint's answer (only what is used).
#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<f64>,
}

/// Exchange the refresh token. Returns `(access, refresh)`. Error messages never contain tokens.
pub async fn refresh(http: &reqwest::Client, token_url: &str, refresh_token: &str) -> Result<(String, String), AiError> {
    let res = http
        .post(token_url)
        .form(&[("grant_type", "refresh_token"), ("refresh_token", refresh_token), ("client_id", CLIENT_ID)])
        .send()
        .await
        .map_err(|e| AiError::Llm(format!("OpenAI Codex token refresh error: {}", e.without_url())))?;
    let status = res.status();
    if !status.is_success() {
        return Err(AiError::Llm(format!(
            "OpenAI Codex token refresh failed ({}). Run `codex login` in a terminal, then retry.",
            status.as_u16()
        )));
    }
    let body: TokenResponse =
        res.json().await.map_err(|_| AiError::Llm("OpenAI Codex token refresh returned an unreadable response.".into()))?;
    match (body.access_token, body.refresh_token, body.expires_in) {
        (Some(a), Some(r), Some(_)) if !a.is_empty() && !r.is_empty() => Ok((a, r)),
        _ => Err(AiError::Llm("OpenAI Codex token refresh response missing fields.".into())),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// An unsigned JWT with the given payload (tests only).
    pub fn jwt(payload: Value) -> String {
        let enc = |v: &Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
        format!("{}.{}.sig", enc(&json!({"alg":"none"})), enc(&payload))
    }

    #[test]
    fn reads_credential_and_account_fallback() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!logged_in(dir.path()));
        assert!(read_credential(dir.path()).unwrap().is_none());
        let access = jwt(json!({"exp": 2_000_000_000, JWT_CLAIM_PATH: {"chatgpt_account_id": "acc-jwt"}}));
        std::fs::write(
            auth_file(dir.path()),
            json!({"auth_mode":"chatgpt","tokens":{"access_token": access, "refresh_token":"r1","id_token":"i"}}).to_string(),
        )
        .unwrap();
        assert!(logged_in(dir.path()));
        let c = read_credential(dir.path()).unwrap().unwrap();
        assert_eq!(c.account_id, "acc-jwt");
        assert_eq!(c.expires_ms, 2_000_000_000_000);
        assert!(!c.needs_refresh(1_700_000_000_000));
        assert!(c.needs_refresh(2_000_000_000_000 - 60_000));
    }

    #[test]
    fn write_back_keeps_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            auth_file(dir.path()),
            json!({"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"access_token":"a","refresh_token":"r","id_token":"i","account_id":"acc"}})
                .to_string(),
        )
        .unwrap();
        write_credential(dir.path(), "a2", "r2", None).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(auth_file(dir.path())).unwrap()).unwrap();
        assert_eq!(v["auth_mode"], "chatgpt");
        assert_eq!(v["tokens"], json!({"access_token":"a2","refresh_token":"r2","id_token":"i","account_id":"acc"}));
        assert!(v["last_refresh"].as_str().unwrap().ends_with('Z'));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(auth_file(dir.path())).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no tmp file left behind");
    }

    #[test]
    fn unreadable_token_means_refresh_now() {
        assert_eq!(jwt_expiry_ms("not-a-jwt"), 0);
        let c = Credential { access: "x".into(), refresh: "y".into(), account_id: "z".into(), expires_ms: 0 };
        assert!(c.needs_refresh(0));
    }
}
