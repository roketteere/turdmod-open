// Push turdmod game data to scummymap's internal API.
// Sends trader zone positions, custom POIs, and live overlays.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct ZonePush {
    zones: Vec<ZoneEntry>,
}

#[derive(Debug, Serialize)]
pub struct ZoneEntry {
    pub id: String,
    pub name: String,
    pub center: Center,
    #[serde(rename = "radiusCm")]
    pub radius_cm: u32,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Center {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Serialize)]
struct ZoneOverrides {
    #[serde(rename = "actorDiscordId")]
    actor_discord_id: String,
    #[serde(rename = "zoneOverrides")]
    zone_overrides: serde_json::Value,
}

pub async fn push_trader_zones(
    api_url: &str,
    api_token: &str,
    map_id: &str,
    zones: &[ZoneEntry],
) -> Result<usize> {
    if zones.is_empty() {
        return Ok(0);
    }

    // Build zoneOverrides object: { "custom-<id>": { center, radiusCm, kind, ... } }
    let mut overrides = serde_json::Map::new();
    for z in zones {
        let key = format!("custom-{}", z.id);
        let mut val = serde_json::Map::new();
        val.insert("center".into(), serde_json::json!({ "x": z.center.x, "y": z.center.y }));
        val.insert("radiusCm".into(), serde_json::json!(z.radius_cm));
        val.insert("kind".into(), serde_json::json!(z.kind));
        val.insert("name".into(), serde_json::json!(z.name));
        if let Some(ref f) = z.faction {
            val.insert("faction".into(), serde_json::json!(f));
        }
        if let Some(ref c) = z.color {
            val.insert("color".into(), serde_json::json!(c));
        }
        overrides.insert(key, serde_json::Value::Object(val));
    }

    let body = ZoneOverrides {
        actor_discord_id: "turdmod-service".into(),
        zone_overrides: serde_json::Value::Object(overrides),
    };

    let url = format!("{}/maps/{}", api_url.trim_end_matches('/'), map_id);
    let client = reqwest::Client::new();
    let resp = client
        .patch(&url)
        .header("X-Internal-Token", api_token)
        .json(&body)
        .send()
        .await
        .context("scummymap PATCH maps")?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("scummymap PATCH failed ({}): {}", status, text);
    }

    tracing::info!("pushed {} trader zones to scummymap map={}", zones.len(), map_id);
    Ok(zones.len())
}

pub fn entities_to_zones(entities: &[crate::scumdb::EntityRow], radius_cm: u32) -> Vec<ZoneEntry> {
    entities
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let faction = if e.class.contains("United") {
                Some("United".into())
            } else if e.class.contains("CMC") {
                Some("CMC".into())
            } else if e.class.contains("Mafia") {
                Some("Mafia".into())
            } else {
                None
            };
            let color = faction.as_deref().map(|f| match f {
                "United" => "#3b82f6",
                "CMC" => "#22c55e",
                "Mafia" => "#a855f7",
                _ => "#6b7280",
            }.into());
            let name = format_trader_name(&e.class, i);
            ZoneEntry {
                id: format!("trader-db-{}", e.id),
                name,
                center: Center { x: e.x, y: e.y },
                radius_cm,
                kind: "safe".into(),
                faction,
                color,
            }
        })
        .collect()
}

fn format_trader_name(class: &str, idx: usize) -> String {
    // Extract a readable name from class like "BP_Trader_United_C" or "SedentaryNPC_Mechanic_C"
    let stripped = class
        .trim_start_matches("BP_")
        .trim_end_matches("_C")
        .replace('_', " ");
    if stripped.is_empty() {
        format!("Trader #{}", idx + 1)
    } else {
        stripped
    }
}
