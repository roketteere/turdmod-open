// Tauri command surface for the Account screen.
//
// Commands are exposed to JS as camelCase via `rename_all`. The
// front-end calls them through `src/lib/tauri-account.ts`.

use crate::account::{
    self, AccountError, AccountStatus, SignInStartInfo,
};

fn err(e: AccountError) -> String {
    e.to_string()
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_account_status() -> Result<AccountStatus, String> {
    account::current_status().await.map_err(err)
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_account_sign_in_start() -> Result<SignInStartInfo, String> {
    Ok(account::sign_in_url())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_account_paste_token(token: String) -> Result<AccountStatus, String> {
    // TODO(web): when `/api/desktop/exchange` ships, treat the pasted
    // string as a one-time code and POST it here to receive the real
    // long-lived token. For now we accept the token directly.
    let trimmed = token.trim().to_string();
    if trimmed.is_empty() {
        return Err("empty_token".into());
    }
    let user = account::verify_token(&trimmed).await.map_err(err)?;
    account::save_token(&trimmed).map_err(err)?;
    Ok(AccountStatus::SignedIn { user })
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_account_sign_out() -> Result<(), String> {
    account::clear_token().map_err(err)
}
