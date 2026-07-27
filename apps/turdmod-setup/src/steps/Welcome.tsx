// Step 1 — detect FIRST, then offer only what this machine can actually do.
//
// @ctx: the wizard used to ask "where does your server live?" before looking at
//   anything, which meant offering choices we already knew were wrong. Scan on
//   arrival, show what's here, and let the options fall out of that.

import { open as openUrl } from "@tauri-apps/plugin-shell";
import { useEffect, useState } from "react";
import { api, type HostKind, type UpdateReport } from "../lib/api";
import { useSetup } from "../lib/setup-state";

const HOSTS: Array<{ id: HostKind; title: string; desc: string }> = [
  {
    id: "local",
    title: "On this PC",
    desc: "The SCUM dedicated server runs on the same computer you're using right now. Everything works — this is the easiest setup.",
  },
  {
    id: "own-vps",
    title: "On my own server box",
    desc: "A VPS or dedicated machine you can log into (Remote Desktop or SSH) and install programs on. Everything works.",
  },
  {
    id: "rented-ftp",
    title: "Rented from a game host",
    desc: "You got a server from a hosting company and manage it through their website. You upload files with FTP. Limited — we'll show you exactly what you can still do.",
  },
  {
    id: "unknown",
    title: "I'm not sure",
    desc: "We'll look around and tell you what you've got.",
  },
];

export function Welcome({ onPickClient }: { onPickClient: () => void }) {
  const { state, set, next } = useSetup();
  const [scanning, setScanning] = useState(!state.detected);
  // Two phases in one step: what's here → then, if they chose the server, where it lives.
  const [phase, setPhase] = useState<"options" | "host">("options");
  const [update, setUpdate] = useState<UpdateReport | null>(null);

  useEffect(() => {
    // Update check runs regardless — it's cheap and never blocks anything.
    void api.checkForUpdate().then(setUpdate).catch(() => {});
    if (state.detected) return;
    void (async () => {
      try {
        const d = await api.detectInstalls();
        set({ detected: d, ...(d.server && !state.serverRoot ? { serverRoot: d.server } : {}) });
      } catch (e) {
        set({ lastError: String(e) });
      } finally {
        setScanning(false);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const found = state.detected;
  const hasServer = !!found?.server;
  const hasClient = !!found?.game;

  if (phase === "host") {
    return (
      <div className="pane">
        <h1>Where does your SCUM server live?</h1>
        <p className="lede">
          This decides what&apos;s actually possible — the engine has to run as a program on the same
          machine as the game server.
        </p>
        <div className="stack">
          {HOSTS.map((c) => (
            <button
              key={c.id}
              className={`choice${state.hostKind === c.id ? " selected" : ""}`}
              onClick={() => set({ hostKind: c.id })}
            >
              <div className="t">{c.title}</div>
              <div className="d">{c.desc}</div>
            </button>
          ))}
        </div>
        <div className="actions">
          <button className="btn ghost" onClick={() => setPhase("options")}>
            Back
          </button>
          <button className="btn primary" disabled={!state.hostKind} onClick={next}>
            Continue
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="pane">
      <h1>Let&apos;s get TurdMOD running.</h1>
      <p className="lede">
        TurdMOD adds live modding to SCUM — custom commands, events, spawning, 90+ mods — without
        needing an admin account logged into the game. First we look at what&apos;s on this PC.
      </p>

      {/* Only shout when there's genuinely something newer. "Unknown" stays
          quiet here — it's surfaced by the assistant if asked, not as alarm. */}
      {update?.state === "available" && (
        <div className="verdict warn">
          {update.summary}
          <div style={{ marginTop: 10 }}>
            <button className="btn small" onClick={() => void openUrl(update.download_url)}>
              Get the latest Server Pack
            </button>
          </div>
        </div>
      )}

      {scanning ? (
        <div className="row">
          <div className="spin" />
          <span>Looking for SCUM…</span>
        </div>
      ) : (
        <>
          <h2>What we found</h2>
          <div className="stack">
            <div className={`result ${hasServer ? "yes" : "no"}`}>
              <span className="mark">{hasServer ? "✓" : "✕"}</span>
              <div className="body">
                <div className="t">SCUM dedicated server</div>
                <div className="d">
                  {hasServer ? <span className="mono">{found!.server}</span> : "Not on this PC."}
                </div>
              </div>
            </div>
            <div className={`result ${hasClient ? "yes" : "no"}`}>
              <span className="mark">{hasClient ? "✓" : "✕"}</span>
              <div className="body">
                <div className="t">SCUM game</div>
                <div className="d">
                  {hasClient ? <span className="mono">{found!.game}</span> : "Not on this PC."}
                </div>
              </div>
            </div>
          </div>

          <h2>What would you like to do?</h2>
          <div className="stack">
            <button className="choice" onClick={() => setPhase("host")}>
              <div className="t">Set up the server engine</div>
              <div className="d">
                {hasServer
                  ? "Install TurdMOD into the server we found, so your server runs mods."
                  : "Your server isn't on this PC — we'll ask where it lives and tell you what it can run."}
              </div>
            </button>

            <button className="choice" onClick={onPickClient} disabled={!hasClient}>
              <div className="t">Set up the modded client{!hasClient && " — needs SCUM installed here"}</div>
              <div className="d">
                {hasClient
                  ? "Make a separate, moddable copy of your game. Your Steam install stays untouched, so you can still play official servers."
                  : "Install SCUM through Steam on this PC first — the modded copy is built from your own files."}
              </div>
            </button>
          </div>

          <div className="note" style={{ marginTop: 20 }}>
            You can do both, in any order — and remove any of it later from the sidebar.
          </div>
        </>
      )}
    </div>
  );
}
