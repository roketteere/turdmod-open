//! Image-host whitelist for the `<img src="https://...">` decorator.
//!
//! File: `%USERPROFILE%/.scummy-map/turdmod-image-hosts.txt`
//!
//! Format: one host per line. Lines starting with `#` are comments. Blank
//! lines ignored. No wildcards in v1 — subdomains must be listed
//! explicitly. Comparison is case-insensitive on the host portion only.
//!
//! Default seed (written if the file is absent on first call):
//!
//!   ```
//!   # TurdMOD image-host whitelist — one host per line.
//!   # The decorator DLL only fetches <img src> URLs whose host matches.
//!   cdn.scummap.com
//!   placehold.co
//!   i.imgur.com
//!   ```
//!
//! Why a file (not baked-in)? Server admins / overlay players legitimately
//! want to add their own CDN hosts (community-server logos hosted on a
//! static-site bucket, etc.). A file lets them edit without rebuilding the
//! DLL. The lock-file pattern of "one host per line" is intentionally
//! boring so it doesn't grow into a config DSL.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

const DEFAULT_HOSTS: &[&str] = &[
    "cdn.scummap.com",
    "placehold.co",
    "i.imgur.com",
];

const SEED_FILE: &str = "\
# TurdMOD image-host whitelist — one host per line.
# The decorator DLL only fetches <img src> URLs whose host matches.
# Lines starting with '#' are comments. Blank lines ignored.
# Edit + save; the next decorator-load picks up the change.
cdn.scummap.com
placehold.co
i.imgur.com
";

#[derive(Debug, Clone)]
pub struct Whitelist {
    hosts: HashSet<String>,
    pub source: WhitelistSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhitelistSource {
    /// Loaded from disk at the given path.
    File(PathBuf),
    /// Defaults applied because the file doesn't exist and we couldn't seed it.
    DefaultsInMemory,
    /// Defaults seeded into the named file on first load.
    DefaultsSeededTo(PathBuf),
}

impl Whitelist {
    pub fn allows(&self, host: &str) -> bool {
        self.hosts.contains(&host.to_ascii_lowercase())
    }

    pub fn host_count(&self) -> usize {
        self.hosts.len()
    }

    /// Build a whitelist from a string body. Each non-comment, non-empty line
    /// is one host (lowercased on insert).
    pub fn from_body(body: &str, source: WhitelistSource) -> Self {
        let hosts = body
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| line.to_ascii_lowercase())
            .collect();
        Self { hosts, source }
    }

    /// Seed an in-memory defaults whitelist (no disk).
    pub fn defaults_in_memory() -> Self {
        let mut hosts = HashSet::new();
        for h in DEFAULT_HOSTS {
            hosts.insert((*h).to_ascii_lowercase());
        }
        Self { hosts, source: WhitelistSource::DefaultsInMemory }
    }
}

/// Resolve the whitelist file path: `%USERPROFILE%/.scummy-map/turdmod-image-hosts.txt`.
/// Returns None if `USERPROFILE` is not set (shouldn't happen on Windows
/// but handle gracefully).
pub fn whitelist_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")?;
    let mut p = PathBuf::from(home);
    p.push(".scummy-map");
    p.push("turdmod-image-hosts.txt");
    Some(p)
}

/// Load the whitelist from disk, seeding defaults to the file if absent.
/// Falls back to in-memory defaults if the seed write fails.
pub fn load() -> Whitelist {
    let Some(path) = whitelist_path() else {
        return Whitelist::defaults_in_memory();
    };
    if let Ok(body) = fs::read_to_string(&path) {
        return Whitelist::from_body(&body, WhitelistSource::File(path));
    }
    // Seed defaults.
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if fs::write(&path, SEED_FILE).is_ok() {
        return Whitelist::from_body(SEED_FILE, WhitelistSource::DefaultsSeededTo(path));
    }
    Whitelist::defaults_in_memory()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hosts_strips_comments_and_blanks() {
        let body = "# top comment\n\n  cdn.scummap.com  \n# inline-comment line\nplacehold.co\n\n";
        let wl = Whitelist::from_body(body, WhitelistSource::DefaultsInMemory);
        assert_eq!(wl.host_count(), 2);
        assert!(wl.allows("cdn.scummap.com"));
        assert!(wl.allows("CDN.SCUMMAP.COM"));
        assert!(wl.allows("placehold.co"));
        assert!(!wl.allows("evil.example"));
    }

    #[test]
    fn no_wildcards_subdomains_must_match_explicitly() {
        let body = "cdn.scummap.com\n";
        let wl = Whitelist::from_body(body, WhitelistSource::DefaultsInMemory);
        assert!(wl.allows("cdn.scummap.com"));
        assert!(!wl.allows("static.cdn.scummap.com"));
        assert!(!wl.allows("scummap.com"));
    }

    #[test]
    fn defaults_in_memory_has_known_hosts() {
        let wl = Whitelist::defaults_in_memory();
        for h in DEFAULT_HOSTS {
            assert!(wl.allows(h), "expected default to allow {h}");
        }
    }

    #[test]
    fn seed_to_tempfile_then_reload() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts.txt");
        fs::write(&path, SEED_FILE).unwrap();
        let body = fs::read_to_string(&path).unwrap();
        let wl = Whitelist::from_body(&body, WhitelistSource::File(path.clone()));
        assert!(wl.allows("placehold.co"));
        assert!(wl.allows("i.imgur.com"));
    }
}
