// Step 6 — prove it works, or say precisely what's wrong.
//
// @dep: verify.rs — checks are dependency-ordered and report "skipped" rather
//       than a false green when an upstream check failed.

import { open as openUrl } from "@tauri-apps/plugin-shell";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { useSetup } from "../lib/setup-state";

export function Verify() {
  const { state, set, back } = useSetup();
  const [running, setRunning] = useState(false);

  async function check() {
    setRunning(true);
    try {
      const rep = await api.verify(state.port, state.token, state.serverRoot);
      set({
        verifyReport: rep,
        lastError: rep.all_ok ? "" : rep.checks.filter((c) => !c.ok).map((c) => c.detail).join("; "),
      });
    } catch (e) {
      set({ lastError: String(e) });
    } finally {
      setRunning(false);
    }
  }

  useEffect(() => {
    if (!state.verifyReport) void check();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const rep = state.verifyReport;

  return (
    <div className="pane">
      <h1>{rep?.all_ok ? "You're running TurdMOD." : "Checking your install."}</h1>
      <p className="lede">
        {rep?.all_ok
          ? "Everything responded. Your server is modded and the engine is live."
          : "Each check below depends on the one before it. Fix the first red item — the rest usually go green on their own."}
      </p>

      {running && (
        <div className="row">
          <div className="spin" />
          <span>Testing the service, the game server, and the engine…</span>
        </div>
      )}

      {rep && (
        <>
          <div className={`verdict ${rep.all_ok ? "good" : "bad"}`}>{rep.summary}</div>
          <div className="stack">
            {rep.checks.map((c) => (
              <div key={c.id} className={`result ${c.ok ? "yes" : "no"}`}>
                <span className="mark">{c.ok ? "✓" : "✕"}</span>
                <div className="body">
                  <div className="t">{c.label}</div>
                  <div className="d">{c.detail}</div>
                  {!c.ok && c.fix && (
                    <div className="fix">
                      <b>Fix:</b> {c.fix}
                    </div>
                  )}
                </div>
              </div>
            ))}
          </div>
        </>
      )}

      {rep?.all_ok && (
        <>
          <h2>What now</h2>
          <div className="stack">
            <div className="note">
              <b>Run your server day to day</b> — grab the TurdMOD Manager dashboard. It connects on port{" "}
              <code>{state.port}</code> with the access key from the previous step.
            </div>
            <div className="note">
              <b>In-game</b> — type <code>!help</code> in chat to see the commands your mods provide. No
              admin account needed.
            </div>
            <div className="note">
              <b>Write your own mod</b> — the docs walk through it, and you can point any AI at them.
            </div>
          </div>
          <div className="row" style={{ marginTop: 16 }}>
            <button className="btn" onClick={() => void openUrl("https://turdmod.com/downloads")}>
              Get the Manager
            </button>
            <button className="btn" onClick={() => void openUrl("https://turdmod.com/docs")}>
              Open the docs
            </button>
          </div>
        </>
      )}

      <div className="actions">
        <button className="btn ghost" onClick={back}>
          Back
        </button>
        <button className="btn" onClick={check} disabled={running}>
          Check again
        </button>
      </div>
    </div>
  );
}
