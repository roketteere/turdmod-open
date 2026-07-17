// NPC Services — intel shop, bounty contracts, periodic warnings
// State: C:\TurdMOD\data\npc_services.json

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const SERVICES_PATH: &str = r"C:\TurdMOD\data\npc_services.json";
const ECONOMY_PATH: &str = r"C:\TurdMOD\data\economy.json";
const REL_DIR: &str = r"C:\TurdMOD\npc\relationships";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const WARN_INTERVAL: Duration = Duration::from_secs(600);

#[derive(Debug, Clone)]
struct IntelEntry {
    id: &'static str, npc_id: &'static str, preview: &'static str,
    full_hint: &'static str, price: i64, tier_min: &'static str,
}

static INTEL: &[IntelEntry] = &[
    IntelEntry { id: "ziggy_rifle", npc_id: "ziggy", tier_min: "neutral", price: 50, preview: "Rifle cache location", full_hint: "M4 rifles in the bunker at grid D7, bottom shelf." },
    IntelEntry { id: "ziggy_ammo", npc_id: "ziggy", tier_min: "neutral", price: 30, preview: "Ammo stockpile", full_hint: "9mm rounds behind the police station locker room." },
    IntelEntry { id: "ziggy_sniper", npc_id: "ziggy", tier_min: "acquaintance", price: 100, preview: "Sniper nest", full_hint: "SVD with scope on the watchtower roof at B3." },
    IntelEntry { id: "ziggy_melee", npc_id: "ziggy", tier_min: "neutral", price: 20, preview: "Melee weapons", full_hint: "Katana in the dojo building near the coast." },
    IntelEntry { id: "doc_antibiotics", npc_id: "doc_vera", tier_min: "neutral", price: 40, preview: "Antibiotics", full_hint: "Full bottle in the vet clinic cabinet, north road." },
    IntelEntry { id: "doc_surgical", npc_id: "doc_vera", tier_min: "acquaintance", price: 80, preview: "Surgical kit", full_hint: "Hospital operating room, third floor, locked cabinet." },
    IntelEntry { id: "doc_bloodbags", npc_id: "doc_vera", tier_min: "neutral", price: 50, preview: "Blood bags", full_hint: "Red Cross tent near the airfield has 4 bags." },
    IntelEntry { id: "rust_vehicle", npc_id: "rust", tier_min: "acquaintance", price: 100, preview: "Working vehicle", full_hint: "Duster with keys in the barn at E5." },
    IntelEntry { id: "rust_sparkplugs", npc_id: "rust", tier_min: "neutral", price: 30, preview: "Spark plugs", full_hint: "Auto shop on the main road, back room toolbox." },
    IntelEntry { id: "rust_battery", npc_id: "rust", tier_min: "neutral", price: 40, preview: "Battery location", full_hint: "Construction site generator room, 2 fresh batteries." },
    IntelEntry { id: "rust_fuel", npc_id: "rust", tier_min: "acquaintance", price: 60, preview: "Fuel cache", full_hint: "Underground fuel depot at the old gas station." },
];

static WARNINGS: &[&str] = &[
    "[Ziggy] Heard gunshots at C4 center. Stay sharp.",
    "[Ziggy] Someone cleared out the ammo stash at B7 north-west.",
    "[Ziggy] Military patrol spotted at D3 top-right, near the airfield.",
    "[Ziggy] Two groups fighting at D4 center. The bridge. Steer clear.",
    "[Ziggy] Don't go near A5 bottom-left tonight. Trust me.",
    "[Ziggy] Saw fresh tracks at B3 south-east. Armed group, moving fast.",
    "[Doc Vera] Contaminated water at E6 center, near the river bend. Boil everything.",
    "[Doc Vera] Infection spreading at B2 north-east, the coastal camps. Stay away.",
    "[Doc Vera] Three people came in with bites from C3 top-middle, the hospital.",
    "[Doc Vera] Blood trails heading toward F4 bottom-right. Someone's hurt bad.",
    "[Doc Vera] Medical supply drop raided at A2 center. Nothing left.",
    "[Rust] Don't take the coast road past A3 middle-right. Saw someone mining it.",
    "[Rust] Bridge at D4 south-west is unstable. Take the detour through D5.",
    "[Rust] Someone stripped the fuel pump at E2 top-left. Walk or find another route.",
    "[Rust] Tire tracks near the junkyard at B5 bottom-middle. Someone's scavenging parts.",
    "[Rust] Heard an engine at C6 center. Working vehicle, someone's mobile.",
];

#[derive(Debug, Serialize, Deserialize, Default)]
struct ServicesState {
    intel_purchased: HashMap<String, Vec<String>>,
    bounties: Vec<BountyEntry>,
    last_warning: u64,
    warning_idx: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BountyEntry { target: String, amount: i64, placed_by: String, placer_name: String, ts: String }

fn load_svc() -> ServicesState { std::fs::read_to_string(SERVICES_PATH).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default() }
fn save_svc(st: &ServicesState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(j) = serde_json::to_string_pretty(st) { let t = format!("{}.tmp", SERVICES_PATH); if std::fs::write(&t, &j).is_ok() { let _ = std::fs::rename(&t, SERVICES_PATH); } }
}

fn tier_rank(t: &str) -> u8 { match t { "enemy" => 0, "neutral" => 1, "acquaintance" => 2, "friend" => 3, _ => 1 } }

fn get_tier(steam: &str, npc_id: &str) -> String {
    let path = PathBuf::from(REL_DIR).join(format!("{}.json", steam));
    let Ok(raw) = std::fs::read_to_string(&path) else { return "neutral".into() };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) else { return "neutral".into() };
    val["npcs"][npc_id]["tier"].as_str().unwrap_or("neutral").to_string()
}

fn npc_display(npc_id: &str) -> &'static str { match npc_id { "ziggy" => "Ziggy", "doc_vera" => "Doc Vera", "rust" => "Rust", _ => "NPC" } }

// Economy helpers - read/write economy.json directly
#[derive(Debug, Serialize, Deserialize, Default)]
struct EconState { players: HashMap<String, EconPlayer>, #[serde(default)] bounties: Vec<serde_json::Value> }
#[derive(Debug, Serialize, Deserialize)]
struct EconPlayer { name: String, balance: i64, #[serde(default)] last_daily: Option<String>, #[serde(default)] total_earned: i64, #[serde(default)] total_spent: i64 }

fn deduct_coins(steam: &str, player: &str, amount: i64) -> bool {
    let mut econ: EconState = std::fs::read_to_string(ECONOMY_PATH).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let e = econ.players.entry(steam.into()).or_insert_with(|| EconPlayer { name: player.into(), balance: 0, last_daily: None, total_earned: 0, total_spent: 0 });
    if e.balance < amount { return false; }
    e.balance -= amount; e.total_spent += amount;
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(j) = serde_json::to_string_pretty(&econ) { let t = format!("{}.tmp", ECONOMY_PATH); if std::fs::write(&t, &j).is_ok() { let _ = std::fs::rename(&t, ECONOMY_PATH); } }
    true
}

fn add_coins(steam: &str, player: &str, amount: i64) {
    let mut econ: EconState = std::fs::read_to_string(ECONOMY_PATH).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let e = econ.players.entry(steam.into()).or_insert_with(|| EconPlayer { name: player.into(), balance: 0, last_daily: None, total_earned: 0, total_spent: 0 });
    e.balance += amount; e.total_earned += amount;
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(j) = serde_json::to_string_pretty(&econ) { let t = format!("{}.tmp", ECONOMY_PATH); if std::fs::write(&t, &j).is_ok() { let _ = std::fs::rename(&t, ECONOMY_PATH); } }
}

fn get_balance(steam: &str) -> i64 {
    std::fs::read_to_string(ECONOMY_PATH).ok().and_then(|s| serde_json::from_str::<EconState>(&s).ok()).and_then(|e| e.players.get(steam).map(|p| p.balance)).unwrap_or(0)
}

fn now_secs() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() }

async fn pm(msg: &str, player: &str) {
    let p = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(p)).await.ok();
}

fn resolve_npc(arg: &str) -> Option<&'static str> {
    match arg.to_lowercase().as_str() { "ziggy" => Some("ziggy"), "vera" | "doc" | "doc_vera" => Some("doc_vera"), "rust" => Some("rust"), _ => None }
}

pub struct NpcServices {
    rate: Mutex<HashMap<String, Instant>>,
}

impl NpcServices {
    pub fn new() -> Self { Self { rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for NpcServices {
    fn name(&self) -> &'static str { "npc_services" }
    // event-driven (no commands(), no interval): a rotating NPC warning pops to each player ON LOGIN
    // (personal) instead of broadcasting to EVERYONE every 10 min (Joel: don't flood the server).

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event == "login" {
            let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if player.is_empty() { return Outcome::Ignored; }
            let mut st = load_svc();
            let msg = WARNINGS[st.warning_idx % WARNINGS.len()];
            st.warning_idx += 1;
            save_svc(&st);
            pipe_rpc::call("sendChatLineToPlayer", Some(serde_json::json!({ "message": msg, "playerName": player, "channel": "4" }))).await.ok();
            return Outcome::Handled;
        }
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }

        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if player.is_empty() { return Outcome::Ignored; }

        let rk = if steam.is_empty() { player.clone() } else { steam.clone() };
        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&rk) { if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; } }
            rate.insert(rk.clone(), now);
        }

        let parts: Vec<&str> = text.split_whitespace().collect();
        let cmd = parts[0].to_lowercase();

        match cmd.as_str() {
            "!intel" => {
                let npc_id = parts.get(1).and_then(|a| resolve_npc(a)).unwrap_or("ziggy");
                let tier = get_tier(&rk, npc_id);
                if tier == "enemy" {
                    pm(&format!("[{}] Get lost.", npc_display(npc_id)), &player).await;
                    return Outcome::Handled;
                }
                let svc = load_svc();
                let owned = svc.intel_purchased.get(&rk).cloned().unwrap_or_default();
                let entries: Vec<String> = INTEL.iter()
                    .filter(|e| e.npc_id == npc_id && tier_rank(&tier) >= tier_rank(e.tier_min))
                    .map(|e| { let s = if owned.contains(&e.id.to_string()) { " [owned]" } else { "" }; format!("  {}: {} - {}c{}", e.id, e.preview, e.price, s) })
                    .collect();
                if entries.is_empty() {
                    pm(&format!("[{}] Nothing for you. Build rep first.", npc_display(npc_id)), &player).await;
                } else {
                    pm(&format!("[{}] Available intel:", npc_display(npc_id)), &player).await;
                    for line in &entries { pm(line, &player).await; }
                    pm("Use !buyintel <id> to purchase.", &player).await;
                }
                Outcome::Handled
            }

            "!buyintel" => {
                if parts.len() < 2 { pm("Usage: !buyintel <id>", &player).await; return Outcome::Handled; }
                let Some(entry) = INTEL.iter().find(|e| e.id == parts[1]) else { pm("Unknown intel ID.", &player).await; return Outcome::Handled; };
                let tier = get_tier(&rk, entry.npc_id);
                if tier_rank(&tier) < tier_rank(entry.tier_min) {
                    pm(&format!("[{}] Need '{}' standing.", npc_display(entry.npc_id), entry.tier_min), &player).await;
                    return Outcome::Handled;
                }
                let mut svc = load_svc();
                let owned = svc.intel_purchased.entry(rk.clone()).or_default();
                if owned.contains(&entry.id.to_string()) {
                    pm(&format!("[{}] You know this: {}", npc_display(entry.npc_id), entry.full_hint), &player).await;
                    return Outcome::Handled;
                }
                if !deduct_coins(&rk, &player, entry.price) {
                    pm(&format!("[{}] Not enough coins. Need {}, have {}.", npc_display(entry.npc_id), entry.price, get_balance(&rk)), &player).await;
                    return Outcome::Handled;
                }
                owned.push(entry.id.to_string());
                save_svc(&svc);
                pm(&format!("[{}] -{}c. {}", npc_display(entry.npc_id), entry.price, entry.full_hint), &player).await;
                Outcome::Handled
            }

            "!bountyboard" => {
                let svc = load_svc();
                if svc.bounties.is_empty() {
                    pm("[Ziggy] Board's clean. Nobody's paying to see anyone dead. Yet.", &player).await;
                } else {
                    pm("[Ziggy] Active bounties:", &player).await;
                    for b in &svc.bounties { pm(&format!("  {} - {}c (by {})", b.target, b.amount, b.placer_name), &player).await; }
                }
                Outcome::Handled
            }

            "!placebounty" => {
                if parts.len() < 3 { pm("[Ziggy] Usage: !placebounty <player> <amount>", &player).await; return Outcome::Handled; }
                let amount: i64 = match parts[2].parse() { Ok(n) if n >= 50 => n, _ => { pm("[Ziggy] Minimum 50 coins.", &player).await; return Outcome::Handled; } };
                let cut = (amount as f64 * 0.10).ceil() as i64;
                if !deduct_coins(&rk, &player, amount + cut) {
                    pm(&format!("[Ziggy] Need {} coins ({}+{} cut). Have {}.", amount + cut, amount, cut, get_balance(&rk)), &player).await;
                    return Outcome::Handled;
                }
                let mut svc = load_svc();
                svc.bounties.push(BountyEntry { target: parts[1].into(), amount, placed_by: rk.clone(), placer_name: player.clone(), ts: now_secs().to_string() });
                save_svc(&svc);
                pm(&format!("[Ziggy] {} coins on {}'s head. Cut: {}.", amount, parts[1], cut), &player).await;
                Outcome::Handled
            }

            "!claimbounty" => {
                if parts.len() < 2 { pm("[Ziggy] Usage: !claimbounty <player>", &player).await; return Outcome::Handled; }
                let mut svc = load_svc();
                let pos = svc.bounties.iter().position(|b| b.target.eq_ignore_ascii_case(parts[1]));
                if let Some(idx) = pos {
                    let b = svc.bounties.remove(idx);
                    save_svc(&svc);
                    add_coins(&rk, &player, b.amount);
                    pm(&format!("[Ziggy] {} coins. Clean work. {}'s off the board.", b.amount, b.target), &player).await;
                } else {
                    pm(&format!("[Ziggy] No bounty on {}.", parts[1]), &player).await;
                }
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
