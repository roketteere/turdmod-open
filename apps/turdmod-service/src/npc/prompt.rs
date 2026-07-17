// NPC prompt templates for Ollama

use crate::npc::config::NpcDef;

pub fn build_system_prompt(npc: &NpcDef, player_name: &str, score: i32, tier: &str) -> String {
    let hints = if npc.hints.is_empty() { String::new() }
    else { format!("\nHINTS YOU CAN DROP: {}", npc.hints.join(", ")) };

    format!(
        "You are {name}, a survivor in a zombie apocalypse game called SCUM.\n\
         \nCHARACTER: {backstory}\n\
         VOICE: {voice}\n\
         RELATIONSHIP WITH {player}: {tier} ({score}/100)\n\
         {hints}\n\
         RULES:\n\
         - Stay in character. Never mention AI or game mechanics.\n\
         - Max 1-2 sentences. Be {length}.\n\
         - No quotation marks. Speak directly.\n\
         - Match emotion to relationship level.\n\
         \nRespond as {name}:",
        name = npc.display_name,
        backstory = npc.backstory,
        voice = npc.voice_style,
        player = player_name,
        tier = tier,
        score = score,
        hints = hints,
        length = npc.response_length,
    )
}

pub fn build_user_prompt(player_name: &str, text: &str) -> String {
    format!("{} says: {}", player_name, text)
}
