// Auction house - player-to-player item listings with bidding.
// !sell <item> <price> / !buy <id> / !bid <id> <amount> / !market. Expire after 1hr (30s tick).

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const AUCTION_DURATION: Duration = Duration::from_secs(3600);
const EXPIRE_TICK: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct Listing {
    id: u32,
    seller: String,
    seller_steam: String,
    item_name: String,
    price: i64,
    highest_bid: i64,
    highest_bidder: Option<String>,
    highest_bidder_steam: Option<String>,
    created: Instant,
}

struct AuctionState { listings: Vec<Listing>, next_id: u32 }
impl AuctionState { fn new() -> Self { Self { listings: Vec::new(), next_id: 1 } } }

fn credit(steam: &str, amount: i64) {
    let Ok(data) = std::fs::read_to_string(ECON_PATH) else { return };
    let Ok(mut state) = serde_json::from_str::<serde_json::Value>(&data) else { return };
    let bal = state.get("players").and_then(|p| p.get(steam)).and_then(|p| p.get("balance")).and_then(|b| b.as_i64()).unwrap_or(0);
    state["players"][steam]["balance"] = serde_json::json!(bal + amount);
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let tmp = format!("{}.tmp", ECON_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, ECON_PATH); }
    }
}

fn debit(steam: &str, amount: i64) -> bool {
    let Ok(data) = std::fs::read_to_string(ECON_PATH) else { return false };
    let Ok(mut state) = serde_json::from_str::<serde_json::Value>(&data) else { return false };
    let bal = state.get("players").and_then(|p| p.get(steam)).and_then(|p| p.get("balance")).and_then(|b| b.as_i64()).unwrap_or(0);
    if bal < amount { return false; }
    state["players"][steam]["balance"] = serde_json::json!(bal - amount);
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let tmp = format!("{}.tmp", ECON_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, ECON_PATH); }
    }
    true
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast_msg(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

pub struct Auction { state: Mutex<AuctionState>, rate: Mutex<HashMap<String, Instant>> }
impl Auction {
    pub fn new() -> Self { Self { state: Mutex::new(AuctionState::new()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Auction {
    fn name(&self) -> &'static str { "auction" }
    fn commands(&self) -> &'static [&'static str] { &["!sell", "!buy", "!bid", "!market"] }
    fn interval(&self) -> Option<Duration> { Some(EXPIRE_TICK) }

    async fn tick(&self, _ctx: &ModCtx) {
        let expired: Vec<Listing> = {
            let mut st = self.state.lock().await;
            let expired: Vec<Listing> = st.listings.iter().filter(|l| l.created.elapsed() > AUCTION_DURATION).cloned().collect();
            st.listings.retain(|l| l.created.elapsed() <= AUCTION_DURATION);
            expired
        };
        for l in &expired {
            if l.highest_bidder.is_some() && l.highest_bidder_steam.is_some() {
                credit(&l.seller_steam, l.highest_bid);
                broadcast_msg(&format!("[Auction] '{}' sold to {} for {} coins!", l.item_name, l.highest_bidder.as_deref().unwrap_or(""), l.highest_bid)).await;
            }
        }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let parts: Vec<&str> = text.split_whitespace().collect();
        let cmd = parts[0].to_lowercase();
        if !matches!(cmd.as_str(), "!sell" | "!buy" | "!bid" | "!market") { return Outcome::Ignored; }

        let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&rate_key) { if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; } }
            rate.insert(rate_key.clone(), now);
        }

        match cmd.as_str() {
            "!sell" => {
                if parts.len() < 3 { reply("[Auction] Usage: !sell <item_name> <price>", &player).await; return Outcome::Handled; }
                let item = parts[1].to_string();
                let price: i64 = parts.last().and_then(|s| s.parse().ok()).unwrap_or(0);
                if price <= 0 { reply("[Auction] Price must be positive.", &player).await; return Outcome::Handled; }
                let (msg, id) = {
                    let mut st = self.state.lock().await;
                    if st.listings.iter().filter(|l| l.seller_steam == steam).count() >= 3 {
                        (Some("[Auction] Max 3 active listings.".to_string()), 0u32)
                    } else {
                        let id = st.next_id; st.next_id += 1;
                        st.listings.push(Listing { id, seller: player.clone(), seller_steam: steam.clone(), item_name: item.clone(), price, highest_bid: 0, highest_bidder: None, highest_bidder_steam: None, created: Instant::now() });
                        (None, id)
                    }
                };
                if let Some(m) = msg { reply(&m, &player).await; }
                else { broadcast_msg(&format!("[Auction] #{} - {} selling '{}' for {} coins!", id, player, item, price)).await; }
                Outcome::Handled
            }
            "!buy" => {
                let id: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                let listing_info = {
                    let st = self.state.lock().await;
                    st.listings.iter().find(|l| l.id == id).map(|l| (l.seller.clone(), l.seller_steam.clone(), l.item_name.clone(), l.price, l.seller_steam == steam))
                };
                let Some((seller, seller_steam, item, price, is_own)) = listing_info else { reply("[Auction] Listing not found.", &player).await; return Outcome::Handled; };
                if is_own { reply("[Auction] Can't buy your own listing.", &player).await; return Outcome::Handled; }
                if !debit(&steam, price) { reply("[Auction] Not enough coins.", &player).await; return Outcome::Handled; }
                { let mut st = self.state.lock().await; st.listings.retain(|l| l.id != id); }
                credit(&seller_steam, price);
                pipe_rpc::call("placeItemInInventory", Some(serde_json::json!({ "playerName": player, "className": item }))).await.ok();
                broadcast_msg(&format!("[Auction] {} bought '{}' from {} for {} coins!", player, item, seller, price)).await;
                Outcome::Handled
            }
            "!bid" => {
                if parts.len() < 3 { reply("[Auction] Usage: !bid <id> <amount>", &player).await; return Outcome::Handled; }
                let id: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                let amount: i64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
                let msg = {
                    let mut st = self.state.lock().await;
                    match st.listings.iter_mut().find(|l| l.id == id) {
                        None => Some(("[Auction] Listing not found.".to_string(), true)),
                        Some(listing) if amount <= listing.highest_bid => Some((format!("[Auction] Bid must exceed current high: {}", listing.highest_bid), true)),
                        Some(listing) => {
                            listing.highest_bid = amount;
                            listing.highest_bidder = Some(player.clone());
                            listing.highest_bidder_steam = Some(steam.clone());
                            Some((format!("[Auction] #{} - {} bid {} on '{}'!", id, player, amount, listing.item_name), false))
                        }
                    }
                };
                if let Some((m, is_reply)) = msg { if is_reply { reply(&m, &player).await; } else { broadcast_msg(&m).await; } }
                Outcome::Handled
            }
            "!market" => {
                let lines: Vec<String> = {
                    let st = self.state.lock().await;
                    if st.listings.is_empty() { vec!["[Auction] No active listings.".to_string()] }
                    else {
                        st.listings.iter().take(10).map(|l| {
                            let remaining = AUCTION_DURATION.checked_sub(l.created.elapsed()).map(|d| d.as_secs() / 60).unwrap_or(0);
                            let bid_info = if l.highest_bid > 0 { format!(" (bid: {})", l.highest_bid) } else { String::new() };
                            format!("[Auction] #{} - '{}' by {} - {}c{} ({}min left)", l.id, l.item_name, l.seller, l.price, bid_info, remaining)
                        }).collect()
                    }
                };
                for line in &lines { reply(line, &player).await; }
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
