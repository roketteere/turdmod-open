// TurdMOD Lite — open-source SCUM admin client for managed hosts.
//
// This crate is the Rust backend for the Tauri app. The frontend lives in
// ../src; the two communicate via Tauri's IPC bridge.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures_util::io::AsyncReadExt as FuturesReadExt;
use serde::Serialize;
use suppaftp::AsyncFtpStream;
use tauri::Manager;
use tokio::time::timeout;

const FTP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SmokeTestResult {
    ok: bool,
    app_version: &'static str,
}

#[tauri::command]
fn lite_smoke_test() -> SmokeTestResult {
    SmokeTestResult {
        ok: true,
        app_version: env!("CARGO_PKG_VERSION"),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FtpTestResult {
    ok: bool,
    welcome: String,
    listing_count: usize,
}

// Opens an FTP connection, logs in, and lists the root. Used by the
// "Connect to server" form to validate creds before reading files.
//
// G-Portal exposes plain FTP. SFTP support is TBD when another host
// that requires it shows up in testing.
#[tauri::command]
async fn lite_ftp_test(
    host: String,
    port: u16,
    username: String,
    password: String,
) -> Result<FtpTestResult, String> {
    ftp_test_inner(&host, port, &username, &password)
        .await
        .map_err(|e| format!("{:#}", e))
}

async fn ftp_test_inner(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
) -> Result<FtpTestResult> {
    let mut stream = ftp_connect(host, port, username, password).await?;
    let welcome = stream.get_welcome_msg().unwrap_or_default().to_string();
    let listing = stream
        .nlst(None)
        .await
        .context("ftp NLST root")
        .unwrap_or_default();
    let _ = stream.quit().await;
    Ok(FtpTestResult {
        ok: true,
        welcome,
        listing_count: listing.len(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FtpReadResult {
    content: String,
    bytes: usize,
}

// Reads a text file from the FTP server. Used by the Servers page to
// pull Notifications.json / ServerSettings.ini / etc. Returns the content
// as UTF-8 (lossy on invalid bytes).
#[tauri::command]
async fn lite_ftp_read_text(
    host: String,
    port: u16,
    username: String,
    password: String,
    remote_path: String,
) -> Result<FtpReadResult, String> {
    ftp_read_inner(&host, port, &username, &password, &remote_path)
        .await
        .map_err(|e| format!("{:#}", e))
}

async fn ftp_read_inner(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    remote_path: &str,
) -> Result<FtpReadResult> {
    let mut stream = ftp_connect(host, port, username, password).await?;
    let mut reader = stream
        .retr_as_stream(remote_path)
        .await
        .with_context(|| format!("ftp RETR {}", remote_path))?;
    let mut buf = Vec::new();
    FuturesReadExt::read_to_end(&mut reader, &mut buf)
        .await
        .context("ftp read stream")?;
    stream
        .finalize_retr_stream(reader)
        .await
        .context("ftp finalize")?;
    let _ = stream.quit().await;
    let content = String::from_utf8_lossy(&buf).into_owned();
    let bytes = buf.len();
    Ok(FtpReadResult { content, bytes })
}

async fn ftp_connect(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
) -> Result<AsyncFtpStream> {
    let addr = format!("{}:{}", host, port);
    let mut stream = timeout(FTP_TIMEOUT, AsyncFtpStream::connect(&addr))
        .await
        .map_err(|_| anyhow!("ftp connect timeout after {:?}", FTP_TIMEOUT))?
        .context("ftp connect")?;
    stream
        .login(username, password)
        .await
        .context("ftp login")?;
    Ok(stream)
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            lite_smoke_test,
            lite_ftp_test,
            lite_ftp_read_text,
        ])
        .setup(|app| {
            tracing::info!(
                "TurdMOD Lite booting — version {}",
                env!("CARGO_PKG_VERSION")
            );
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.open_devtools();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
