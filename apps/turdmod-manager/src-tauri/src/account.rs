// Account / OAuth glue for the manager.
//
// We store the long-lived turdmod.com session token in the OS keychain
// (Windows Credential Manager / macOS Keychain / Secret Service) via
// the `keyring` crate. Falling back to plaintext disk storage is
// intentionally NOT implemented — if the keychain is unavailable we'd
// rather surface a clear error than silently downgrade.
//
// The current "sign in" UX is the simplest thing that works without a
// new web endpoint: open turdmod.com/account?desktop=1 in the system
// browser, the user grabs a one-time code, pastes it back here, and
// we exchange it for a token. Until the web side ships the
// `/api/auth/desktop-token` exchange endpoint we accept the pasted
// value as the token directly. Swapping to a real exchange call later
// is a single function-body change.
//
// State machine surfaced to JS is `unknown | signed-out | signed-in`.
// `unknown` covers the brief boot window before we've checked the
// keychain. The web app's NextAuth `/api/auth/session` endpoint
// returns either `{}` (no session) or a populated `{ user: {...} }`
// payload — we treat the absence of `user` as signed-out.

use std::time::Duration;

use keyring::Entry;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const KEYCHAIN_SERVICE: &str = "com.turdmod.manager";
const KEYCHAIN_ACCOUNT: &str = "session-token";

// TODO(web): once `/api/auth/desktop-token` ships on turdmod.com, also
// store the matching refresh token under a second keychain entry and
// swap `verify_token` to call the dedicated desktop endpoint with a
// Bearer header instead of leaning on the cookie-only session route.
const SESSION_URL: &str = "https://turdmod.com/api/auth/session";

// Default callback URL the manager hands to the web sign-in form.
// `desktop=1` is read by the web `/account` page (TODO on web side)
// to surface the one-time code. We point at /account rather than /
// because the user's next physical action after signing in is to read
// a code shown there.
//
// TODO(web): add the `?desktop=1` branch to `/account` that shows a
// short-lived code + "Copy" button.
const SIGN_IN_URL: &str = "https://turdmod.com/account?desktop=1";

#[derive(Debug, Error)]
pub enum AccountError {
    #[error("keychain: {0}")]
    Keychain(#[from] keyring::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("token rejected by server")]
    TokenRejected,
    #[error("empty token")]
    EmptyToken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUser {
    pub id: Option<i64>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub discord_id: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum AccountStatus {
    SignedOut,
    SignedIn { user: AccountUser },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInStartInfo {
    /// URL the front-end should hand to `@tauri-apps/plugin-shell`'s
    /// `open()` so the OS launches it in the user's default browser.
    pub url: String,
}

fn entry() -> Result<Entry, AccountError> {
    Ok(Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)?)
}

pub fn load_token() -> Result<Option<String>, AccountError> {
    match entry()?.get_password() {
        Ok(s) if s.trim().is_empty() => Ok(None),
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AccountError::Keychain(e)),
    }
}

pub fn save_token(token: &str) -> Result<(), AccountError> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(AccountError::EmptyToken);
    }
    entry()?.set_password(trimmed)?;
    Ok(())
}

pub fn clear_token() -> Result<(), AccountError> {
    match entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AccountError::Keychain(e)),
    }
}

pub fn sign_in_url() -> SignInStartInfo {
    SignInStartInfo {
        url: SIGN_IN_URL.to_string(),
    }
}

/// Hit turdmod.com and confirm the stored token still resolves a user.
///
/// TODO(web): the production NextAuth `/api/auth/session` endpoint
/// only honours the session cookie today. Sending `Authorization:
/// Bearer <token>` is a no-op against it — this call is structured so
/// the moment a desktop-token-aware endpoint exists, we just point
/// `SESSION_URL` at it and the rest of the pipeline continues to work.
pub async fn verify_token(token: &str) -> Result<AccountUser, AccountError> {
    if token.trim().is_empty() {
        return Err(AccountError::EmptyToken);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let resp = client
        .get(SESSION_URL)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json")
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(AccountError::TokenRejected);
    }

    // NextAuth shape: `{ user: { id, name, email, image, username,
    // discordId }, expires }` for an authenticated request, or `{}`
    // when there is no session.
    let body: serde_json::Value = resp.json().await?;
    let user_obj = match body.get("user") {
        Some(v) if v.is_object() => v,
        _ => return Err(AccountError::TokenRejected),
    };

    let s = |k: &str| {
        user_obj
            .get(k)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };
    let i = |k: &str| user_obj.get(k).and_then(|v| v.as_i64());

    Ok(AccountUser {
        id: i("id"),
        username: s("username").or_else(|| s("name")),
        display_name: s("name"),
        avatar_url: s("image").or_else(|| s("avatarUrl")),
        discord_id: s("discordId"),
        email: s("email"),
    })
}

pub async fn current_status() -> Result<AccountStatus, AccountError> {
    let Some(token) = load_token()? else {
        return Ok(AccountStatus::SignedOut);
    };
    match verify_token(&token).await {
        Ok(user) => Ok(AccountStatus::SignedIn { user }),
        Err(AccountError::TokenRejected) => {
            // Token is dead — wipe so we don't keep retrying it on
            // every status poll.
            let _ = clear_token();
            Ok(AccountStatus::SignedOut)
        }
        Err(e) => Err(e),
    }
}
