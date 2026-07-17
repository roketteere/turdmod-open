import { useEffect, useRef, useState } from "react";
import {
  listServers,
  listMods,
  setEnabledMods,
  launchModded,
  pidAlive,
  joinProgress,
  type ServerDto,
  type ModDto,
} from "./api";
import {
  getCurrentWindow,
  currentMonitor,
  PhysicalPosition,
  PhysicalSize,
} from "@tauri-apps/api/window";
import { makeAnalyser, resumeCtx } from "./audio";
import { attachParty } from "./party";
import { attachIntroFx } from "./intro-fx";

type DisplayMode = "fullscreen" | "windowed";

// Apply a lobby display mode. Both modes are non-resizable and cover the whole
// screen (no desktop / taskbar showing).
//   - "fullscreen": real exclusive fullscreen.
//   - "windowed":   borderless covering the ENTIRE monitor (over the taskbar) —
//     NOT maximize(), which only fills the work area and leaves the taskbar.
async function applyDisplayMode(mode: DisplayMode) {
  const win = getCurrentWindow();
  try {
    if (mode === "fullscreen") {
      await win.setResizable(false);
      await win.setFullscreen(true);
    } else {
      await win.setFullscreen(false);
      // size/position can be rejected while non-resizable on some platforms;
      // allow resize for the move, then lock it back.
      await win.setResizable(true);
      const mon = await currentMonitor();
      if (mon) {
        await win.setPosition(new PhysicalPosition(mon.position.x, mon.position.y));
        await win.setSize(new PhysicalSize(mon.size.width, mon.size.height));
      } else {
        await win.maximize();
      }
      await win.setResizable(false);
    }
  } catch {
    /* window op not permitted / unavailable */
  }
}
import introTrack from "./assets/TurdMODIntro.mp3"; // "TurdMOD Intro" by TeKi (0:33 splash)
import lobbyTrack from "./assets/TurdMODLobby1.mp3"; // "TeKi's Rave Party" by TeKi (lobby loop)
import logoMark from "./assets/turdmod-logo.svg"; // official TurdMOD mark (poop + flies + neon glow)

// Frameless window chrome (decorations:false, like turdmod-manager).
function WindowChrome() {
  const win = getCurrentWindow();
  return (
    <div className="winchrome">
      <div className="winchrome-drag" />
      <button className="winbtn min" title="Minimize" onClick={() => win.minimize()}>
        ─
      </button>
      <button className="winbtn close" title="Close" onClick={() => win.close()}>
        ✕
      </button>
    </div>
  );
}

type Phase = "intro" | "menu";

export default function App() {
  const [phase, setPhase] = useState<Phase>("intro");
  const [muted, setMuted] = useState(false);

  const introAudio = useRef<HTMLAudioElement | null>(null);
  const themeAudio = useRef<HTMLAudioElement | null>(null);

  // SEPARATE analysers — intro splash + lobby each get their own, off their
  // own track, via the shared AudioContext (see audio.ts). Built lazily on a
  // user gesture (browser autoplay rule): intro on first interaction, lobby
  // on ENTER.
  const [introAnalyser, setIntroAnalyser] = useState<AnalyserNode | null>(null);
  const [lobbyAnalyser, setLobbyAnalyser] = useState<AnalyserNode | null>(null);

  const enter = () => {
    // Build the lobby analyser inside this click gesture (AudioContext rule).
    if (themeAudio.current && !lobbyAnalyser) {
      setLobbyAnalyser(makeAnalyser(themeAudio.current, 512));
    }
    if (introAudio.current) {
      introAudio.current.pause();
      introAudio.current.currentTime = 0;
    }
    // Default the lobby to fullscreen-borderless covering the whole monitor
    // (no desktop/taskbar showing). Settings can switch to real fullscreen.
    applyDisplayMode("windowed");
    setPhase("menu");
  };

  // Start the intro track looping. Also try to wire the intro analyser; if the
  // AudioContext is still suspended (no gesture yet), intro-fx falls back to a
  // calm ambient breathe until the first interaction.
  useEffect(() => {
    if (introAudio.current) introAudio.current.volume = 0.5; // intro music at half volume
    introAudio.current?.play().catch(() => {});
    if (introAudio.current && !introAnalyser) {
      setIntroAnalyser(makeAnalyser(introAudio.current, 256));
    }
    // a one-shot pointer handler resumes the context so the intro reacts even
    // before ENTER (autoplay policy needs a gesture to start audio data).
    const kick = () => resumeCtx();
    window.addEventListener("pointerdown", kick, { once: true });
    return () => window.removeEventListener("pointerdown", kick);
  }, [introAnalyser]);

  // Lobby track playback + mute.
  useEffect(() => {
    const t = themeAudio.current;
    if (!t) return;
    t.volume = 0.5; // lobby music at half volume
    t.muted = muted;
    if (phase === "menu") {
      t.play().catch(() => {});
      resumeCtx();
    }
  }, [phase, muted]);

  return (
    <div className={`app phase-${phase}`}>
      <WindowChrome />
      <audio ref={introAudio} src={introTrack} preload="auto" loop crossOrigin="anonymous" />
      <audio ref={themeAudio} src={lobbyTrack} preload="auto" loop crossOrigin="anonymous" />

      {phase === "intro" ? (
        <Intro analyser={introAnalyser} onEnter={enter} />
      ) : (
        <Menu
          analyser={lobbyAnalyser}
          muted={muted}
          onToggleMute={() => setMuted((m) => !m)}
          onLaunched={(pid) => {
            // Game's launching — stop the lobby music so it doesn't keep
            // looping over SCUM.
            const t = themeAudio.current;
            if (t) {
              t.pause();
              t.currentTime = 0;
            }
            // Watch the SCUM process: when the GAME exits, close the launcher.
            // Just disconnecting from a server leaves SCUM.exe running, so the
            // pid stays alive and we keep the launcher open.
            const poll = setInterval(async () => {
              try {
                if (!(await pidAlive(pid))) {
                  clearInterval(poll);
                  getCurrentWindow().close();
                }
              } catch {
                /* transient — keep watching */
              }
            }, 2000);
          }}
        />
      )}
    </div>
  );
}

// Calm beat-reactive lights behind the splash (its own analyser).
function IntroFx({ analyser }: { analyser: AnalyserNode | null }) {
  const ref = useRef<HTMLCanvasElement | null>(null);
  useEffect(() => {
    if (!ref.current) return;
    return attachIntroFx(ref.current, analyser);
  }, [analyser]);
  return <canvas ref={ref} className="introfx" />;
}

function Intro({ analyser, onEnter }: { analyser: AnalyserNode | null; onEnter: () => void }) {
  // Only the ENTER button advances — clicking the backdrop does nothing.
  return (
    <div className="intro">
      <IntroFx analyser={analyser} />
      <div className="intro-bg" />
      <div className="intro-content">
        <img className="brand-mark intro-mark" src={logoMark} alt="TurdMOD" />
        <div className="intro-logo">
          <span className="logo-turd">TURD</span>
          <span className="logo-mod">MOD</span>
        </div>
        <div className="intro-tag">MODDED CLIENT · BATTLEYE OFF</div>
        <button className="enter-btn" onClick={onEnter}>
          ▶ ENTER
        </button>
        <div className="intro-credit">♪ "TurdMOD Intro" by TeKi</div>
      </div>
      <div className="scanlines" />
    </div>
  );
}

// Lobby settings: display mode only — Fullscreen vs Fullscreen Windowed.
function SettingsMenu() {
  const [open, setOpen] = useState(false);
  const [mode, setMode] = useState<"fullscreen" | "windowed">("windowed");

  const apply = async (m: "fullscreen" | "windowed") => {
    setMode(m);
    setOpen(false);
    await applyDisplayMode(m);
  };

  return (
    <div className="settings">
      <button className="gear" title="Settings" onClick={() => setOpen((o) => !o)}>
        ⚙
      </button>
      {open && (
        <div className="settings-pop">
          <div className="settings-title">DISPLAY MODE</div>
          <button
            className={`opt${mode === "fullscreen" ? " active" : ""}`}
            onClick={() => apply("fullscreen")}
          >
            Fullscreen
          </button>
          <button
            className={`opt${mode === "windowed" ? " active" : ""}`}
            onClick={() => apply("windowed")}
          >
            Fullscreen Windowed
          </button>
        </div>
      )}
    </div>
  );
}

// Rotating TurdMOD loading quips — it's TurdMOD, lean into it.
const LOADING_QUIPS = [
  "Polishing the turds…",
  "Wiping the server clean…",
  "Loading premium fertilizer…",
  "Convincing BattlEye to look away…",
  "Spawning the prisoners…",
  "Flushing the lag…",
  "Composting the bugs…",
  "Warming up the porta-potty…",
  "Bribing the zombies to behave…",
  "Plunging into the world…",
  "Fertilizing the island…",
  "Rolling out the brown carpet…",
];

// Full-cover loading screen: a bouncing/spinning TurdMOD mascot + rotating
// funny quips, with the lightning beam as the REAL progress bar (pct from
// SCUM's log milestones). No default SCUM menu, no inject log.
function LoadingBeam({
  pct,
  label,
  serverName,
}: {
  pct: number;
  label: string;
  serverName: string;
}) {
  const [quip, setQuip] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setQuip((q) => (q + 1) % LOADING_QUIPS.length), 2600);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="loading">
      <div className="load-scene">
        {/* spinning poop mascot with a little orbiting fly */}
        <div className="poop">
          💩
          <span className="fly">🪰</span>
        </div>
        <div className="poop-shadow" />
      </div>

      <div className="loading-logo">
        <span className="logo-turd">TURD</span>
        <span className="logo-mod">MOD</span>
      </div>
      {serverName && <div className="loading-server">{serverName}</div>}

      <div className="quip">{LOADING_QUIPS[quip]}</div>

      <div className="beam-track">
        <div className="beam-fill" style={{ width: `${pct}%` }}>
          <span className="beam-head" />
        </div>
      </div>

      <div className="loading-row">
        <span className="loading-label">{label}</span>
        <span className="loading-pct">{Math.round(pct)}%</span>
      </div>
    </div>
  );
}

// Full-bleed beat-reactive party canvas behind the lobby UI.
function PartyCanvas({ analyser }: { analyser: AnalyserNode | null }) {
  const ref = useRef<HTMLCanvasElement | null>(null);
  useEffect(() => {
    if (!analyser || !ref.current) return;
    return attachParty(ref.current, analyser);
  }, [analyser]);
  return <canvas ref={ref} className="partyfx" />;
}

function Menu({
  analyser,
  muted,
  onToggleMute,
  onLaunched,
}: {
  analyser: AnalyserNode | null;
  muted: boolean;
  onToggleMute: () => void;
  onLaunched: (pid: number) => void;
}) {
  const [servers, setServers] = useState<ServerDto[]>([]);
  const [mods, setMods] = useState<ModDto[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [status, setStatus] = useState<{ kind: "ok" | "err"; msg: string } | null>(null);
  const [launching, setLaunching] = useState(false);
  // Loading-beam state: real progress read from SCUM's log after launch.
  const [loadPct, setLoadPct] = useState(0);
  const [loadLabel, setLoadLabel] = useState("");

  useEffect(() => {
    listServers()
      .then((s) => {
        setServers(s);
        if (s.length > 0) setSelected(s[0].id);
      })
      .catch((e) => setStatus({ kind: "err", msg: `servers: ${e}` }));
    listMods()
      .then(setMods)
      .catch((e) => setStatus({ kind: "err", msg: `mods: ${e}` }));
  }, []);

  const toggleMod = async (id: string) => {
    const next = mods.map((m) => (m.id === id ? { ...m, enabled: !m.enabled } : m));
    setMods(next);
    try {
      await setEnabledMods(next.filter((m) => m.enabled).map((m) => m.id));
    } catch (e) {
      setStatus({ kind: "err", msg: `save mods: ${e}` });
    }
  };

  const play = async () => {
    if (!selected) return;
    setLaunching(true);
    setStatus(null);
    setLoadPct(8);
    setLoadLabel("Spawning client");
    try {
      const res = await launchModded(selected);
      // Spawn + inject done — game is booting. Hand off to music-stop + pid
      // watch, then drive the beam off REAL SCUM-log milestones.
      onLaunched(res.pid);
      setLoadPct((p) => Math.max(p, 20));
      setLoadLabel("Injecting loader");

      const poll = setInterval(async () => {
        try {
          const jp = await joinProgress();
          if (jp.error) {
            clearInterval(poll);
            setLaunching(false);
            setStatus({ kind: "err", msg: jp.error });
            return;
          }
          // monotonic — the beam never goes backwards
          setLoadPct((p) => Math.max(p, jp.pct));
          setLoadLabel(jp.label);
          if (jp.done) {
            clearInterval(poll);
            // In world. SCUM has the screen now; the pid-watch handles close.
          }
        } catch {
          /* transient log read — keep polling */
        }
      }, 700);
    } catch (e) {
      setLaunching(false);
      setStatus({ kind: "err", msg: `${e}` });
    }
  };

  const sel = servers.find((s) => s.id === selected) || null;

  // While launching, take over the whole window with the beam loader — no
  // default menu / inject log shown. The modded launch path itself is untouched.
  if (launching) {
    return <LoadingBeam pct={loadPct} label={loadLabel} serverName={sel?.name ?? ""} />;
  }

  return (
    <div className="menu">
      <PartyCanvas analyser={analyser} />
      <header className="topbar">
        <div className="brand">
          <img className="brand-mark brand-mark-sm" src={logoMark} alt="" />
          <span className="logo-turd">TURD</span>
          <span className="logo-mod">MOD</span>
          <span className="brand-sub">LAUNCHER</span>
        </div>
        <div className="now-playing">
          <span className="np-text">♪ TeKi's Rave Party — TeKi</span>
          <button className="mute" onClick={onToggleMute} title="Toggle music">
            {muted ? "🔇" : "🔊"}
          </button>
          <SettingsMenu />
        </div>
      </header>

      <div className="stage">
        <section className="hero">
          <div className="hero-eyebrow">SELECTED SERVER</div>
          {sel ? (
            <>
              <h1 className="hero-name">{sel.name}</h1>
              <div className="hero-addr">
                {sel.ip}:{sel.port}
                {sel.region ? ` · ${sel.region}` : ""}
                <span className="hero-beoff">BATTLEYE OFF</span>
              </div>
              {sel.description && <p className="hero-desc">{sel.description}</p>}
            </>
          ) : (
            <h1 className="hero-name dim">No server selected</h1>
          )}

          <div className="hero-actions">
            <button className="play" disabled={!selected} onClick={play}>
              ▶  PLAY
            </button>
            {status && <span className={`status ${status.kind}`}>{status.msg}</span>}
          </div>

          <p className="vanilla-note">
            Official servers? Use Steam's <strong>Play</strong> — your Steam install
            is never modified and BattlEye stays on for vanilla SCUM.
          </p>
        </section>

        <aside className="side">
          <div className="side-block">
            <h2>Servers · BE OFF</h2>
            <div className="list">
              {servers.length === 0 && (
                <div className="empty">No servers available. Check your connection.</div>
              )}
              {servers.map((s) => (
                <button
                  key={s.id}
                  className={`server${selected === s.id ? " selected" : ""}`}
                  onClick={() => setSelected(s.id)}
                >
                  <span className="dot" />
                  <span className="server-meta">
                    <span className="name">{s.name}</span>
                    <span className="addr">
                      {s.ip}:{s.port}
                    </span>
                  </span>
                </button>
              ))}
            </div>
          </div>

          <div className="side-block">
            <h2>Mods</h2>
            <div className="list">
              {mods.length === 0 && <div className="empty">No mods installed.</div>}
              {mods.map((m) => (
                <label className="mod" key={m.id}>
                  <span className="meta">
                    <span className="name">{m.name}</span>
                    <span className="sub">
                      {m.version ? `v${m.version}` : m.id}
                      {m.author ? ` · ${m.author}` : ""}
                    </span>
                  </span>
                  <input
                    type="checkbox"
                    checked={m.enabled}
                    onChange={() => toggleMod(m.id)}
                  />
                </label>
              ))}
            </div>
          </div>
        </aside>
      </div>
    </div>
  );
}
