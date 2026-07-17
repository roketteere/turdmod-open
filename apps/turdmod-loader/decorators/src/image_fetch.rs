//! HTTP fetch + image decode pipeline for the `<img src="https://...">`
//! decorator.
//!
//! Flow: URL → host-allowlist check → ureq GET → bytes → image decode →
//! `Bgra8Buffer { width, height, bgra: Vec<u8> }`. The buffer is what
//! UE4's UTexture2D::CreateTransient consumes (PF_B8G8R8A8 pixel format).
//!
//! Caps applied here (not at UE4 boundary): max body bytes (default 512 KB),
//! request timeout (3s), decoded-pixel cap (default 4 MB raw buffer = 1024×1024
//! BGRA). All overridable via config.
//!
//! No UE4 internals here — this module is fully testable in isolation via
//! Cargo `cargo test --package turdmod-rich-decorators`.

use std::time::Duration;

use crate::whitelist::Whitelist;

#[derive(Debug)]
pub struct Bgra8Buffer {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct FetchLimits {
    pub max_body_bytes: usize,
    pub timeout: Duration,
    pub max_decoded_bytes: usize,
}

impl Default for FetchLimits {
    fn default() -> Self {
        Self {
            max_body_bytes: 512 * 1024,
            timeout: Duration::from_secs(3),
            max_decoded_bytes: 4 * 1024 * 1024, // 1024×1024 BGRA
        }
    }
}

#[derive(Debug)]
pub enum FetchError {
    InvalidUrl(String),
    HostNotAllowed { host: String },
    HttpError(String),
    BodyTooLarge { read: usize, cap: usize },
    DecodeError(String),
    BufferTooLarge { decoded: usize, cap: usize },
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::InvalidUrl(u) => write!(f, "invalid url: {u}"),
            FetchError::HostNotAllowed { host } => write!(f, "host not in whitelist: {host}"),
            FetchError::HttpError(s) => write!(f, "http error: {s}"),
            FetchError::BodyTooLarge { read, cap } => write!(f, "body too large: {read} > {cap}"),
            FetchError::DecodeError(s) => write!(f, "decode error: {s}"),
            FetchError::BufferTooLarge { decoded, cap } => {
                write!(f, "decoded buffer too large: {decoded} > {cap}")
            }
        }
    }
}

impl std::error::Error for FetchError {}

/// Parse the URL, check host against the whitelist, GET the body up to
/// `limits.max_body_bytes`, decode as PNG/JPG, and return a BGRA8 buffer
/// suitable for UTexture2D::CreateTransient. Errors describe the failure
/// stage so the decorator detour can fall back to alt text.
pub fn fetch_and_decode(
    url: &str,
    whitelist: &Whitelist,
    limits: &FetchLimits,
) -> Result<Bgra8Buffer, FetchError> {
    let parsed = url::Url::parse(url).map_err(|e| FetchError::InvalidUrl(format!("{url}: {e}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| FetchError::InvalidUrl(format!("no host in {url}")))?
        .to_ascii_lowercase();

    if !whitelist.allows(&host) {
        return Err(FetchError::HostNotAllowed { host });
    }

    let agent = ureq::AgentBuilder::new()
        .timeout_read(limits.timeout)
        .timeout_write(limits.timeout)
        .timeout_connect(limits.timeout)
        .user_agent(concat!("turdmod-rich-decorators/", env!("CARGO_PKG_VERSION")))
        .build();

    let resp = agent
        .get(url)
        .call()
        .map_err(|e| FetchError::HttpError(format!("{e}")))?;

    let mut body = Vec::with_capacity(64 * 1024);
    let mut reader = resp.into_reader().take(limits.max_body_bytes as u64 + 1);
    use std::io::Read;
    reader
        .read_to_end(&mut body)
        .map_err(|e| FetchError::HttpError(format!("read: {e}")))?;
    if body.len() > limits.max_body_bytes {
        return Err(FetchError::BodyTooLarge {
            read: body.len(),
            cap: limits.max_body_bytes,
        });
    }

    decode_bytes(&body, limits)
}

/// Decode an in-memory image buffer (PNG / JPG) to BGRA8. Public so the
/// detour can also accept `<img id="...">` paths that resolve to local
/// pak-embedded bytes.
pub fn decode_bytes(body: &[u8], limits: &FetchLimits) -> Result<Bgra8Buffer, FetchError> {
    let img = image::load_from_memory(body)
        .map_err(|e| FetchError::DecodeError(format!("{e}")))?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let raw = rgba.into_raw();

    if raw.len() > limits.max_decoded_bytes {
        return Err(FetchError::BufferTooLarge {
            decoded: raw.len(),
            cap: limits.max_decoded_bytes,
        });
    }

    // Convert RGBA → BGRA in place (PF_B8G8R8A8 is what UE4 expects for
    // the typical Texture2D::CreateTransient brush).
    let mut bgra = raw;
    let mut i = 0;
    while i + 3 < bgra.len() {
        bgra.swap(i, i + 2);
        i += 4;
    }

    Ok(Bgra8Buffer { width: w, height: h, bgra })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_not_allowed_short_circuits_before_network() {
        let wl = Whitelist::defaults_in_memory();
        let err = fetch_and_decode(
            "https://evil.example/x.png",
            &wl,
            &FetchLimits::default(),
        )
        .unwrap_err();
        match err {
            FetchError::HostNotAllowed { host } => assert_eq!(host, "evil.example"),
            other => panic!("expected HostNotAllowed, got {other:?}"),
        }
    }

    #[test]
    fn invalid_url_fails_before_network() {
        let wl = Whitelist::defaults_in_memory();
        let err = fetch_and_decode("not a url", &wl, &FetchLimits::default()).unwrap_err();
        matches!(err, FetchError::InvalidUrl(_));
    }

    /// Encode a known-valid PNG at test time using the same `image` crate
    /// we depend on. Self-validating: if encode → decode round-trips, the
    /// pipeline works.
    fn make_test_png(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut img = image::RgbaImage::new(w, h);
        for px in img.pixels_mut() {
            *px = image::Rgba(rgba);
        }
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    #[test]
    fn decodes_a_real_png_buffer_to_bgra() {
        // 2×1 magenta (R=255, G=0, B=255, A=255).
        let png = make_test_png(2, 1, [255, 0, 255, 255]);
        let buf = decode_bytes(&png, &FetchLimits::default()).unwrap();
        assert_eq!(buf.width, 2);
        assert_eq!(buf.height, 1);
        // 2 × 1 × 4 bytes BGRA.
        assert_eq!(buf.bgra.len(), 8);
        // Magenta in BGRA = B=255, G=0, R=255, A=255 (RGBA → BGRA swaps R↔B).
        assert_eq!(buf.bgra[0], 255, "pixel 0 B");
        assert_eq!(buf.bgra[1], 0,   "pixel 0 G");
        assert_eq!(buf.bgra[2], 255, "pixel 0 R");
        assert_eq!(buf.bgra[3], 255, "pixel 0 A");
    }

    #[test]
    fn decoded_buffer_too_large_rejected() {
        // 4×4 PNG = 64 bytes BGRA decoded; cap at 32 to force the limit hit.
        let png = make_test_png(4, 4, [10, 20, 30, 255]);
        let limits = FetchLimits {
            max_decoded_bytes: 32,
            ..FetchLimits::default()
        };
        let err = decode_bytes(&png, &limits).unwrap_err();
        match err {
            FetchError::BufferTooLarge { decoded, cap } => {
                assert_eq!(decoded, 64);
                assert_eq!(cap, 32);
            }
            other => panic!("expected BufferTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn garbage_bytes_fail_decode_cleanly() {
        let garbage = vec![0u8; 64];
        let err = decode_bytes(&garbage, &FetchLimits::default()).unwrap_err();
        match err {
            FetchError::DecodeError(_) => {}
            other => panic!("expected DecodeError, got {other:?}"),
        }
    }
}
