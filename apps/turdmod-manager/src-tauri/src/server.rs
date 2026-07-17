// Remote SCUM dedicated-server connector.
//
// Three transports:
//   - SFTP    — full file-ops (push mods, list, delete, tail logs) plus
//               optional RCON socket for chat/admin commands.
//   - FTP     — file-ops only via plain FTP (suppaftp, rustls); RCON
//               available only if the profile carries an rcon_port.
//   - RconOnly — chat/admin commands only, no file management.
//
// Secrets (SSH/FTP password, RCON password) NEVER live on the
// `ServerProfile` struct — they're stashed in tauri-plugin-store under
// fixed key patterns so the profile JSON can be passed around freely.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use futures_util::io::{AsyncReadExt as FuturesReadExt, Cursor as FuturesCursor};
use russh::client::{self, Handle, Handler};
use russh::keys::key;
use russh::Disconnect;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use suppaftp::AsyncFtpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

pub const SECRET_SSH_PREFIX: &str = "server-creds-ssh-";
pub const SECRET_RCON_PREFIX: &str = "server-creds-rcon-";
pub const STORE_PROFILES_KEY: &str = "server-profiles";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const RCON_TIMEOUT: Duration = Duration::from_secs(5);
const REMOTE_MODS_SUBPATH: &str = "SCUM/Saved/Config/turdmod-mods";
const REMOTE_LOGS_SUBPATH: &str = "SCUM/Saved/SaveFiles/Logs";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Transport {
    Sftp,
    Ftp,
    RconOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub transport: Transport,
    pub username: String,
    pub scum_root: String,
    #[serde(default)]
    pub rcon_port: Option<u16>,
    #[serde(default)]
    pub rcon_password_ref: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteMod {
    pub slug: String,
    pub size_bytes: u64,
    pub modified: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePath {
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProbeResult {
    pub sftp_ok: bool,
    pub rcon_ok: bool,
    pub latency_ms: Option<u32>,
    pub error: Option<String>,
}

// Public free functions -----------------------------------------------------

pub async fn upload_mod(
    profile: &ServerProfile,
    secret: Option<&str>,
    mod_slug: &str,
    mod_zip_bytes: &[u8],
) -> Result<RemotePath> {
    let remote_dir = join_remote(&profile.scum_root, REMOTE_MODS_SUBPATH);
    let filename = format!("{}.turdmod", mod_slug);
    let remote_path = join_remote(&remote_dir, &filename);

    match profile.transport {
        Transport::Sftp => {
            let sftp = SftpClient::connect(profile, secret).await?;
            sftp.ensure_dir(&remote_dir).await?;
            sftp.write_file(&remote_path, mod_zip_bytes).await?;
            sftp.close().await;
        }
        Transport::Ftp => {
            let mut ftp = ftp_connect(profile, secret).await?;
            ftp_ensure_dir(&mut ftp, &remote_dir).await?;
            ftp.cwd(&remote_dir).await.context("ftp cwd mods dir")?;
            let mut reader = FuturesCursor::new(mod_zip_bytes.to_vec());
            ftp.put_file(&filename, &mut reader)
                .await
                .context("ftp put mod artifact")?;
            ftp.quit().await.ok();
        }
        Transport::RconOnly => {
            return Err(anyhow!(
                "Transport=RconOnly does not support file uploads"
            ));
        }
    }

    Ok(RemotePath {
        path: remote_path,
        size_bytes: mod_zip_bytes.len() as u64,
    })
}

pub async fn list_remote_mods(
    profile: &ServerProfile,
    secret: Option<&str>,
) -> Result<Vec<RemoteMod>> {
    let remote_dir = join_remote(&profile.scum_root, REMOTE_MODS_SUBPATH);

    match profile.transport {
        Transport::Sftp => {
            let sftp = SftpClient::connect(profile, secret).await?;
            let entries = sftp.list_dir(&remote_dir).await.unwrap_or_default();
            sftp.close().await;
            Ok(entries
                .into_iter()
                .filter(|e| e.name.ends_with(".turdmod"))
                .map(|e| RemoteMod {
                    slug: strip_turdmod_ext(&e.name),
                    size_bytes: e.size_bytes,
                    modified: e.modified,
                })
                .collect())
        }
        Transport::Ftp => {
            let mut ftp = ftp_connect(profile, secret).await?;
            if ftp.cwd(&remote_dir).await.is_err() {
                ftp.quit().await.ok();
                return Ok(Vec::new());
            }
            let listings = ftp.list(None).await.context("ftp list mods dir")?;
            ftp.quit().await.ok();
            Ok(parse_ftp_listings(&listings))
        }
        Transport::RconOnly => Err(anyhow!("Transport=RconOnly cannot list remote mods")),
    }
}

pub async fn delete_remote_mod(
    profile: &ServerProfile,
    secret: Option<&str>,
    slug: &str,
) -> Result<()> {
    let remote_dir = join_remote(&profile.scum_root, REMOTE_MODS_SUBPATH);
    let filename = format!("{}.turdmod", slug);
    let remote_path = join_remote(&remote_dir, &filename);

    match profile.transport {
        Transport::Sftp => {
            let sftp = SftpClient::connect(profile, secret).await?;
            sftp.delete_file(&remote_path).await?;
            sftp.close().await;
            Ok(())
        }
        Transport::Ftp => {
            let mut ftp = ftp_connect(profile, secret).await?;
            ftp.cwd(&remote_dir).await.context("ftp cwd mods dir")?;
            ftp.rm(&filename).await.context("ftp rm")?;
            ftp.quit().await.ok();
            Ok(())
        }
        Transport::RconOnly => Err(anyhow!("Transport=RconOnly cannot delete remote mods")),
    }
}

pub async fn tail_log(
    profile: &ServerProfile,
    secret: Option<&str>,
    log_name: &str,
    lines: usize,
) -> Result<Vec<String>> {
    let safe = sanitize_log_name(log_name);
    let remote_path = join_remote(
        &join_remote(&profile.scum_root, REMOTE_LOGS_SUBPATH),
        &format!("{}.log", safe),
    );

    let raw = match profile.transport {
        Transport::Sftp => {
            let sftp = SftpClient::connect(profile, secret).await?;
            let bytes = sftp.read_file(&remote_path).await?;
            sftp.close().await;
            bytes
        }
        Transport::Ftp => {
            let mut ftp = ftp_connect(profile, secret).await?;
            let mut stream = ftp
                .retr_as_stream(&remote_path)
                .await
                .context("ftp retr")?;
            let mut bytes = Vec::new();
            FuturesReadExt::read_to_end(&mut stream, &mut bytes)
                .await
                .context("ftp read body")?;
            ftp.finalize_retr_stream(stream)
                .await
                .context("ftp finalize retr")?;
            ftp.quit().await.ok();
            bytes
        }
        Transport::RconOnly => return Err(anyhow!("Transport=RconOnly cannot tail logs")),
    };

    let text = decode_utf16le(&raw);
    let all: Vec<&str> = text.lines().collect();
    let take = all.len().min(lines);
    Ok(all[all.len() - take..]
        .iter()
        .map(|s| s.to_string())
        .collect())
}

pub async fn rcon_send(
    profile: &ServerProfile,
    rcon_password: &str,
    command: &str,
) -> Result<String> {
    let port = profile
        .rcon_port
        .ok_or_else(|| anyhow!("profile has no rcon_port set"))?;
    let addr: SocketAddr = format!("{}:{}", profile.host, port)
        .parse()
        .with_context(|| format!("invalid rcon address {}:{}", profile.host, port))?;

    let mut stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .context("rcon connect timeout")??;

    write_rcon_packet(&mut stream, 1, RCON_AUTH, rcon_password.as_bytes()).await?;
    let auth_resp = timeout(RCON_TIMEOUT, read_rcon_packet(&mut stream)).await??;
    if auth_resp.id == -1 {
        return Err(anyhow!("rcon auth failed"));
    }

    write_rcon_packet(&mut stream, 2, RCON_EXEC, command.as_bytes()).await?;
    let exec_resp = timeout(RCON_TIMEOUT, read_rcon_packet(&mut stream)).await??;
    Ok(String::from_utf8_lossy(&exec_resp.body).to_string())
}

pub async fn probe(
    profile: &ServerProfile,
    secret_ssh: Option<&str>,
    secret_rcon: Option<&str>,
) -> ServerProbeResult {
    let started = Instant::now();
    let mut result = ServerProbeResult {
        sftp_ok: false,
        rcon_ok: false,
        latency_ms: None,
        error: None,
    };

    match profile.transport {
        Transport::Sftp => match SftpClient::connect(profile, secret_ssh).await {
            Ok(client) => {
                result.sftp_ok = true;
                client.close().await;
            }
            Err(err) => {
                result.error = Some(format!("sftp: {}", err));
            }
        },
        Transport::Ftp => match ftp_connect(profile, secret_ssh).await {
            Ok(mut s) => {
                result.sftp_ok = true;
                s.quit().await.ok();
            }
            Err(err) => {
                result.error = Some(format!("ftp: {}", err));
            }
        },
        Transport::RconOnly => {}
    }

    if profile.rcon_port.is_some() {
        if let Some(password) = secret_rcon {
            match rcon_send(profile, password, "Echo TurdMODProbe").await {
                Ok(_) => result.rcon_ok = true,
                Err(err) => {
                    let prev = result.error.take();
                    let combined = match prev {
                        Some(p) => format!("{} | rcon: {}", p, err),
                        None => format!("rcon: {}", err),
                    };
                    result.error = Some(combined);
                }
            }
        }
    }

    result.latency_ms = Some(started.elapsed().as_millis().min(u32::MAX as u128) as u32);
    result
}

// SFTP client ---------------------------------------------------------------

struct SftpClient {
    handle: Handle<NoopSshHandler>,
    sftp: SftpSession,
}

impl SftpClient {
    async fn connect(profile: &ServerProfile, secret: Option<&str>) -> Result<Self> {
        let password = secret.ok_or_else(|| anyhow!("missing SSH password"))?;
        let config = Arc::new(client::Config::default());
        let addr = (profile.host.as_str(), profile.port);

        let mut handle = timeout(
            CONNECT_TIMEOUT,
            client::connect(config, addr, NoopSshHandler),
        )
        .await
        .context("ssh connect timeout")??;

        let authed = handle
            .authenticate_password(&profile.username, password)
            .await
            .context("ssh auth")?;
        if !authed {
            return Err(anyhow!("ssh password auth rejected"));
        }

        let channel = handle.channel_open_session().await.context("open session")?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .context("request sftp subsystem")?;
        let sftp = SftpSession::new(channel.into_stream())
            .await
            .context("init sftp session")?;

        Ok(Self { handle, sftp })
    }

    async fn ensure_dir(&self, path: &str) -> Result<()> {
        if self.sftp.metadata(path).await.is_ok() {
            return Ok(());
        }
        let mut accum = String::new();
        for part in path.split('/') {
            if part.is_empty() {
                accum.push('/');
                continue;
            }
            if !accum.ends_with('/') && !accum.is_empty() {
                accum.push('/');
            }
            accum.push_str(part);
            if self.sftp.metadata(&accum).await.is_err() {
                self.sftp
                    .create_dir(&accum)
                    .await
                    .with_context(|| format!("sftp mkdir {}", accum))?;
            }
        }
        Ok(())
    }

    async fn write_file(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let mut file = self
            .sftp
            .open_with_flags(
                path,
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
            )
            .await
            .with_context(|| format!("sftp open-write {}", path))?;
        file.write_all(bytes).await.context("sftp write")?;
        file.shutdown().await.ok();
        Ok(())
    }

    async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let mut file = self
            .sftp
            .open(path)
            .await
            .with_context(|| format!("sftp open-read {}", path))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).await.context("sftp read")?;
        Ok(buf)
    }

    async fn delete_file(&self, path: &str) -> Result<()> {
        self.sftp
            .remove_file(path)
            .await
            .with_context(|| format!("sftp remove {}", path))?;
        Ok(())
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>> {
        let entries = self
            .sftp
            .read_dir(path)
            .await
            .with_context(|| format!("sftp readdir {}", path))?;
        Ok(entries
            .map(|e| {
                let name = e.file_name();
                let meta = e.metadata();
                DirEntry {
                    name,
                    size_bytes: meta.size.unwrap_or(0),
                    modified: meta.mtime.map(|t| t as i64),
                }
            })
            .collect())
    }

    async fn close(self) {
        let SftpClient { handle, sftp } = self;
        let _ = sftp.close().await;
        let _ = handle
            .disconnect(Disconnect::ByApplication, "bye", "en")
            .await;
    }
}

struct DirEntry {
    name: String,
    size_bytes: u64,
    modified: Option<i64>,
}

#[derive(Clone)]
struct NoopSshHandler;

#[async_trait]
impl Handler for NoopSshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &key::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        Ok(true)
    }
}

// FTP -----------------------------------------------------------------------

async fn ftp_connect(
    profile: &ServerProfile,
    secret: Option<&str>,
) -> Result<AsyncFtpStream> {
    let password = secret.ok_or_else(|| anyhow!("missing FTP password"))?;
    let addr = format!("{}:{}", profile.host, profile.port);
    let mut stream = timeout(CONNECT_TIMEOUT, AsyncFtpStream::connect(&addr))
        .await
        .context("ftp connect timeout")??;
    stream
        .login(profile.username.as_str(), password)
        .await
        .context("ftp login")?;
    Ok(stream)
}

async fn ftp_ensure_dir(stream: &mut AsyncFtpStream, path: &str) -> Result<()> {
    if stream.cwd(path).await.is_ok() {
        return Ok(());
    }
    let mut accum = String::new();
    for part in path.split('/') {
        if part.is_empty() {
            accum.push('/');
            continue;
        }
        if !accum.ends_with('/') && !accum.is_empty() {
            accum.push('/');
        }
        accum.push_str(part);
        let _ = stream.mkdir(&accum).await;
    }
    Ok(())
}

fn parse_ftp_listings(lines: &[String]) -> Vec<RemoteMod> {
    let mut out = Vec::new();
    for line in lines {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.is_empty() {
            continue;
        }
        let name = cols.last().copied().unwrap_or("");
        if !name.ends_with(".turdmod") {
            continue;
        }
        let size_bytes = if cols.len() >= 5 {
            cols[4].parse::<u64>().unwrap_or(0)
        } else {
            0
        };
        out.push(RemoteMod {
            slug: strip_turdmod_ext(name),
            size_bytes,
            modified: None,
        });
    }
    out
}

// RCON (Source protocol) ----------------------------------------------------

const RCON_AUTH: i32 = 3;
const RCON_EXEC: i32 = 2;

struct RconPacket {
    id: i32,
    body: Vec<u8>,
}

async fn write_rcon_packet(
    stream: &mut TcpStream,
    id: i32,
    kind: i32,
    body: &[u8],
) -> Result<()> {
    let body_len = body.len();
    let size: i32 = (4 + 4 + body_len + 2) as i32;
    let mut buf = Vec::with_capacity(size as usize + 4);
    buf.extend_from_slice(&size.to_le_bytes());
    buf.extend_from_slice(&id.to_le_bytes());
    buf.extend_from_slice(&kind.to_le_bytes());
    buf.extend_from_slice(body);
    buf.push(0);
    buf.push(0);
    stream.write_all(&buf).await.context("rcon write")?;
    Ok(())
}

async fn read_rcon_packet(stream: &mut TcpStream) -> Result<RconPacket> {
    let mut size_buf = [0u8; 4];
    stream
        .read_exact(&mut size_buf)
        .await
        .context("rcon read size")?;
    let size = i32::from_le_bytes(size_buf);
    if !(10..=4096).contains(&size) {
        return Err(anyhow!("rcon packet size out of range: {}", size));
    }
    let mut rest = vec![0u8; size as usize];
    stream
        .read_exact(&mut rest)
        .await
        .context("rcon read body")?;
    let id = i32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]);
    let body_end = rest.len().saturating_sub(2);
    let body = rest[8..body_end].to_vec();
    Ok(RconPacket { id, body })
}

// Helpers -------------------------------------------------------------------

fn join_remote(base: &str, sub: &str) -> String {
    let base_trim = base.trim_end_matches('/');
    let sub_trim = sub.trim_start_matches('/');
    if base_trim.is_empty() {
        format!("/{}", sub_trim)
    } else {
        format!("{}/{}", base_trim, sub_trim)
    }
}

fn strip_turdmod_ext(name: &str) -> String {
    name.strip_suffix(".turdmod")
        .unwrap_or(name)
        .to_string()
}

// SCUM dedicated-server logs are UTF-16 LE without a BOM. We decode by
// pairing every two bytes; if the byte length is odd we drop the trailing
// half-pair. Replacement chars stand in for invalid surrogate pairs so a
// single corrupted run doesn't lose the whole file.
fn decode_utf16le(bytes: &[u8]) -> String {
    let pair_count = bytes.len() / 2;
    let mut units: Vec<u16> = Vec::with_capacity(pair_count);
    for i in 0..pair_count {
        let lo = bytes[i * 2];
        let hi = bytes[i * 2 + 1];
        units.push(u16::from_le_bytes([lo, hi]));
    }
    let mut s = String::with_capacity(units.len());
    for r in std::char::decode_utf16(units) {
        s.push(r.unwrap_or(std::char::REPLACEMENT_CHARACTER));
    }
    s.replace("\r\n", "\n")
}

fn sanitize_log_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect()
}

pub fn store_path(app_data_dir: &std::path::Path) -> PathBuf {
    app_data_dir.join("server-profiles.json")
}

// Generic file I/O — shared by upload_mod, tail_log, and admin_files.
// Returns raw bytes; the caller decodes as appropriate (UTF-8 for INI,
// UTF-16 LE for SCUM logs).
pub async fn download_file(
    profile: &ServerProfile,
    secret: Option<&str>,
    remote_path: &str,
) -> Result<Vec<u8>> {
    match profile.transport {
        Transport::Sftp => {
            let sftp = SftpClient::connect(profile, secret).await?;
            let data = sftp.read_file(remote_path).await?;
            sftp.close().await;
            Ok(data)
        }
        Transport::Ftp => {
            let mut ftp = ftp_connect(profile, secret).await?;
            let mut stream = ftp
                .retr_as_stream(remote_path)
                .await
                .with_context(|| format!("ftp retr {}", remote_path))?;
            let mut buf = Vec::new();
            FuturesReadExt::read_to_end(&mut stream, &mut buf)
                .await
                .context("ftp read body")?;
            ftp.finalize_retr_stream(stream)
                .await
                .context("ftp finalize")?;
            ftp.quit().await.ok();
            Ok(buf)
        }
        Transport::RconOnly => Err(anyhow!("Transport=RconOnly cannot read files")),
    }
}

pub async fn upload_file(
    profile: &ServerProfile,
    secret: Option<&str>,
    remote_path: &str,
    bytes: &[u8],
) -> Result<()> {
    match profile.transport {
        Transport::Sftp => {
            let sftp = SftpClient::connect(profile, secret).await?;
            sftp.write_file(remote_path, bytes).await?;
            sftp.close().await;
            Ok(())
        }
        Transport::Ftp => {
            let mut ftp = ftp_connect(profile, secret).await?;
            let (dir, filename) = split_remote_path(remote_path);
            ftp.cwd(dir)
                .await
                .with_context(|| format!("ftp cwd {}", dir))?;
            let mut reader = FuturesCursor::new(bytes.to_vec());
            ftp.put_file(filename, &mut reader)
                .await
                .with_context(|| format!("ftp put {}", filename))?;
            ftp.quit().await.ok();
            Ok(())
        }
        Transport::RconOnly => Err(anyhow!("Transport=RconOnly cannot write files")),
    }
}

fn split_remote_path(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(idx) => (&path[..idx], &path[idx + 1..]),
        None => (".", path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_remote_handles_trailing_slashes() {
        assert_eq!(
            join_remote("/srv/scum/", "/SCUM/Saved/"),
            "/srv/scum/SCUM/Saved/"
        );
        assert_eq!(join_remote("/srv/scum", "SCUM"), "/srv/scum/SCUM");
    }

    #[test]
    fn decode_utf16le_pairs_bytes() {
        let bytes = [b'H', 0, b'i', 0, b'\r', 0, b'\n', 0];
        assert_eq!(decode_utf16le(&bytes), "Hi\n");
    }

    #[test]
    fn strip_extension() {
        assert_eq!(strip_turdmod_ext("welcome-screen.turdmod"), "welcome-screen");
        assert_eq!(strip_turdmod_ext("plain"), "plain");
    }

    #[test]
    fn sanitize_log_blocks_traversal() {
        assert_eq!(sanitize_log_name("../../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize_log_name("gameplay"), "gameplay");
    }
}
