//! DIMs character packs — the AUTHORED content (by the AI-CLI bridge, offline) that gives each
//! character its personality + large pools of contextual line variants + actions. Loaded from
//! C:\TurdMOD\npc\characters\*.json at startup. Runtime selection over these pools (+ slot-fill +
//! anti-repeat + memory) is what makes them feel alive without a per-line LLM call. @dep [[dims]].

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterPack {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub archetype: String,
    #[serde(default)]
    pub voice: String,
    /// "active" (in world) | "recurring" (comes and goes) | "retired" (gone, memory kept).
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub triggers: Vec<Trigger>,
}

fn default_status() -> String { "active".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trigger {
    /// Event kind that fires this: "chat" | "login" | "kill" | "death".
    pub on: String,
    /// For on=="chat": a lowercase substring the chat line must contain (e.g. "!rust", "parts").
    #[serde(default)]
    pub phrase: Option<String>,
    /// Anti-repeat bucket label (so variants in this situation don't repeat to the same player).
    pub situation: String,
    /// The variant pool. Each may contain slots: {player} {fact} {killer} {victim} {count}.
    pub lines: Vec<String>,
    /// Optional in-world action the character takes when this fires (spawn item, etc.).
    #[serde(default)]
    pub action: Option<Action>,
    /// 0..1 chance to fire when matched (lets characters be selective, not robotic).
    #[serde(default = "default_chance")]
    pub chance: f64,
    /// Min seconds between firings of THIS trigger for a given player.
    #[serde(default = "default_cooldown")]
    pub cooldown_s: u64,
    /// "broadcast" (global chat) | "private" (only the target player). Default broadcast.
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_chance() -> f64 { 1.0 }
fn default_cooldown() -> u64 { 45 }
fn default_scope() -> String { "broadcast".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    /// Bridge method or admin verb the director invokes (e.g. "spawnItem").
    pub command: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// One illustrative seed pack so the engine has content on first run. Real packs are authored by
/// the AI-CLI bridge into separate files; this just proves the shape + non-repetition + a memory
/// callback. NOTE: deliberately small — the authored pools will be far larger.
pub fn seed_rust() -> CharacterPack {
    CharacterPack {
        id: "rust".into(),
        name: "Rust".into(),
        archetype: "scavenger-mechanic".into(),
        voice: "rambling, paranoid, oddly insightful about machines".into(),
        status: "active".into(),
        triggers: vec![
            Trigger {
                on: "login".into(),
                phrase: None,
                situation: "greet".into(),
                lines: vec![
                    "[Rust] *wipes grease off a wrench* Oh. {player}. Thought the radio static was you.".into(),
                    "[Rust] {player}. Still alive. The machines told me you might be.".into(),
                    "[Rust] Back again, {player}? The junkyard missed your footsteps. I didn't.".into(),
                ],
                action: None,
                chance: 0.7,
                cooldown_s: 600,
                scope: "broadcast".into(),
            },
            Trigger {
                on: "death".into(),
                phrase: None,
                situation: "death_callout".into(),
                lines: vec![
                    "[Rust] Heard you went down, {player}. Machines don't die. Maybe take notes.".into(),
                    "[Rust] {player} dead again? I'm starting a tally on the workshop wall.".into(),
                ],
                action: None,
                chance: 0.8,
                cooldown_s: 120,
                scope: "broadcast".into(),
            },
            Trigger {
                on: "chat".into(),
                phrase: Some("parts".into()),
                situation: "parts_talk".into(),
                lines: vec![
                    "[Rust] Parts? *eyes narrow* Farm vehicles. Same guts as trucks. Nobody looks.".into(),
                    "[Rust] You want parts, {player}? Radiators, batteries, plugs. The junkyard remembers.".into(),
                ],
                action: None,
                chance: 1.0,
                cooldown_s: 60,
                scope: "broadcast".into(),
            },
        ],
    }
}
