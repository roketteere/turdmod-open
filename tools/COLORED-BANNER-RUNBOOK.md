# Colored-instant banner — morning finish runbook

Goal: a center-screen banner that is **custom text + custom color + instant + zero-admin**.
Status as of 2026-06-19 ~01:30 (autonomous session): white-instant is LIVE; colored is one
client-side verification away. The blockers tonight were (1) no game client connected = the
notification multicast can't fire/capture, and (2) shouldn't deploy an unverified bridge on the
RE-UE4SS tree unwatched. Everything below is client-needed or build-risky, so do it with Joel +
his client online.

## What we know (verified)
- `#Announce` Data = `BasicNotificationDescriptionData`: FString text @64 (writable — `fireBanner`
  msgSet:true) but NO color field (renders fixed red/white).
- Notifications.json Data = `WarningNotificationDescription`: HAS color but text is a nested FText
  (fireBanner reports cap:0 on it). This is the color-capable type we must drive.
- `fireBanner` writes color @84 (BGRA). On a `#Announce` capture it had no effect (wrong type).
  UNTESTED on a Notifications.json capture — that's step 2 below.
- Notification multicast (and thus `captureNotification`) appears to need >=1 connected client.

## Step 1 — capture a ground-truth colored Data (client online)
```
# push a known red banner (distinct marker), arm capture, let the ~30s re-read fire it
scp tools/_gt_notifications.json admin@OVH:C:/SCUMServer/SCUM/Saved/Config/WindowsServer/Notifications.json
node tools/engine-rpc.mjs remote captureNotification
# poll getCapturedNotification until captured:true AND msg contains the marker
node tools/engine-rpc.mjs remote getCapturedNotification   # -> dataHex, msg[], deref[], refHex
```
Ground-truth file content:
`{"Notifications":[{"day":"Everyday","duration":"20","color":"255-0-0","wait":"0","message":"REDxGROUNDxTRUTHx987654321"}]}`

## Step 2 — find the REAL offsets from the captured dataHex (no guessing)
- **Color:** byte-search dataHex for red. FColor BGRA red = `0000ffff`; FLinearColor floats red =
  `0000803f` for R. Whatever offset holds it IS the color field. Compare to fireBanner's @84.
- **Text/FText:** the capture hook already walks d0+{8,16,24,32} x3 deref levels into `g_notif_msg`
  (see TurdMODEngineBridge.cpp ~line 1773). Whichever `msg[i]` == the marker tells you (offset,
  deref-depth) of the FText DisplayString. The FString that HOLDS it (Data ptr + ArrayMax) is one
  level up — that's what `fireBannerColored` overwrites.

## Step 3 — decide the handler
- IF color offset == 84 AND a Notifications.json capture lets fireBanner write text: maybe just
  seed capture from Notifications.json (not #Announce) and existing fireBanner works for BOTH.
  Test: capture red GT, then `fireBanner {text:"CUSTOM", r,g,b}` -> capture the re-fire -> verify
  dataHex has CUSTOM text + the color bytes. Joel confirms color on screen.
- ELSE build `fireBannerColored` (draft in the RE-agent report, session c798ee73): writes color at
  the REAL offset + navigates the FText (offsets from step 2) to overwrite the DisplayString buffer
  (only if new text <= captured ArrayMax). SEH-wrapped, ok_ptr-guarded, cap-checked.

## Step 4 — build + deploy bridge (see reference_bridge_build_deploy)
- Edit canonical `apps/turdmod-engine-bridge/src/TurdMODEngineBridge.cpp` (3 edits: decl, handler,
  table row). Copy-Item -> `C:\Development\RE-UE4SS\cppmods\TurdMODEngineBridge\src\dllmain.cpp`
  (⚠ check for divergence first — RE-UE4SS holds uncommitted work history; never `git restore` there).
- Build: vcvars64 + `ninja -C C:\Development\RE-UE4SS\build -f build-Game__Shipping__Win64.ninja TurdMODEngineBridge`
- **Back up the live DLL first**, then deploy via service API (`/server/stop` -> Copy-Item -> `/server/start`),
  verify SCUM boots healthy (>2GB) + `getOnlinePlayers` works. Roll back the backup on boot-kill.

## Step 5 — verify + ship
- Fire colored with custom text, Joel confirms on screen. Then wire the restart countdown +
  spa + events to the colored path (replace the `announce()`/Notifications.json calls).
- Restore quiet Notifications.json. Update reference_colored_banners memory with the cracked offsets.

## NetSerialize caveat
If color writes to the right offset still don't render: the struct likely has custom
`STRUCT_NetSerializeNative` (flag 0x400 @ StructFlags ~0xB0) — the wire color comes from the
serializer, not the in-memory FColor. Then chase the NetSerialize fn (CppStructOps @0xB8 -> vtable[14]).
But step 1-2 (matching a real Notifications.json Data byte-for-byte) sidesteps this: if our Data is
structurally identical to a known-rendering colored one, it serializes the same.
