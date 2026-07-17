//! Legacy-mod adapter — every mod not yet converted to the `registry::Mod` trait, spawned from ONE
//! list (`run_all`) that BOTH entrypoints call. Ends the main.rs/service.rs drift for ALL mods.
//!
//! Each event-driven legacy mod now gets its OWN gated channel instead of subscribing to the main
//! bus directly: a single `registry::legacy_dispatch` forwards bus events to each mod's channel
//! ONLY while that mod is Enabled (enable/disable/maintenance control) and records a metric tick
//! (calls/last-fired/health) per delivery. So every legacy mod gets the SAME control + activity
//! metrics as the trait mods — with NO per-mod code change (the mods still just `tx.subscribe()`,
//! now on their private gated channel). Timer/announcer mods (no `tx`) are marked but not gated.
//! @inv add a mod HERE, never in main.rs/service.rs.

use tokio::sync::broadcast;

use crate::events::GameEvent;
use crate::map_tracker::SharedSnapshot;
use crate::permissions::SharedPerms;

pub struct LegacyCtx {
    pub ev_tx: broadcast::Sender<GameEvent>,
    pub perms: SharedPerms,
    pub map: SharedSnapshot,
}

/// Spawn every legacy mod + a single dispatcher that gates/meters their events. Single source of truth.
pub fn run_all(c: &LegacyCtx) {
    // (name, sender) for each event-gated mod; the dispatcher forwards bus events to these.
    let mut gated: Vec<(&'static str, broadcast::Sender<GameEvent>)> = Vec::new();

    // Make a private gated channel for a mod, register it, return the sender to pass as the mod's tx.
    macro_rules! gtx {
        ($name:literal) => {{
            let (s, _r) = broadcast::channel::<GameEvent>(256);
            gated.push(($name, s.clone()));
            crate::registry::monitor_mark_legacy($name);
            s
        }};
    }
    // Mark a non-event-driven mod (timer/announcer) so it still lists in the Monitor.
    macro_rules! mark { ($name:literal) => { crate::registry::monitor_mark_legacy($name); }; }

    // command interpreter + permissions + teleport (need perms)

    // event/chat-driven mods (gated)

    // map-snapshot mods (map first, then gated tx)

    // non-event-driven mods (timers/announcers/poll) — marked, not gated

    // (friendly_passive replaced by the registry trait mod `passive_control` — !zombies/!animals
    // passive on/off, enforced on its 30s tick, with a live toggle + banner. No restart needed.)

    // The single gating + metering dispatcher: main bus -> per-mod gated channels.
    let main_rx = c.ev_tx.subscribe();
    tokio::spawn(crate::registry::legacy_dispatch(main_rx, gated));
}
