//! Mod registry spine + live Monitor — the single, observable routing layer every migrated "mod"
//! plugs into. ONE `registered()` list (cached) both entrypoints drive via `run_registered_mods`,
//! so the main.rs↔service.rs spawn-list drift is structurally impossible. The router resolves each
//! event to claiming mods (by chat verb, priority-ordered) or hands every event to event-driven
//! mods, dispatches CONCURRENTLY (a slow Ollama/NPC reply never head-of-line-blocks), enforces
//! per-mod control-state + rate-limit + timeout, and records outcomes into a global `Monitor`.
//!
//! `monitor()` is a process-global the router writes and the HTTP `/monitor/*` + `/control/*`
//! handlers read — so turdmod-manager gets per-mod 🟢🟡🔴 health, a live activity feed, and
//! enable/disable/maintenance control without threading state through AppState. @dep [[world]].

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::{broadcast, Mutex};

use crate::events::GameEvent;
use crate::map_tracker::SharedSnapshot;
use crate::permissions::SharedPerms;
use crate::world::WorldCache;

/// Shared handles + live perception handed to every registered mod's `handle`.
#[derive(Clone)]
pub struct ModCtx {
    pub perms: SharedPerms,
    pub world: Arc<WorldCache>,
    /// Live map snapshot (player/vehicle positions) — what the legacy map-mods polled. @dep [[map_tracker]].
    pub map: SharedSnapshot,
}

/// What a mod reports back so the Monitor shows TRUE success/fail (not just timeout/latency).
pub enum Outcome {
    Handled,            // the mod acted on this event
    Ignored,            // not for this mod (no activity logged — keeps the feed signal-rich)
    Failed(String),     // the mod tried and errored
}

#[async_trait::async_trait]
pub trait Mod: Send + Sync {
    fn name(&self) -> &'static str;
    /// Chat verbs claimed (lowercased, leading '!'), matched against the first word of chat. Empty
    /// = event-driven: sees every event + self-filters.
    fn commands(&self) -> &'static [&'static str] { &[] }
    fn priority(&self) -> i8 { 0 }
    fn rate_limit(&self) -> Duration { Duration::ZERO }
    fn timeout(&self) -> Duration { Duration::from_secs(15) }
    /// If Some, the router runs `tick` on this interval (scheduled mods, e.g. the 2h spa). Ticks run
    /// only while Enabled and are recorded in metrics as "tick" activity.
    fn interval(&self) -> Option<Duration> { None }
    /// Whether this scheduled mod currently has work to do. Default true. Feature mods
    /// (duels/races/warzone/votes/jail…) override to return false when their feature is DORMANT,
    /// so the ticker skips them entirely — no wasted tick(), no monitor/activity-feed churn.
    /// @ctx: stops ~20 mods polling every 3s when nothing is actually running. Re-checked each
    /// interval (cheap timer wake); the mod still wakes on its command via handle() to become active.
    async fn active(&self) -> bool { true }
    /// Periodic action for scheduled mods. Default no-op.
    async fn tick(&self, _ctx: &ModCtx) {}
    async fn handle(&self, ev: &GameEvent, ctx: &ModCtx) -> Outcome;
}

pub type SharedMod = Arc<dyn Mod>;

/// THE single source of truth, cached so instances (and their state, e.g. DIMs memory) are built
/// ONCE and shared by the router + the monitor/control handlers. Migrate a mod = one line here.
static MODS: OnceLock<Vec<SharedMod>> = OnceLock::new();
pub fn registered() -> &'static Vec<SharedMod> { MODS.get_or_init(build_registered) }
fn build_registered() -> Vec<SharedMod> {
    vec![
        Arc::new(crate::hitmebaby::HitMeBaby),
        Arc::new(crate::restore_campaign::RestoreCampaignMod),
        Arc::new(crate::elevated::Elevated),
        Arc::new(crate::nightvote::NightVote::new()),
        Arc::new(crate::spa::Spa),
        Arc::new(crate::dims::Dims::new()),
        Arc::new(crate::rules::Rules::new()),
        Arc::new(crate::leaderboard::Leaderboard::new()),
        Arc::new(crate::kits::Kits::new()),
        Arc::new(crate::login_streak::LoginStreak::new()),
        Arc::new(crate::vehicle_durability::VehicleDurability::new()),
        Arc::new(crate::prestige::Prestige::new()),
        Arc::new(crate::title_system::TitleSystem::new()),
        Arc::new(crate::analytics::Analytics::new()),
        Arc::new(crate::safe_zones::SafeZones::new()),
        Arc::new(crate::pvp_zones::PvpZones::new()),
        Arc::new(crate::scoreboard::Scoreboard::new()),
        Arc::new(crate::bounty_hunter::BountyHunter::new()),
        // supply_drops PARKED 2026-06-10: grid_ref() uses a made-up 8x8 A-H/1-8 grid that
        // doesn't match SCUM's map, so it announces non-existent squares (E5, H8...) that nobody
        // can find -> always "expired, nobody claimed". Replaced by the real airdrop feature
        // (3-6 real drops in valid zones, native cargo event). See IDEAS.md.
        // Arc::new(crate::supply_drops::SupplyDrops::new()),
        Arc::new(crate::map_markers::MapMarkers::new()),
        Arc::new(crate::scavenger_hunt::ScavengerHunt::new()),
        Arc::new(crate::territory::Territory_::new()),
        Arc::new(crate::racing::Racing::new()),
        Arc::new(crate::convoy::Convoy::new()),
        Arc::new(crate::reputation::Reputation::new()),
        Arc::new(crate::radio::Radio::new()),
        Arc::new(crate::achievements::Achievements::new()),
        Arc::new(crate::fast_travel::FastTravel::new()),
        Arc::new(crate::lottery::Lottery::new()),
        Arc::new(crate::gambling::Gambling::new()),
        Arc::new(crate::banking::Banking::new()),
        Arc::new(crate::duels::Duels::new()),
        Arc::new(crate::skill_boost::SkillBoost::new()),
        Arc::new(crate::smoker_time::SmokerTime::new()),
        // trader_refill REMOVED 2026-06-19: dead — traders run unlimited funds via ServerSettings.
        Arc::new(crate::spawn_loadout::SpawnLoadout::new()),
        Arc::new(crate::chat_filter::ChatFilter::new()),
        // loot_multiplier REMOVED 2026-06-19: native loot config (TMM Loot tab + ServerSettings) covers it.
        Arc::new(crate::weather_alerts::WeatherAlerts::new()),
        Arc::new(crate::referral::Referral::new()),
        Arc::new(crate::mod_list::ModList::new()),
        Arc::new(crate::combat_log::CombatLog::new()),
        Arc::new(crate::death_recap::DeathRecap::new()),
        Arc::new(crate::death_notes::DeathNotes::new()),
        Arc::new(crate::voting::Voting::new()),
        Arc::new(crate::raid_alerts::RaidAlerts::new()),
        Arc::new(crate::team_balance::TeamBalance::new()),
        Arc::new(crate::building_zones::BuildingZones::new()),
        Arc::new(crate::npc_contracts::NpcContracts::new()),
        Arc::new(crate::jail::Jail::new()),
        Arc::new(crate::announcements::Announcements),
        Arc::new(crate::admin_log::AdminLog),
        Arc::new(crate::vac_screening::VacScreening::new()),
        Arc::new(crate::god_mode::GodMode::new()),
        Arc::new(crate::overlay::Overlay::new()),
        Arc::new(crate::bounty_board::BountyBoard::new()),
        Arc::new(crate::objective_events::ObjectiveEvents::new()),
        Arc::new(crate::metabolism::Metabolism::new()),
        Arc::new(crate::horde::Horde::new()),
        // airdrop REMOVED 2026-06-19: native cargo drops (#ScheduleCargoDrop + ServerSettings) cover it.
        Arc::new(crate::trading::Trading::new()),
        Arc::new(crate::quests::Quests::new()),
        Arc::new(crate::clans::Clans::new()),
        Arc::new(crate::event_scheduler::EventScheduler::new()),
        Arc::new(crate::fishing_tournament::FishingTournament::new()),
        Arc::new(crate::player_shops::PlayerShops::new()),
        Arc::new(crate::zilla_protection::ZillaProtection::new()),
        Arc::new(crate::vehicle_interactions::VehicleInteractions::new()),
        // tow PARKED 2026-06-10 (physics fight -> crash; see main.rs + IDEAS.md):
        // Arc::new(crate::tow::Tow::new()),
        // vehicle_ownership REMOVED 2026-06-10: it collided with chat_cmds (both claimed
        // !register/!transfer/!yes/!no/!unregister) and wrote vehicle_ownership.json with a
        // DIFFERENT (simple) schema, so chat_cmds' !release couldn't match its rows -> released
        // vehicles "came back". chat_cmds (event-driven, lock-based, rich schema = OVH's) is the
        // sole owner now. @dep [[reference_vehicle_revert_persistence]].
        Arc::new(crate::boss_fight::BossFight::new()),
        // spawn_protection REMOVED 2026-06-19: native spawn protection (ServerSettings) covers it.
        Arc::new(crate::perf_monitor::PerfMonitor::new()),
        Arc::new(crate::backup::Backup::new()),
        Arc::new(crate::auction::Auction::new()),
        Arc::new(crate::mechman::MechMan::new()),
        Arc::new(crate::economy::Economy::new()),
        Arc::new(crate::game_events::GameEvents::new()),
        Arc::new(crate::factions::Factions::new()),
        Arc::new(crate::player_db::PlayerDb::new()),
        Arc::new(crate::vehicles::Vehicles::new()),
        Arc::new(crate::vehicle_registry::VehicleRegistry::new()),
        // base_protection REMOVED 2026-06-19: native raid protection covers it.
        Arc::new(crate::warzone::Warzone::new()),
        Arc::new(crate::companions::Companions::new()),
        Arc::new(crate::afk::Afk::new()),
        Arc::new(crate::checkpoint::Checkpoint::new()),
        Arc::new(crate::auto_announce::AutoAnnounce::new()),
        // passive_control REMOVED 2026-06-19: native zombie/animal behavior via ServerSettings.
        // weather_cycle REMOVED 2026-06-19: native weather (#SetWeatherControllerOverride*/#SetTime).
        Arc::new(crate::chat_cmds::ChatCmds::new()),
        Arc::new(crate::permissions::Permissions::new()),
        Arc::new(crate::teleport::Teleport::new()),
        Arc::new(crate::npc::services::NpcServices::new()),
        Arc::new(crate::pvp_rotation::PvpRotation::new()),
        Arc::new(crate::vehicle_repo::VehicleRepo::new()),
    ]
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ─── Monitor (global; router writes, HTTP reads) ────────────────────────────────────────────

#[derive(Default, Clone, Serialize)]
pub struct ModMetric {
    pub calls: u64,
    pub handled: u64,
    pub ignored: u64,
    pub failed: u64,
    pub timeouts: u64,
    pub last_ms: u64,
    pub total_ms: u64,
    pub last_fired_secs: u64, // unix secs (0 = never)
    pub last_error: String,
}

#[derive(Clone, Serialize)]
pub struct ModView {
    pub name: String,
    pub status: &'static str, // "green" | "yellow" | "red"
    pub state: &'static str,  // "enabled" | "disabled" | "maintenance"
    pub commands: Vec<String>,
    pub avg_ms: u64,
    pub metric: ModMetric,
}

#[derive(Clone, Serialize)]
pub struct Activity {
    pub at: u64,
    pub event: String,
    pub from: String,
    pub mod_name: String,
    pub outcome: String, // "handled" | "failed" | "timeout"
    pub ms: u64,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ModState { Enabled, Disabled, Maintenance }

impl ModState {
    fn as_str(self) -> &'static str {
        match self { ModState::Enabled => "enabled", ModState::Disabled => "disabled", ModState::Maintenance => "maintenance" }
    }
    pub fn parse(s: &str) -> ModState {
        match s { "disabled" => ModState::Disabled, "maintenance" => ModState::Maintenance, _ => ModState::Enabled }
    }
}

pub struct Monitor {
    metrics: Mutex<HashMap<&'static str, ModMetric>>,
    activity: Mutex<VecDeque<Activity>>,
    control: Mutex<HashMap<&'static str, ModState>>,
}

static MONITOR: OnceLock<Monitor> = OnceLock::new();
pub fn monitor() -> &'static Monitor { MONITOR.get_or_init(Monitor::new) }

// Legacy (not-yet-trait) mods spawned via legacy::run_all — tracked by name so the Monitor lists
// every mod, not just the trait ones. Sync (std Mutex) since run_all is sync.
static LEGACY_NAMES: OnceLock<std::sync::Mutex<Vec<&'static str>>> = OnceLock::new();
fn legacy_cell() -> &'static std::sync::Mutex<Vec<&'static str>> {
    LEGACY_NAMES.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Mark a legacy mod as registered + running so it appears in the Monitor. @dep [[legacy]].
pub fn monitor_mark_legacy(name: &'static str) {
    if let Ok(mut v) = legacy_cell().lock() {
        if !v.contains(&name) { v.push(name); }
    }
}

impl Monitor {
    fn new() -> Self {
        Monitor {
            metrics: Mutex::new(HashMap::new()),
            activity: Mutex::new(VecDeque::new()),
            control: Mutex::new(HashMap::new()),
        }
    }

    pub async fn state_of(&self, name: &'static str) -> ModState {
        *self.control.lock().await.get(name).unwrap_or(&ModState::Enabled)
    }

    pub async fn set_state(&self, name: &'static str, st: ModState) {
        self.control.lock().await.insert(name, st);
    }

    /// Record an event delivered to a legacy mod (calls + last-fired). No latency/handled split —
    /// the mod processes in its own loop — but enough for activity + health + control. @dep legacy_dispatch.
    pub async fn tick_legacy(&self, name: &'static str) {
        let mut m = self.metrics.lock().await;
        let e = m.entry(name).or_default();
        e.calls += 1;
        e.last_fired_secs = now_secs();
    }

    async fn record(&self, name: &'static str, outcome: &Outcome, ms: u64, timed_out: bool, from: &str, event: &str) {
        let now = now_secs();
        {
            let mut m = self.metrics.lock().await;
            let e = m.entry(name).or_default();
            e.calls += 1;
            e.last_ms = ms;
            e.total_ms += ms;
            e.last_fired_secs = now;
            if timed_out { e.timeouts += 1; }
            match outcome {
                Outcome::Handled => e.handled += 1,
                Outcome::Ignored => e.ignored += 1,
                Outcome::Failed(msg) => { e.failed += 1; e.last_error = msg.clone(); }
            }
        }
        // Log only signal-rich activity (handled/failed/timeout); ignored is noise.
        let oc = if timed_out { "timeout" }
            else { match outcome { Outcome::Handled => "handled", Outcome::Ignored => return, Outcome::Failed(_) => "failed" } };
        let mut a = self.activity.lock().await;
        a.push_front(Activity { at: now, event: event.into(), from: from.into(), mod_name: name.into(), outcome: oc.into(), ms });
        while a.len() > 300 { a.pop_back(); }
    }

    /// Per-mod snapshot with computed 🟢🟡🔴 health — backs `/monitor/mods`.
    pub async fn mod_views(&self) -> Vec<ModView> {
        let metrics = self.metrics.lock().await;
        let control = self.control.lock().await;
        let mut views: Vec<ModView> = registered().iter().map(|m| {
            let name = m.name();
            let met = metrics.get(name).cloned().unwrap_or_default();
            let state = *control.get(name).unwrap_or(&ModState::Enabled);
            let avg = if met.calls > 0 { met.total_ms / met.calls } else { 0 };
            ModView {
                name: name.to_string(),
                status: health(&met, state),
                state: state.as_str(),
                commands: m.commands().iter().map(|c| c.to_string()).collect(),
                avg_ms: avg,
                metric: met,
            }
        }).collect();
        // legacy mods: real calls/last-fired metrics (via legacy_dispatch) + control state + health,
        // same treatment as trait mods (minus the handled/failed/latency split).
        if let Ok(names) = legacy_cell().lock() {
            for &n in names.iter() {
                let met = metrics.get(n).cloned().unwrap_or_default();
                let state = *control.get(n).unwrap_or(&ModState::Enabled);
                let avg = if met.calls > 0 { met.total_ms / met.calls } else { 0 };
                views.push(ModView {
                    name: n.to_string(),
                    status: health(&met, state),
                    state: state.as_str(),
                    commands: vec![],
                    avg_ms: avg,
                    metric: met,
                });
            }
        }
        views
    }

    pub async fn recent_activity(&self, n: usize) -> Vec<Activity> {
        self.activity.lock().await.iter().take(n).cloned().collect()
    }
}

/// 🟢🟡🔴 from control state + failure rate + latency. Red = off/failing; yellow = maintenance/
/// slow/elevated-errors/idle-unknown; green = healthy.
fn health(m: &ModMetric, state: ModState) -> &'static str {
    match state {
        ModState::Disabled => return "red",
        ModState::Maintenance => return "yellow",
        ModState::Enabled => {}
    }
    if m.calls == 0 { return "yellow"; } // registered but never fired — unknown
    let fails = m.failed + m.timeouts;
    let rate = fails as f64 / m.calls as f64;
    if rate >= 0.25 { "red" } else if rate >= 0.05 || m.last_ms > 3000 { "yellow" } else { "green" }
}

/// Resolve a mod name (trait OR legacy) to its &'static str so /control/mod can target any mod.
pub fn resolve_name(name: &str) -> Option<&'static str> {
    if let Some(m) = registered().iter().find(|m| m.name() == name) { return Some(m.name()); }
    if let Ok(v) = legacy_cell().lock() {
        if let Some(&n) = v.iter().find(|&&n| n == name) { return Some(n); }
    }
    None
}

/// Fan-out dispatcher: forward main-bus events to each legacy mod's gated channel ONLY while that
/// mod is Enabled (control), recording a metric tick per delivery. This gives every legacy mod the
/// same enable/disable/maintenance control + calls/last-fired/health as the trait mods, with NO
/// per-mod code change. @dep legacy::run_all builds `gated`.
pub async fn legacy_dispatch(
    mut rx: broadcast::Receiver<GameEvent>,
    gated: Vec<(&'static str, broadcast::Sender<GameEvent>)>,
) {
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => return,
        };
        for (name, tx) in &gated {
            if monitor().state_of(name).await != ModState::Enabled { continue; }
            monitor().tick_legacy(name).await;
            let _ = tx.send(ev.clone());
        }
    }
}

// ─── Router ─────────────────────────────────────────────────────────────────────────────────

pub async fn run_registered_mods(tx: broadcast::Sender<GameEvent>, ctx: ModCtx) {
    let mods = registered();
    let mut by_cmd: HashMap<&'static str, Vec<usize>> = HashMap::new();
    let mut event_driven: Vec<usize> = Vec::new();
    for (i, m) in mods.iter().enumerate() {
        if m.commands().is_empty() { event_driven.push(i); }
        for c in m.commands() { by_cmd.entry(*c).or_default().push(i); }
    }
    for ids in by_cmd.values_mut() {
        ids.sort_by_key(|&i| std::cmp::Reverse(mods[i].priority()));
    }
    tracing::info!(
        "registry: {} mod(s) — {} command verb(s), {} event-driven",
        mods.len(), by_cmd.len(), event_driven.len()
    );

    // Scheduled mods: spawn a per-mod ticker (gated on control state, recorded as "tick").
    for m in mods.iter() {
        if let Some(iv) = m.interval() {
            let m = Arc::clone(m);
            let ctx = ctx.clone();
            tokio::spawn(async move {
                let mut tk = tokio::time::interval(iv);
                tk.tick().await; // drop the immediate fire
                loop {
                    tk.tick().await;
                    if monitor().state_of(m.name()).await != ModState::Enabled { continue; }
                    if !m.active().await { continue; } // dormant feature mod — skip tick + record
                    let start = Instant::now();
                    m.tick(&ctx).await;
                    let ms = start.elapsed().as_millis() as u64;
                    monitor().record(m.name(), &Outcome::Handled, ms, false, "", "tick").await;
                }
            });
        }
    }

    let last_fire: Arc<Mutex<HashMap<&'static str, Instant>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut rx = tx.subscribe();
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("registry: lagged {} events", n);
                continue;
            }
            Err(_) => return,
        };

        // Capture every chat line into the ring buffer (before mod-target filtering,
        // so lines no mod handles still land) for the hosted admin map's /chat/recent.
        if ev.event == "chat" {
            crate::chat_log::record(&ev);
        }

        let mut targets: Vec<usize> = Vec::new();
        if ev.event == "chat" {
            let verb = ev.data.get("text").and_then(|v| v.as_str())
                .map(|t| t.trim().split_whitespace().next().unwrap_or("").to_ascii_lowercase())
                .unwrap_or_default();
            if let Some(ids) = by_cmd.get(verb.as_str()) { targets.extend(ids); }
        }
        targets.extend(&event_driven);
        if targets.is_empty() { continue; }

        let from = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let event_name = ev.event.clone();
        let ev = Arc::new(ev);
        for &i in &targets {
            let m = Arc::clone(&mods[i]);
            let ctx = ctx.clone();
            let ev = Arc::clone(&ev);
            let last_fire = Arc::clone(&last_fire);
            let from = from.clone();
            let event_name = event_name.clone();
            tokio::spawn(async move {
                // control gate: disabled/maintenance mods don't dispatch
                if monitor().state_of(m.name()).await != ModState::Enabled { return; }
                let rl = m.rate_limit();
                if !rl.is_zero() {
                    let mut lf = last_fire.lock().await;
                    let now = Instant::now();
                    if let Some(prev) = lf.get(m.name()) {
                        if now.duration_since(*prev) < rl { return; }
                    }
                    lf.insert(m.name(), now);
                }
                let start = Instant::now();
                let res = tokio::time::timeout(m.timeout(), m.handle(ev.as_ref(), &ctx)).await;
                let ms = start.elapsed().as_millis() as u64;
                let (outcome, timed_out) = match res {
                    Ok(o) => (o, false),
                    Err(_) => (Outcome::Failed("timeout".into()), true),
                };
                monitor().record(m.name(), &outcome, ms, timed_out, &from, &event_name).await;
            });
        }
    }
}
