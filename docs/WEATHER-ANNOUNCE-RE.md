# Weather + Announce engine control — RE findings (2026-06-08, live session)

Both reached via the live engine bridge (`/engine/rpc` findInstancesByClass / readActorByPtr /
writeActorProperty / findFunctions / describeFunction) on build 23622834. Conclusion: the
*mechanisms* are identified, but wiring either into the bridge needs **new handler code + a DLL
rebuild** (not a property write) — they're a focused build session, not a live tweak.

## Weather — `#SetWeather` works, bridge `setWeather` doesn't stick (now understood)
- Controller: **`BP_WeatherController1_0_C`** (instance `BP_WeatherController1_2`). The bridge's
  current setWeather finds `WeatherController2` / this BP and writes a field at offset 0x10 — wrong.
- The live weather STATE is these writable floats (confirmed values, clear→storm via `#SetWeather 1`):
  `_rainIntensity` 0→1, `_windIntensity` 0.93→1, `_nimbostratusCoverage` 0→1 (rain clouds),
  `_cumulonimbusCoverage` 0.54→0, `_lightningRate` 0→0.1, `_fogDensity` 0.38→0.19.
- **But direct writes REVERT:** wrote `_rainIntensity=0` (offset 2728) → 7s later the tick forced it
  back to 1. So the controller tick recomputes these from a MASTER (the severity `#SetWeather` sets).
  No simple `_severity`/`_target*` property surfaced in the diff → the master is a function/internal.
- **To wire the bridge:** find the function `#SetWeather` calls (sets the master severity the tick
  reads) and call it via callObjectFunction, OR find the target field the tick reads and write THAT.
  Writing the rendered `_rainIntensity` etc. will never stick.

## Announce — `#Announce <string>` works; it's a multicast notification, not chat
- SCUM.log logs the command (`'<steam>:<name>' Command: 'Announce REVERSE!'`) but NOT the internal
  call; UE4SS.log shows nothing → `#Announce` does NOT use the chat path the bridge already hooks
  (broadcastChat), and the notification is multicast + transient (NotificationsManager dequeues it),
  so it can't be reversed from state-reads.
- Candidate functions (from findFunctions grep + describeFunction):
  - `NotificationsManager::NetMulticast_RequestNotification(Description: StructProperty)` — most
    likely what `#Announce` calls; needs the bridge to BUILD the notification Description struct.
  - `GameEventBase::Multicast_ShowEventNotification(Type: EnumProperty, auxString: StrProperty)` —
    takes a string directly, but it's game-event-tied + needs the right enum.
  - `GameEventBase::Multicast_PlayAnnouncementToAllParticipants(Sound: ObjectProperty)` — audio only.
- **To wire the bridge:** new handler that constructs the notification struct (or sets up a
  GameEventBase + enum) and calls the multicast — a struct-param call capability the bridge lacks today.

## Working alternatives in the meantime (no rebuild needed)
- Weather: `#SetWeather <0..1>` admin command works in-game now.
- Announce: the bridge already has `broadcastChat` (server chat line) + `sendHudMessage` (HUD) +
  `broadcastRaidBanner` (fixed raid text). Use those for PVP-zone callouts until the struct-call
  handler is built. The pvp_rotation announce loop already uses sendHudMessage + broadcastChat.

## Next-session task to finish
1. Add a callObjectFunction-with-struct capability to the bridge (build a UE struct from JSON, call).
2. Weather: resolve the `#SetWeather` master fn (sigscan / decompile the SetWeather admin cmd) → wrap.
3. Announce: build the notification Description struct → wrap NetMulticast_RequestNotification.
4. Rebuild TurdMODEngineBridge.dll → deploy-engine.ps1 → verify (ping + a test call).
