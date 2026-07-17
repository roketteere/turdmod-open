// RCON client — Source RCON (Valve's standard, TCP).
//
// ⚠️ WRONG PROTOCOL FOR DIRECT SCUM (corrected 2026-05-30). SCUM uses **BattlEye
// RCON (BERcon, UDP)**, NOT Source RCON — this client never binds a port against a
// real SCUM dedicated server. It may only work through a managed-host RCON proxy
// (e.g. G-Portal). The working client is `scripts/rcon_be.py` (BERcon).
// Also note: RCON of either kind only runs BattlEye server-mgmt commands
// (players/kick/ban/say) — NOT SCUM '#' game-admin commands. See HANDBOOK.md §2.5.
// Kept as-is for the proxy case; do not assume it drives a bare SCUM server.

use anyhow::{bail, Context, Result};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const RCON_TIMEOUT: Duration = Duration::from_secs(10);

// Source RCON packet types
const SERVERDATA_AUTH: i32 = 3;
const SERVERDATA_EXECCOMMAND: i32 = 2;
const SERVERDATA_AUTH_RESPONSE: i32 = 2;
const SERVERDATA_RESPONSE_VALUE: i32 = 0;

struct RconPacket {
    id: i32,
    ptype: i32,
    body: String,
}

impl RconPacket {
    fn encode(&self) -> Vec<u8> {
        let body_bytes = self.body.as_bytes();
        let size = 4 + 4 + body_bytes.len() + 2; // id + type + body + 2 null terminators
        let mut buf = Vec::with_capacity(4 + size);
        buf.extend_from_slice(&(size as i32).to_le_bytes());
        buf.extend_from_slice(&self.id.to_le_bytes());
        buf.extend_from_slice(&self.ptype.to_le_bytes());
        buf.extend_from_slice(body_bytes);
        buf.push(0); // body null terminator
        buf.push(0); // packet null terminator
        buf
    }

    async fn read_from(stream: &mut TcpStream) -> Result<Self> {
        let mut size_buf = [0u8; 4];
        stream.read_exact(&mut size_buf).await.context("read size")?;
        let size = i32::from_le_bytes(size_buf) as usize;
        if size > 4096 { bail!("packet too large: {}", size); }

        let mut data = vec![0u8; size];
        stream.read_exact(&mut data).await.context("read body")?;

        let id = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let ptype = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let body_end = data[8..].iter().position(|&b| b == 0).unwrap_or(data.len() - 8);
        let body = String::from_utf8_lossy(&data[8..8 + body_end]).to_string();

        Ok(Self { id, ptype, body })
    }
}

pub async fn exec(host: &str, port: u16, password: &str, command: &str) -> Result<String> {
    let addr = format!("{}:{}", host, port);

    let mut stream = timeout(RCON_TIMEOUT, TcpStream::connect(&addr))
        .await
        .context("connect timeout")?
        .context("connect failed")?;

    // Authenticate
    let auth = RconPacket { id: 1, ptype: SERVERDATA_AUTH, body: password.into() };
    stream.write_all(&auth.encode()).await.context("send auth")?;

    let resp = timeout(RCON_TIMEOUT, RconPacket::read_from(&mut stream))
        .await
        .context("auth response timeout")?
        .context("read auth response")?;

    if resp.id == -1 {
        bail!("RCON auth failed — bad password");
    }

    // Sometimes SCUM sends an empty response before the auth response
    if resp.ptype != SERVERDATA_AUTH_RESPONSE {
        let resp2 = timeout(RCON_TIMEOUT, RconPacket::read_from(&mut stream))
            .await
            .context("auth response2 timeout")?
            .context("read auth response2")?;
        if resp2.id == -1 { bail!("RCON auth failed"); }
    }

    // Send command
    let cmd = RconPacket { id: 2, ptype: SERVERDATA_EXECCOMMAND, body: command.into() };
    stream.write_all(&cmd.encode()).await.context("send command")?;

    let result = timeout(RCON_TIMEOUT, RconPacket::read_from(&mut stream))
        .await
        .context("command response timeout")?
        .context("read command response")?;

    Ok(result.body)
}

/// Read RCON config from service config or fallback
pub fn get_rcon_config() -> Option<(String, u16, String)> {
    // Read from C:\TurdMOD\data\rcon.json or fallback to localhost:7048
    let path = r"C:\TurdMOD\data\rcon.json";
    if let Ok(raw) = std::fs::read_to_string(path).map(|s| s.trim_start_matches('\u{feff}').trim().to_string()) {
        tracing::debug!("rcon: loaded config ({} bytes)", raw.len());
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
            let host = val.get("host").and_then(|v| v.as_str()).unwrap_or("127.0.0.1").to_string();
            let port = val.get("port").and_then(|v| v.as_u64()).unwrap_or(7048) as u16;
            let pass = val.get("password").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if !pass.is_empty() { return Some((host, port, pass)); }
        }
    }
    None
}
