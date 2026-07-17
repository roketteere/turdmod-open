// NPC registry — loads NPC definitions from C:\TurdMOD\npc\npcs.json

use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

const NPC_PATH: &str = r"C:\TurdMOD\npc\npcs.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcDef {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub archetype: String,
    pub backstory: String,
    #[serde(default)]
    pub mood_default: String,
    pub trigger_phrases: Vec<String>,
    pub voice_style: String,
    #[serde(default = "default_short")]
    pub response_length: String,
    #[serde(default)]
    pub greet_on_enter: bool,
    #[serde(default)]
    pub greet_template: Option<String>,
    #[serde(default)]
    pub hints: Vec<String>,
    #[serde(default)]
    pub fallback_response: Option<String>,
}

fn default_short() -> String { "short".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcConfig {
    pub npcs: Vec<NpcDef>,
}

impl NpcConfig {
    pub fn load() -> Self {
        let path = PathBuf::from(NPC_PATH);
        if !path.exists() {
            tracing::info!("npc: npcs.json not found — writing defaults");
            let cfg = Self::default_config();
            let _ = fs::create_dir_all(path.parent().unwrap());
            if let Ok(json) = serde_json::to_string_pretty(&cfg) {
                let _ = fs::write(&path, json);
            }
            return cfg;
        }
        match fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                tracing::warn!("npc: bad npcs.json ({}), using defaults", e);
                Self::default_config()
            }),
            Err(e) => {
                tracing::warn!("npc: can't read npcs.json ({})", e);
                Self::default_config()
            }
        }
    }

    pub fn find_by_trigger<'a>(&'a self, text: &str) -> Option<&'a NpcDef> {
        let lower = text.to_lowercase();
        self.npcs.iter().find(|npc| {
            npc.trigger_phrases.iter().any(|p| lower.contains(p.as_str()))
        })
    }

    fn default_config() -> Self {
        Self {
            npcs: vec![
                NpcDef {
                    id: "ziggy".into(),
                    display_name: "Ziggy".into(),
                    archetype: "trader".into(),
                    backstory: "Former arms dealer turned survival merchant. Suspicious of strangers, loyal to regulars.".into(),
                    mood_default: "guarded".into(),
                    trigger_phrases: vec!["!ziggy".into(), "ziggy".into(), "trader".into(), "shop".into(), "buy".into(), "sell".into()],
                    voice_style: "clipped, street-smart, gallows humor".into(),
                    response_length: "short".into(),
                    greet_on_enter: true,
                    greet_template: Some("[Ziggy] *counts his bullets* Another one. What do you need?".into()),
                    hints: vec![
                        "Ammo cache east of the lumber mill. You didn't hear that from me.".into(),
                        "Police stations — check the evidence lockers.".into(),
                    ],
                    fallback_response: Some("[Ziggy] *shrugs and goes back to work*".into()),
                },
                NpcDef {
                    id: "doc_vera".into(),
                    display_name: "Doc Vera".into(),
                    archetype: "medic".into(),
                    backstory: "ER surgeon barricaded in a farmhouse. Patches up survivors but keeps score.".into(),
                    mood_default: "exhausted".into(),
                    trigger_phrases: vec!["!doc".into(), "!vera".into(), "vera".into(), "doc".into(), "doctor".into(), "heal".into(), "medicine".into()],
                    voice_style: "clinical, exhausted, dry dark wit".into(),
                    response_length: "medium".into(),
                    greet_on_enter: true,
                    greet_template: Some("[Doc Vera] *doesn't look up* If you're bleeding, get in line.".into()),
                    hints: vec![
                        "Vet supply north of here has antibiotics.".into(),
                        "Tourniquets first. Always.".into(),
                    ],
                    fallback_response: Some("[Doc Vera] I'm busy. Come back later.".into()),
                },
                NpcDef {
                    id: "rust".into(),
                    display_name: "Rust".into(),
                    archetype: "scavenger".into(),
                    backstory: "Mechanic who survived in a junkyard. Names his wrenches. Knows where every car part is.".into(),
                    mood_default: "paranoid".into(),
                    trigger_phrases: vec!["!rust".into(), "rust".into(), "scavenger".into(), "junk".into(), "parts".into(), "mechanic".into(), "car".into(), "vehicle".into()],
                    voice_style: "rambling, paranoid, oddly insightful about machines".into(),
                    response_length: "medium".into(),
                    greet_on_enter: true,
                    greet_template: Some("[Rust] *talking to a wrench* Oh. A person. Haven't seen one in a while.".into()),
                    hints: vec![
                        "Junkyard. Radiators, batteries, spark plugs. Obviously.".into(),
                        "Farm vehicles have the same parts as trucks. People don't look.".into(),
                    ],
                    fallback_response: Some("[Rust] *goes back to tinkering*".into()),
                },
            ],
        }
    }
}
