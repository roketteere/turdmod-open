// Owner identity — read from config, never baked into the binary.
//
// @ctx: these used to be `const OWNER_STEAM_ID` / `OWNER_NAME` constants copied
//   into ~46 files. That made the open-source scrub and a production deploy
//   mutually exclusive: scrubbing the source for publication produced a binary
//   that silently stopped recognising the owner (god mode, safe zones,
//   teleport, spa, warzone… all just ignoring them, with no error). See
//   DEPLOY-WARNING.md.
//
// @inv: no real SteamID may appear in this file or anywhere else in src/. The
//   values come from service.json, which is private and never published. That's
//   what makes the public source safe BY CONSTRUCTION rather than by scrubbing.
// @inv: init() is called once at startup from the loaded Config. Mods must go
//   through is_owner/is_owner_steam rather than comparing strings themselves —
//   a stray literal is exactly the thing this module exists to prevent.

use std::sync::OnceLock;

#[derive(Debug, Default)]
pub struct Owner {
    ids: Vec<String>,
    name: String,
}

static OWNER: OnceLock<Owner> = OnceLock::new();

/// Install the owner identity from config. Later calls are ignored, so tests
/// and a double-init can't clobber the real values.
pub fn init(ids: Vec<String>, name: String) {
    let ids = ids.into_iter().filter(|s| !s.trim().is_empty()).collect();
    let _ = OWNER.set(Owner { ids, name: name.trim().to_string() });
}

fn get() -> &'static Owner {
    OWNER.get_or_init(Owner::default)
}

/// True if this SteamID belongs to an owner.
///
/// @inv: an unconfigured owner list matches NOBODY. Failing closed matters —
///   the alternative (matching everyone) would hand owner powers to every
///   player on a server whose config forgot the field.
pub fn is_owner_steam(steam: &str) -> bool {
    let s = steam.trim();
    !s.is_empty() && get().ids.iter().any(|id| id == s)
}

/// True if either the SteamID or the in-game name identifies the owner.
/// Name matching is a convenience for chat paths that only carry a name;
/// SteamID is the authoritative check.
pub fn is_owner(steam: &str, player: &str) -> bool {
    if is_owner_steam(steam) {
        return true;
    }
    let n = get().name.as_str();
    !n.is_empty() && player.trim() == n
}

/// The owner's display name, or empty when unconfigured.
pub fn name() -> &'static str {
    &get().name
}

/// The first configured owner SteamID, or empty. For seeding permissions.
pub fn primary_id() -> &'static str {
    get().ids.first().map(|s| s.as_str()).unwrap_or("")
}

/// Every configured owner SteamID.
pub fn ids() -> &'static [String] {
    &get().ids
}

/// True when nobody is configured — the service logs this loudly at startup,
/// because it means owner-gated mods will ignore everyone.
pub fn is_unconfigured() -> bool {
    get().ids.is_empty() && get().name.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    // OnceLock is process-global, so exercise the matching logic directly
    // rather than fighting init() ordering across parallel tests.
    fn owner_of(ids: &[&str], name: &str) -> Owner {
        Owner { ids: ids.iter().map(|s| s.to_string()).collect(), name: name.into() }
    }
    fn is_owner_in(o: &Owner, steam: &str, player: &str) -> bool {
        let s = steam.trim();
        if !s.is_empty() && o.ids.iter().any(|i| i == s) {
            return true;
        }
        !o.name.is_empty() && player.trim() == o.name
    }

    #[test]
    fn matches_any_configured_id_or_the_name() {
        let o = owner_of(&["111", "222"], "Owner");
        assert!(is_owner_in(&o, "111", "someone"));
        assert!(is_owner_in(&o, "222", "someone"));
        assert!(is_owner_in(&o, "999", "Owner"), "name is a valid fallback");
        assert!(!is_owner_in(&o, "999", "someone"));
    }

    /// @inv: the dangerous failure is matching everyone. An empty config must
    /// match nobody, not grant owner to all.
    #[test]
    fn an_unconfigured_owner_matches_nobody() {
        let o = owner_of(&[], "");
        assert!(!is_owner_in(&o, "111", "anyone"));
        assert!(!is_owner_in(&o, "", ""), "empty steam + empty name must not match");
    }

    /// A blank SteamID must never match a blank configured entry.
    #[test]
    fn blank_ids_never_match() {
        let o = owner_of(&["111"], "Owner");
        assert!(!is_owner_in(&o, "", "nobody"));
        assert!(!is_owner_in(&o, "   ", "nobody"));
    }

    #[test]
    fn init_filters_blank_entries() {
        let raw: Vec<String> = vec!["111".to_string(), String::new(), "  ".to_string()];
        let ids: Vec<String> = raw.into_iter().filter(|s| !s.trim().is_empty()).collect();
        assert_eq!(ids, vec!["111".to_string()]);
    }
}
