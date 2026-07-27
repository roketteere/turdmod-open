// Step 5 — do the work, show every step pass or fail.
//
// @inv: a failed step must never be summarised away. install_local_full stops
//       before the service install if any file copy failed; the UI has to make
//       that visible or the user will "continue" onto a broken install.

import { useState } from "react";
import { api } from "../lib/api";
import { useSetup } from "../lib/setup-state";

export function Install() {
  const { state, set, next, back } = useSetup();
  const [running, setRunning] = useState(false);

  const results = state.installResults;
  const done = results.length > 0 && !running;
  const failed = results.filter((r) => !r.ok);
  const engineBlocked = state.capability && !state.capability.engine_supported;
  // @inv: install_local_full writes to THIS machine. Offering it when the
  // server lives elsewhere would install TurdMOD onto the wrong computer —
  // so the remote cases get instructions, not a button that lies.
  const isLocal = state.hostKind === "local";

  async function install() {
    if (!state.config) return;
    setRunning(true);
    set({ installResults: [] });
    try {
      const r = await api.installLocal(state.serverRoot, state.config, state.artifactsDir);
      set({
        installResults: r,
        lastError: r.filter((x) => !x.ok).map((x) => `${x.step}: ${x.detail}`).join("; "),
      });
    } catch (e) {
      set({ lastError: String(e) });
    } finally {
      setRunning(false);
    }
  }

  if (!isLocal) {
    return (
      <div className="pane">
        <h1>Your server isn&apos;t on this PC.</h1>
        <p className="lede">
          This app installs onto the machine it&apos;s running on. Yours is somewhere else, so here&apos;s
          the shortest path from where you are.
        </p>

        {state.capability?.engine_supported ? (
          <>
            <div className="verdict good">
              Good news — your setup runs everything. You just need to do this from the server box.
            </div>
            <div className="stack">
              <div className="result yes">
                <span className="mark">1</span>
                <div className="body">
                  <div className="t">Log into your server box</div>
                  <div className="d">Remote Desktop for Windows, or SSH if you prefer the shell.</div>
                </div>
              </div>
              <div className="result yes">
                <span className="mark">2</span>
                <div className="body">
                  <div className="t">Copy two things over</div>
                  <div className="d">
                    This app (<span className="mono">TurdMOD-Setup.exe</span>) and the extracted Server
                    Pack. Both are in the same folder you got them from.
                  </div>
                </div>
              </div>
              <div className="result yes">
                <span className="mark">3</span>
                <div className="body">
                  <div className="t">Run it there, as Administrator</div>
                  <div className="d">
                    Pick &ldquo;On this PC&rdquo; on the first screen and it does everything
                    automatically — this whole wizard, but on the right machine.
                  </div>
                </div>
              </div>
            </div>
            <div className="note" style={{ marginTop: 20 }}>
              Installing over SSH from here is coming. For now, running Setup on the box itself is
              faster anyway — it can see the files it&apos;s working with.
            </div>
          </>
        ) : (
          <>
            <div className="verdict bad">
              Your host can&apos;t run the engine, so there&apos;s nothing for this step to install.
            </div>
            <p style={{ marginBottom: 16 }}>
              What you can still do, using your host&apos;s file manager or FTP:
            </p>
            <div className="stack">
              <div className="result yes">
                <span className="mark">✓</span>
                <div className="body">
                  <div className="t">Pak and asset mods</div>
                  <div className="d">
                    Upload <span className="mono">.pak</span> files to your server&apos;s content
                    folder. These work on any host.
                  </div>
                </div>
              </div>
              <div className="result yes">
                <span className="mark">✓</span>
                <div className="body">
                  <div className="t">Config tuning</div>
                  <div className="d">
                    Loot multipliers, skill rates, zombie settings — edit{" "}
                    <span className="mono">ServerSettings.ini</span> through your host&apos;s panel.
                  </div>
                </div>
              </div>
            </div>
            <div className="note" style={{ marginTop: 20 }}>
              If you want the full engine later, the usual move is a cheap VPS you control, or running
              the server on a PC at home. Ask the assistant and it&apos;ll walk you through either.
            </div>
          </>
        )}

        <div className="actions">
          <button className="btn ghost" onClick={back}>
            Back
          </button>
          <button className="btn" onClick={() => set({ hostKind: "local" })}>
            Actually, the server is on this PC
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="pane">
      <h1>
        {done
          ? failed.length
            ? "Something went wrong."
            : state.isUpdate
              ? "Updated."
              : "Installed."
          : state.isUpdate
            ? "Ready to update."
            : "Ready to install."}
      </h1>
      <p className="lede">
        {done
          ? failed.length
            ? "Here's exactly where it stopped. Fix the item below and run it again — or ask the assistant, it can read the logs."
            : state.isUpdate
              ? "New files are in place and the service is back up, with your settings untouched. Next we check it's actually working."
              : "Files are in place and the TurdMOD service is installed. Next we check that it's actually working."
          : state.isUpdate
            ? "This replaces the TurdMOD program files with the new ones and restarts the service. Your config, access key, and mod settings stay as they are."
            : "This copies the TurdMOD files into your server folder and installs the background service that runs the engine."}
      </p>

      {!done && !running && (
        <>
          {engineBlocked && (
            <div className="verdict bad">
              Your host can't run the engine — this install will not give you engine mods. You can still
              use pak mods and config tuning.
            </div>
          )}
          {/* Said before they commit, not discovered afterwards. */}
          {state.battleyeWillBeDisabled && (
            <div className="verdict warn">
              <b>Heads up: this turns BattlEye off on your server.</b>
              <div style={{ marginTop: 8 }}>
                TurdMOD needs it off — the modded client refuses to connect to a BattlEye server, so
                leaving it on means client mods silently don&apos;t work. We&apos;re telling you
                rather than doing it quietly, and <b>removing TurdMOD turns it back on</b>, exactly
                as you had it.
              </div>
            </div>
          )}

          {state.isUpdate ? (
            <>
              {state.serviceState === "running" && (
                <div className="verdict warn">
                  Your server is running right now. Updating stops it — the engine files are loaded
                  inside the game process, so they can&apos;t be swapped live. Expect a few minutes of
                  downtime. Warn your players first if that matters.
                </div>
              )}
              <div className="note">
                We back up your current <span className="mono">turdmod-service.exe</span> and{" "}
                <span className="mono">service.json</span> to{" "}
                <span className="mono">.bak-preupdate</span> first, then stop the service, replace the
                files, and start it again. Run this <b>as Administrator</b>.
              </div>
            </>
          ) : (
            <div className="note">
              Two things worth checking first: <b>stop your SCUM server</b> if it&apos;s running (files
              can&apos;t be replaced while it&apos;s in use), and make sure this app is running{" "}
              <b>as Administrator</b> (installing a Windows service needs it).
            </div>
          )}
        </>
      )}

      {running && (
        <div className="row">
          <div className="spin" />
          <span>
            {state.isUpdate
              ? "Updating — stopping the service, swapping files, starting it back up."
              : "Installing — this takes a few seconds."}
          </span>
        </div>
      )}

      {results.length > 0 && (
        <div className="stack" style={{ marginTop: 20 }}>
          {results.map((r, i) => (
            <div key={`${r.step}-${i}`} className={`result ${r.ok ? "yes" : "no"}`}>
              <span className="mark">{r.ok ? "✓" : "✕"}</span>
              <div className="body">
                <div className="t">{r.step}</div>
                <div className="d">{r.detail}</div>
              </div>
            </div>
          ))}
        </div>
      )}

      <div className="actions">
        <button className="btn ghost" onClick={back} disabled={running}>
          Back
        </button>
        <button className="btn" onClick={install} disabled={running || !state.config}>
          {results.length ? "Run again" : state.isUpdate ? "Update now" : "Install now"}
        </button>
        <span className="spacer" />
        <button className="btn primary" disabled={!done} onClick={next}>
          Check it worked
        </button>
      </div>
    </div>
  );
}
