//! BattlEye RCON (BERcon) client — the REAL admin channel for a SCUM server.
//!
//! Rust port of `scripts/rcon_be.py` (proven manually 2026-05-30). SCUM speaks
//! BattlEye RCON (BERcon, **UDP**) — NOT Valve Source RCON (that's `rcon.rs`,
//! left intact for the managed-host proxy path). BERcon only runs BattlEye
//! server-mgmt verbs (players/kick/ban/say) — NOT SCUM '#' game-admin commands
//! (HANDBOOK §2.5). The router's Rcon bridge calls `exec_cfg`.
//!
//! Packet:  'B''E' | CRC32(payload) LE 4B | payload
//!   payload = 0xFF <type> <data>
//!     0x00 login   : data = password         | reply 0xFF 0x00 <0x01 ok|0x00 fail>
//!     0x01 command : data = <seq:1B> <ascii>  | reply 0xFF 0x01 <seq> [0x00 <n> <i>]? <ascii>
//!     0x02 server  : keepalive/multipart — ack with 0xFF 0x02 <seq>
//!
//! @inv: zlib.crc32 == CRC-32/ISO-HDLC (poly 0xEDB88320, init/xorout 0xFFFFFFFF).
//! @dep: router.rs Rcon arm (route_rcon_*); config C:\TurdMOD\data\rcon.json.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use tokio::net::UdpSocket;
use tokio::time::timeout;

const CFG_PATH: &str = r"C:\TurdMOD\data\rcon.json";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(6);
const DRAIN_TIMEOUT: Duration = Duration::from_millis(1500); // gap that means "no more packets"

fn default_host() -> String { "127.0.0.1".into() }
fn default_port() -> u16 { 30016 } // BEServer_x64.cfg RConPort, set this session

#[derive(Deserialize)]
pub struct RconCfg {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default, alias = "pass", alias = "pw", alias = "RConPassword")]
    pub password: String,
}

pub fn load_cfg() -> Result<RconCfg> {
    let s = std::fs::read_to_string(CFG_PATH).with_context(|| format!("read {CFG_PATH}"))?;
    let cfg: RconCfg = serde_json::from_str(&s).context("parse rcon.json")?;
    if cfg.password.is_empty() {
        bail!("rcon.json has no password (key 'pass'/'password')");
    }
    Ok(cfg)
}

/// Load config and run one BERcon command. The router's only entry point.
pub async fn exec_cfg(command: &str) -> Result<String> {
    let cfg = load_cfg()?;
    exec(&cfg.host, cfg.port, &cfg.password, command).await
}

/// IEEE CRC-32 (reflected) — byte-for-byte match with Python's zlib.crc32.
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn frame(ptype: u8, data: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(2 + data.len());
    payload.push(0xFF);
    payload.push(ptype);
    payload.extend_from_slice(data);
    let mut pkt = Vec::with_capacity(6 + payload.len());
    pkt.extend_from_slice(b"BE");
    pkt.extend_from_slice(&crc32(&payload).to_le_bytes());
    pkt.extend_from_slice(&payload);
    pkt
}

/// Strip 'BE' + 4-byte CRC, return the `0xFF <type> <data>` payload.
fn payload(pkt: &[u8]) -> Result<&[u8]> {
    if pkt.len() < 7 || &pkt[..2] != b"BE" {
        bail!("bad BERcon header: {:?}", &pkt[..pkt.len().min(8)]);
    }
    Ok(&pkt[6..])
}

/// Login, run `command`, drain (possibly multipart) reply, ack keepalives. Returns the
/// concatenated ascii response (trimmed). Bridges nothing stateful — one-shot socket.
pub async fn exec(host: &str, port: u16, password: &str, command: &str) -> Result<String> {
    let sock = UdpSocket::bind("0.0.0.0:0").await.context("bind udp")?;
    sock.connect((host, port)).await.with_context(|| format!("connect {host}:{port}"))?;

    // login
    sock.send(&frame(0x00, password.as_bytes())).await?;
    let mut buf = [0u8; 4096];
    let n = timeout(LOGIN_TIMEOUT, sock.recv(&mut buf))
        .await
        .context("BERcon login timed out (wrong UDP port, or RCON disabled in BEServer_x64.cfg?)")??;
    let p = payload(&buf[..n])?;
    if p.len() < 3 || p[0] != 0xFF || p[1] != 0x00 {
        bail!("unexpected login reply: {:?}", &p[..p.len().min(4)]);
    }
    if p[2] != 0x01 {
        bail!("BERcon login FAILED (bad RCON password)");
    }

    // command, seq 0
    let mut data = Vec::with_capacity(1 + command.len());
    data.push(0x00);
    data.extend_from_slice(command.as_bytes());
    sock.send(&frame(0x01, &data)).await?;

    let mut out = String::new();
    loop {
        let n = match timeout(DRAIN_TIMEOUT, sock.recv(&mut buf)).await {
            Ok(r) => r?,
            Err(_) => break, // no more packets — done
        };
        let p = match payload(&buf[..n]) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if p.len() < 2 {
            continue;
        }
        match p[1] {
            0x01 => {
                let mut body: &[u8] = p.get(3..).unwrap_or(&[]); // skip 0xFF 0x01 <seq>
                if body.first() == Some(&0x00) && body.len() >= 3 {
                    body = &body[3..]; // strip multipart header 0x00 <total> <index>
                }
                out.push_str(&String::from_utf8_lossy(body));
            }
            0x02 => {
                // server keepalive — echo the seq back
                if p.len() >= 3 {
                    let _ = sock.send(&frame(0x02, &[p[2]])).await;
                }
            }
            _ => {}
        }
    }
    Ok(out.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_zlib() {
        // Canonical CRC-32/ISO-HDLC check value (== Python zlib.crc32(b"123456789")).
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0x0000_0000);
    }

    #[test]
    fn frame_layout_matches_python() {
        // login frame for password "x": 'BE' | crc32(0xFF 0x00 'x') LE | 0xFF 0x00 'x'
        let f = frame(0x00, b"x");
        assert_eq!(&f[..2], b"BE");
        let payload = &[0xFFu8, 0x00, b'x'];
        assert_eq!(&f[2..6], &crc32(payload).to_le_bytes());
        assert_eq!(&f[6..], payload);
    }
}
