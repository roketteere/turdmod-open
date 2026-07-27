// Step 3 — the honest step.
//
// @ctx: this exists because the most expensive failure mode is someone on a
//       rented FTP-only host spending an hour on an install that physically
//       cannot work. Telling them in 30 seconds is the whole point of the app.

import { open as openUrl } from "@tauri-apps/plugin-shell";
import { useEffect, useState } from "react";
import type { Support } from "../lib/api";
import { api } from "../lib/api";
import { useSetup } from "../lib/setup-state";

const MARK: Record<Support, string> = { yes: "✓", no: "✕", maybe: "?" };

export function Capability() {
  const { state, set, next, back } = useSetup();
  const [loading, setLoading] = useState(false);
  // Only asked when it's genuinely in doubt — local is always yes, rented is
  // always no, and pretending otherwise would be the dishonest part.
  const asksExec = state.hostKind === "own-vps" || state.hostKind === "unknown";
  const [canExec, setCanExec] = useState<boolean | null>(
    state.hostKind === "local" ? true : state.hostKind === "rented-ftp" ? false : null,
  );

  async function load(exec: boolean) {
    setLoading(true);
    try {
      set({ capability: await api.capabilityReport(state.hostKind ?? "unknown", exec) });
    } catch (e) {
      set({ lastError: String(e) });
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (canExec !== null && !state.capability) void load(canExec);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const rep = state.capability;
  const verdictClass = !rep ? "warn" : rep.engine_supported ? "good" : "bad";

  return (
    <div className="pane">
      <h1>Here's what your setup can actually run.</h1>
      <p className="lede">
        TurdMOD's engine has to run as a program on the same machine as the game server. That's the one
        thing that decides what's possible for you — so we check it before doing anything else.
      </p>

      {asksExec && canExec === null && (
        <>
          <h2>One question</h2>
          <p style={{ marginBottom: 14 }}>
            Can you install and run your own programs on that machine — log in with Remote Desktop or SSH
            and start an .exe?
          </p>
          <div className="stack">
            <button
              className="choice"
              onClick={() => {
                setCanExec(true);
                void load(true);
              }}
            >
              <div className="t">Yes, I have full access</div>
              <div className="d">Remote Desktop or SSH, and I can run programs on it.</div>
            </button>
            <button
              className="choice"
              onClick={() => {
                setCanExec(false);
                void load(false);
              }}
            >
              <div className="t">No, or I don't know</div>
              <div className="d">I only get a web panel and file uploads.</div>
            </button>
          </div>
        </>
      )}

      {loading && (
        <div className="row">
          <div className="spin" />
          <span>Checking…</span>
        </div>
      )}

      {rep && (
        <>
          <div className={`verdict ${verdictClass}`}>{rep.verdict}</div>
          <div className="stack">
            {rep.capabilities.map((c) => (
              <div key={c.id} className={`result ${c.support}`}>
                <span className="mark">{MARK[c.support]}</span>
                <div className="body">
                  <div className="t">{c.label}</div>
                  {c.reason && <div className="d">{c.reason}</div>}
                </div>
              </div>
            ))}
          </div>

          {!rep.engine_supported && (
            <>
              <div className="note" style={{ marginTop: 22 }}>
                This isn&apos;t something we can work around, and it isn&apos;t your fault — it&apos;s
                how rented game hosting works. If you want the full engine, the usual move is a cheap
                VPS you control, or running the server on a PC at home. Ask the assistant about either.
              </div>
              {/* Don't leave them with only bad news — Lite exists for exactly
                  this audience and is free. */}
              <h2>What to use instead</h2>
              <div className="note">
                <b>TurdMOD Lite</b> is built for hosts like yours — G-Portal, Nitrado, Host Havoc,
                GTX, PingPerfect. It works over FTP and RCON, and covers most of what admins
                actually do: server settings, loot and economy, raid times, banner messages, admin
                and ban lists, live chat and kill logs. It&apos;s free.
                <div style={{ marginTop: 10 }}>
                  <button className="btn small" onClick={() => void openUrl("https://turdmod.com/downloads")}>
                    Get TurdMOD Lite
                  </button>
                </div>
              </div>
            </>
          )}
        </>
      )}

      <div className="actions">
        <button className="btn ghost" onClick={back}>
          Back
        </button>
        <button className="btn primary" disabled={!rep} onClick={next}>
          {rep?.engine_supported ? "Continue" : "Continue anyway"}
        </button>
      </div>
    </div>
  );
}
