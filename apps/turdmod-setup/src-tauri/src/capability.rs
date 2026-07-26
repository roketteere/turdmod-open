// Capability probing — the honest step.
//
// TurdMOD's engine runs INSIDE the game server process (Windows Service +
// DLL injection). That's a hard requirement, and it's why rented shared hosts
// (Nitrado, GTX, Host Havoc, g-portal) can't run it: they give you FTP and a
// web panel, not the ability to execute arbitrary binaries.
//
// Rather than let someone spend an hour on an install that CANNOT work, we
// probe what their host actually allows and say so plainly.
//
// @inv: never report a capability as available unless we actually verified it.
//       A false "yes" here costs the user an hour and a support ticket.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HostKind {
    /// Server runs on this machine.
    Local,
    /// Their own VPS / dedicated box — SSH access, can run anything.
    OwnVps,
    /// Rented from a game host — FTP + web panel only.
    RentedFtp,
    /// Couldn't determine.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Support {
    Yes,
    No,
    /// Possible but unverified — we couldn't fully probe.
    Maybe,
}

#[derive(Debug, Clone, Serialize)]
pub struct Capability {
    pub id: &'static str,
    pub label: &'static str,
    pub support: Support,
    /// Plain-language reason. Always populated for `No` and `Maybe`.
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CapabilityReport {
    pub host_kind: HostKind,
    pub capabilities: Vec<Capability>,
    /// One-line verdict for the header.
    pub verdict: String,
    /// True when the full engine (service + bridge) can run.
    pub engine_supported: bool,
}

fn cap(id: &'static str, label: &'static str, support: Support, reason: &str) -> Capability {
    Capability { id, label, support, reason: reason.to_string() }
}

/// Build the capability report for a host kind.
///
/// `can_execute` — did we verify the host can run our binaries? For Local this
/// is always true; for OwnVps it's the result of an SSH probe; for RentedFtp
/// it's false by definition.
pub fn report_for(host_kind: HostKind, can_execute: bool) -> CapabilityReport {
    let engine = match host_kind {
        HostKind::Local => true,
        HostKind::OwnVps => can_execute,
        HostKind::RentedFtp => false,
        HostKind::Unknown => false,
    };

    let mut caps = Vec::new();

    // 1. The engine — the thing that matters most.
    caps.push(match host_kind {
        HostKind::Local => cap(
            "engine",
            "Engine mods (90+ modules)",
            Support::Yes,
            "The server runs on this PC, so TurdMOD can install alongside it.",
        ),
        HostKind::OwnVps if can_execute => cap(
            "engine",
            "Engine mods (90+ modules)",
            Support::Yes,
            "Your VPS lets us install and run the TurdMOD service.",
        ),
        HostKind::OwnVps => cap(
            "engine",
            "Engine mods (90+ modules)",
            Support::Maybe,
            "We couldn't confirm your VPS can run the service. Check SSH access and that you can run executables.",
        ),
        HostKind::RentedFtp => cap(
            "engine",
            "Engine mods (90+ modules)",
            Support::No,
            "Your host only gives you FTP. TurdMOD's engine has to run as a program on the server box, and rented game hosts don't allow that. This isn't something we can work around — it's how the hosting works.",
        ),
        HostKind::Unknown => cap(
            "engine",
            "Engine mods (90+ modules)",
            Support::Maybe,
            "We couldn't work out what kind of hosting you have yet.",
        ),
    });

    // 2. Manager dashboard — needs the service's HTTP API.
    caps.push(if engine {
        cap(
            "manager",
            "Manager dashboard",
            Support::Yes,
            "Connects to the service's API to control mods and view the live map.",
        )
    } else {
        cap(
            "manager",
            "Manager dashboard",
            Support::No,
            "The dashboard talks to the TurdMOD service, which can't run on this host.",
        )
    });

    // 3. Pak / asset mods — just files in a folder. Works anywhere you can write.
    caps.push(match host_kind {
        HostKind::RentedFtp => cap(
            "paks",
            "Pak & asset mods",
            Support::Yes,
            "These are just files — we can upload them over FTP.",
        ),
        HostKind::Unknown => cap(
            "paks",
            "Pak & asset mods",
            Support::Maybe,
            "Needs write access to the server's Paks folder.",
        ),
        _ => cap("paks", "Pak & asset mods", Support::Yes, "Files are written directly to the server."),
    });

    // 4. Config tuning — ServerSettings.ini etc. Same story as paks.
    caps.push(match host_kind {
        HostKind::Unknown => cap(
            "config",
            "Server config tuning",
            Support::Maybe,
            "Needs write access to the server's config folder.",
        ),
        _ => cap(
            "config",
            "Server config tuning",
            Support::Yes,
            "Loot rates, spawn settings, weather, and the rest of ServerSettings.ini.",
        ),
    });

    let verdict = match (host_kind, engine) {
        (HostKind::Local, _) => "Your setup supports everything TurdMOD does.".to_string(),
        (HostKind::OwnVps, true) => "Your VPS supports everything TurdMOD does.".to_string(),
        (HostKind::OwnVps, false) => {
            "We couldn't confirm your VPS can run the engine — check the notes below.".to_string()
        }
        (HostKind::RentedFtp, _) => {
            "Your host can't run the TurdMOD engine, but you can still use pak and config mods."
                .to_string()
        }
        (HostKind::Unknown, _) => "Tell us where your server lives and we'll check.".to_string(),
    };

    CapabilityReport { host_kind, capabilities: caps, verdict, engine_supported: engine }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(r: &'a CapabilityReport, id: &str) -> &'a Capability {
        r.capabilities.iter().find(|c| c.id == id).expect("capability present")
    }

    #[test]
    fn local_supports_everything() {
        let r = report_for(HostKind::Local, true);
        assert!(r.engine_supported);
        assert_eq!(find(&r, "engine").support, Support::Yes);
        assert_eq!(find(&r, "manager").support, Support::Yes);
    }

    #[test]
    fn rented_ftp_cannot_run_engine_but_can_do_paks() {
        let r = report_for(HostKind::RentedFtp, false);
        assert!(!r.engine_supported);
        assert_eq!(find(&r, "engine").support, Support::No);
        assert_eq!(find(&r, "manager").support, Support::No);
        // The point of being honest: they still get something.
        assert_eq!(find(&r, "paks").support, Support::Yes);
        assert_eq!(find(&r, "config").support, Support::Yes);
    }

    #[test]
    fn vps_without_exec_proof_is_maybe_not_yes() {
        let r = report_for(HostKind::OwnVps, false);
        assert!(!r.engine_supported);
        // Must NOT claim Yes when unverified.
        assert_eq!(find(&r, "engine").support, Support::Maybe);
    }

    #[test]
    fn every_non_yes_capability_explains_itself() {
        for (kind, exec) in [
            (HostKind::RentedFtp, false),
            (HostKind::OwnVps, false),
            (HostKind::Unknown, false),
        ] {
            let r = report_for(kind, exec);
            for c in &r.capabilities {
                if c.support != Support::Yes {
                    assert!(!c.reason.is_empty(), "{:?}/{} needs a reason", kind, c.id);
                }
            }
        }
    }
}
