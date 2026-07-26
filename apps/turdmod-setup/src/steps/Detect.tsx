// Step 2 — find the server folder. Auto-scan first (Steam registry + library
// folders); manual browse as the fallback, because plenty of people install
// the dedicated server outside Steam entirely.

import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { useSetup } from "../lib/setup-state";

export function Detect() {
  const { state, set, next, back } = useSetup();
  const [scanning, setScanning] = useState(false);
  const [pathOk, setPathOk] = useState<boolean | null>(null);
  const remote = state.hostKind === "rented-ftp" || state.hostKind === "own-vps";

  async function scan() {
    setScanning(true);
    try {
      const d = await api.detectInstalls();
      set({ detected: d, ...(d.server ? { serverRoot: d.server } : {}) });
      if (d.server) setPathOk(true);
    } catch (e) {
      set({ lastError: String(e) });
    } finally {
      setScanning(false);
    }
  }

  // Auto-scan on arrival for the local case — nobody should have to click
  // "search" before the app has even tried.
  useEffect(() => {
    if (state.hostKind === "local" || state.hostKind === "unknown") {
      if (!state.detected) void scan();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function browse() {
    const picked = await open({ directory: true, title: "Pick your SCUM server folder" });
    if (typeof picked !== "string") return;
    set({ serverRoot: picked });
    setPathOk(await api.validatePath(picked));
  }

  async function recheck(path: string) {
    set({ serverRoot: path });
    setPathOk(path ? await api.validatePath(path) : null);
  }

  return (
    <div className="pane">
      <h1>Where are the server files?</h1>
      <p className="lede">
        {remote
          ? "TurdMOD installs into your SCUM dedicated server folder. Point us at your local copy of the server files for now — the remote upload step comes after."
          : "We looked for a SCUM dedicated server install on this PC. Confirm we found the right one, or point us at it."}
      </p>

      {scanning && (
        <div className="row">
          <div className="spin" />
          <span className="note" style={{ border: "none", background: "none", padding: 0 }}>
            Searching Steam libraries…
          </span>
        </div>
      )}

      {!scanning && state.detected && (
        <>
          <div className="stack">
            <div className={`result ${state.detected.server ? "yes" : "no"}`}>
              <span className="mark">{state.detected.server ? "✓" : "✕"}</span>
              <div className="body">
                <div className="t">SCUM dedicated server</div>
                <div className="d">
                  {state.detected.server ? (
                    <span className="mono">{state.detected.server}</span>
                  ) : (
                    "Not found automatically — use Browse below."
                  )}
                </div>
              </div>
            </div>
            {state.detected.game && (
              <div className="result yes">
                <span className="mark">✓</span>
                <div className="body">
                  <div className="t">SCUM game (client)</div>
                  <div className="d">
                    <span className="mono">{state.detected.game}</span> — used later if you want the
                    modded client.
                  </div>
                </div>
              </div>
            )}
          </div>
          {!state.detected.server && state.detected.searched.length > 0 && (
            <>
              <h2>Where we looked</h2>
              <div className="note">
                {state.detected.searched.map((p) => (
                  <div key={p} className="mono">
                    {p}
                  </div>
                ))}
              </div>
            </>
          )}
        </>
      )}

      <h2>Server folder</h2>
      <div className="stack">
        <div className="row">
          <input
            className="input"
            style={{ flex: 1, minWidth: 260 }}
            placeholder="C:\SCUMServer"
            value={state.serverRoot}
            onChange={(e) => void recheck(e.target.value)}
          />
          <button className="btn" onClick={browse}>
            Browse…
          </button>
          <button className="btn ghost small" onClick={scan} disabled={scanning}>
            Search again
          </button>
        </div>
        {pathOk === false && state.serverRoot && (
          <div className="note">
            That folder doesn't contain <code>GameServer.exe</code>. Pick the folder that has it — usually
            named <code>SCUM Server</code> or <code>SCUMServer</code>.
          </div>
        )}
        {pathOk === true && (
          <div className="note">
            Looks right — <code>GameServer.exe</code> is in there.
          </div>
        )}
      </div>

      <div className="actions">
        <button className="btn ghost" onClick={back}>
          Back
        </button>
        <button className="btn primary" disabled={!state.serverRoot} onClick={next}>
          Continue
        </button>
      </div>
    </div>
  );
}
