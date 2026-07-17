# Car Radio — jam Apple Music over SCUM proximity voice

Goal: stream Apple Music (browser, music.apple.com) so nearby players hear it
out of your character/car, and you can still talk over it. Uses SCUM's own
positional proximity voice — no game audio-streaming mod (UE can't replicate
arbitrary streamed PCM; voice chat is the only real transport). BattlEye is
off on our servers, so routing your audio into the mic is fine.

## Why this design
- **Transport = SCUM proximity VOIP.** It's already positional + distance-attenuated,
  so the music feels diegetic (comes from you / the car, fades with range).
- **Mixer = VoiceMeeter Banana.** Blends real mic + Apple Music into ONE virtual
  mic that SCUM reads. Lets you DJ and talk simultaneously.
- **@inv NEVER set "Voicemeeter Input" as the Windows DEFAULT playback device.**
  If you do, ALL system sound (incl. the game) pours into the music strip →
  feedback loop + game audio transmitted over voice. Route ONLY the browser
  per-app.

## One-time install
```
winget install VB-Audio.Voicemeeter.Banana --accept-package-agreements --accept-source-agreements
```
Reboot after install (the audio driver needs it).

## Routing (after reboot)

### 1. Windows Sound
- Settings → System → Sound. Keep **default output = your headphones**
  (the USB Audio Device you actually wear). Do NOT change the default.
- Settings → System → Sound → **Volume mixer** (or "App volume & device
  preferences"): find your **browser** → set its **Output** to
  **"Voicemeeter Input (VB-Audio VoiceMeeter VAIO)"**.
  → Only Apple Music now feeds the cable; everything else stays on headphones.

### 2. VoiceMeeter Banana
- **Hardware Input 1** (top-left): click the name → **WDM** → select your real
  **microphone**. On its strip, enable bus **B1** (and leave A1 OFF to avoid
  hearing yourself).
- **"Voicemeeter Input"** strip (1st virtual input, top-right area — this is
  where the browser audio lands): enable **A1** (so YOU hear the music) **and
  B1** (so it transmits).
- **Hardware Out A1** (top-right): click → select your **headphones** (same USB
  Audio Device as the Windows default).
- Result: B1 = mic + music mixed → that's the virtual mic SCUM reads.

### 3. SCUM
- Settings → Audio/Voice → **Voice Input Device** =
  **"Voicemeeter Out B1 (VB-Audio VoiceMeeter VAIO)"**.
- Set voice to **open mic / voice-activation, always-on** (not push-to-talk) so
  music streams continuously. Lower the activation threshold so quiet passages
  still transmit. If SCUM forces PTT, bind a toggle and leave it on.

## Test
1. Play a song in the browser. You should hear it (A1 routing).
2. VoiceMeeter B1 meter should bounce with the music.
3. In-game, a second player in proximity should hear the song from your
   position. Talk — your voice mixes in over the track.

## Gotchas
- **No sound in-game but you hear it locally** → SCUM voice input isn't set to
  "Voicemeeter Out B1", or voice is PTT-only (hold the key / set open-mic).
- **Players hear game audio / echo** → you set VoiceMeeter as the Windows
  default. Revert default to headphones; route ONLY the browser per-app.
- **Robotic/choppy music** → SCUM voice codec is low-bitrate (it's built for
  speech). Music will sound lo-fi/radio-ish by nature — that's the medium, not
  a bug. Leaning into the "AM radio" vibe is the move.
- **You can't hear yourself** → intended (mic strip A1 off). Enable mic→A1 only
  if you want monitoring.
