# server_hooks.rs — Design Reference (Sprint 1 Part C)

**Status:** Design / research document. Not yet implemented in `apps/turdmod-server-loader/src/server_hooks.rs` (currently a stub).

**Why deferred:** Part C's design uses typed `EngineApi` methods + a typed `EngineEvent` enum, while Part B's `admin_api.rs` ships a JSON-RPC dispatch surface (`Result<Json, EngineError>` per method, string-named events via `EventBroadcaster::emit`). Reconciling the two requires either (a) a Json→typed adapter layer in `server_hooks` or (b) refactoring `admin_api.rs` to expose typed methods that get serialised at the wire boundary. **All AOB patterns and UE4 field offsets in the Part C draft are `TODO(stage2)` placeholders** — empirical validation against a live `SCUMServer.exe` (via `tools/engine-validation/`) must happen before the hook layer can compile against real targets.

This document preserves the Part C design so Sprint 2 can pick it up directly.

---

## Module structure (proposed)

```
mod ue4_types     — Opaque UE4 type stubs (UWorld, APawn, APlayerController, FString, FVector, FLinearColor)
mod ue4_offsets   — Field offset constants (all TODO(stage2))
mod signatures    — SIGNATURES const struct (10 named AOB patterns)
mod hooks         — extern "C" hook callbacks (tick, chat, login, logout, death)
mod calls         — Outbound function pointer invocations
pub struct ScumEngineApi  — implements EngineApi
pub fn install(broadcaster: Arc<EventBroadcaster>) -> Result<Arc<ScumEngineApi>, InstallError>
```

## Key architectural decisions

### Game-thread marshalling

UE4 actor mutations (teleport, spawn, broadcast) MUST happen on the game thread. Inbound RPC calls land on Tokio worker threads and need to be marshalled.

**Design:** `crossbeam_channel::unbounded::<Box<dyn FnOnce() + Send>>()` queue. `tick_hook` (the `UWorld::Tick` detour) drains the queue every frame before forwarding to the original tick. This guarantees mutations happen in the same thread context UE4 itself uses for world simulation.

### Sig-scan resolution

Every hook + outbound call has a corresponding `OnceLock<usize>` cached at `install` time. Mis-scans log an error and leave the cell unset; methods that depend on a missing pointer return `EngineError::HookFailed { msg: "<reason>" }` rather than panicking. **Engine boots in degraded mode** — partial capability is more useful to operators than a hard crash.

### Hook installation via `retour::GenericDetour`

Same crate as `apps/turdmod-loader/decorators/`. Each detour is `Box::leak`ed for a `'static` lifetime, then stored in a `OnceLock<&'static GenericDetour<FnType>>` so the hook callback can call `.call()` for the trampoline.

### EventBroadcaster lifecycle

`BROADCASTER: OnceLock<Arc<EventBroadcaster>>` is set during `install()` before any detour is `enable()`d. Hook callbacks read it via `BROADCASTER.get()` and silently drop events if the cell is empty (defensive against init-order races).

## Signature table (all placeholders)

| Name | What it points to | rip_offset |
|---|---|---|
| `gworld` | `GWorld` global (UWorld**) | Some(3) |
| `uworld_tick` | `UWorld::Tick(float, ELevelTick)` function entry | None |
| `chat_receive` | Chat-receive callback (likely `ServerSay` or SCUM override) | None |
| `player_login` | `AGameModeBase::PostLogin(APlayerController*)` | None |
| `player_logout` | `AGameModeBase::Logout(AController*)` | None |
| `player_death` | SCUM `Die(...)` or equivalent | None |
| `client_message` | `APlayerController::ClientMessage(FString&, FName, float)` | None |
| `teleport_to` | `APawn::TeleportTo(FVector&, FRotator&, bool, bool)` | None |
| `world_spawn_actor` | `UWorld::SpawnActor(UClass*, FVector*, FRotator*, FActorSpawnParameters*)` | None |
| `world_broadcast` | World-wide chat broadcast (likely `BroadcastChatMessage`) | None |

## UE4 field offsets (all TODO(stage2))

| Constant | Type / Path | Placeholder Value |
|---|---|---|
| `UWORLD_GAME_INSTANCE` | UWorld::OwningGameInstance | 0x1C0 |
| `UWORLD_NET_DRIVER` | UWorld::NetDriver | 0x278 |
| `NETDRIVER_CLIENT_CONNECTIONS` | UNetDriver::ClientConnections | 0xE8 |
| `NETCONN_PLAYER_CONTROLLER` | UNetConnection::PlayerController | 0x88 |
| `APC_PLAYER_STATE` | APlayerController::PlayerState | 0x2E8 |
| `APC_PAWN` | AController::Pawn | 0x318 |
| `APS_STEAM_ID` | APlayerState::SteamID (SCUM-specific) | 0x508 |
| `APS_PLAYER_NAME` | APlayerState::PlayerName | 0x3E8 |
| `APAWN_ROOT_LOCATION` | APawn root component world location | 0x280 |
| `APAWN_CONTROLLER` | APawn::Controller | 0x2C8 |

## EngineApi → JSON-RPC adaptation strategy

Part B's trait is `async fn broadcast_chat(&self, params: Json) -> Result<Json, EngineError>`. Part C's design is `fn broadcast_chat(&self, text: String, color: Option<[f32; 4]>) -> Result<(), EngineError>`.

**Adapter pattern for Sprint 2:**

```rust
#[async_trait]
impl admin_api::EngineApi for ScumEngineApi {
    async fn broadcast_chat(&self, params: Json) -> Result<Json, admin_api::EngineError> {
        let text = params.get("text").and_then(Json::as_str)
            .ok_or_else(|| admin_api::EngineError::InvalidParams { msg: "missing text".into() })?
            .to_string();
        let color = params.get("color")
            .and_then(|v| serde_json::from_value::<[f32; 4]>(v.clone()).ok());
        self.broadcast_chat_typed(text, color)
            .map_err(map_internal_error)?;
        Ok(serde_json::json!({ "ok": true }))
    }
    // ... etc
}
```

**Event emission adaptation:**

```rust
// Inside hook_chat_receive:
if let Some(bc) = BROADCASTER.get() {
    bc.emit("chat", serde_json::json!({
        "steamId": steam_id.to_string(),
        "channel": channel,
        "text": message,
        "ts": chrono::Utc::now().to_rfc3339(),
    }));
}
```

## Stage 2 deliverables required before this can ship

1. **Real AOB patterns** for every `SIGNATURES` entry (run `tools/engine-validation/sigscan_transfer.py` against `SCUMServer.exe` after deriving real bytes via IDA / Binary Ninja / patternsleuth).
2. **Verified UE4 field offsets** (run `Dumper-7` against a running SCUMServer.exe, cross-reference against UE4 4.27 source).
3. **Real FString layout offset** (verify `+8` for `len` field on Win64 — likely correct, but SCUM's fork may add padding).
4. **Chat-receive function identity** — capture a live callstack when a player sends chat to identify the actual receiving function (`ServerSay` vs `BroadcastChatMessage` vs SCUM override).
5. **Headshot damage type UClass name** — needed for `is_headshot_damage_type()` in the death hook.

## Dependencies to add to `apps/turdmod-server-loader/Cargo.toml` when Sprint 2 lands

```toml
retour = { version = "0.3", features = ["static-detour"] }
crossbeam-channel = "0.5"
thiserror = "1"
```

## Full draft source

The complete ~700-LOC Part C draft (with all module bodies, hook callbacks, helpers, tests) is preserved in the conversation transcript. Sprint 2 should re-derive it with:
- Wire-protocol adaptation per the adapter pattern above
- Real AOB bytes from Stage 2
- Live-tested offsets

Until then, `apps/turdmod-server-loader/src/server_hooks.rs` remains a 1-line stub: `pub fn install() { logging::log("[server_hooks] stub: not yet implemented"); }`.
