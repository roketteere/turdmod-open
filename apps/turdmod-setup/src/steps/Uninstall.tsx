// Remove TurdMOD. Not part of the numbered flow — reachable any time from the
// rail, because an installer you can't back out of is one people won't try.
//
// @inv: always show the plan before doing anything. And surface `warning`
//   prominently — that's the "we can't fully reverse this" case.

import { useEffect, useState } from "react";
import { api, type StepResult, type UninstallPlan } from "../lib/api";
import { useSetup } from "../lib/setup-state";

export function Uninstall({ onClose }: { onClose: () => void }) {
  const { set } = useSetup();
  const [plan, setPlan] = useState<UninstallPlan | null>(null);
  const [removeSettings, setRemoveSettings] = useState(false);
  const [running, setRunning] = useState(false);
  const [results, setResults] = useState<StepResult[]>([]);
  const [confirming, setConfirming] = useState(false);

  useEffect(() => {
    void (async () => {
      try {
        setPlan(await api.uninstallPlan());
      } catch (e) {
        set({ lastError: String(e) });
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function run() {
    setRunning(true);
    setConfirming(false);
    try {
      const r = await api.uninstallRun(removeSettings);
      setResults(r);
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

  const done = results.length > 0 && !running;
  const failed = results.filter((r) => !r.ok);

  return (
    <div className="pane">
      <h1>{done ? (failed.length ? "Partly removed." : "TurdMOD removed.") : "Remove TurdMOD"}</h1>
      <p className="lede">
        {done
          ? failed.length
            ? "Some items didn't reverse. The install record was kept so you can run this again after fixing what's below."
            : "Your server is back to how it was before TurdMOD was installed."
          : "This puts everything back: files we replaced are restored from backup, files we added are deleted, and the bridge comes out of UE4SS's mod list — leaving your other mods alone."}
      </p>

      {plan?.warning && !done && <div className="verdict warn">{plan.warning}</div>}

      {plan && !done && (
        <>
          {plan.service_state === "running" && (
            <div className="verdict bad">
              Your server is running. Removing TurdMOD stops it.
            </div>
          )}

          <h2>What will happen</h2>
          <div className="stack">
            {plan.steps.length === 0 ? (
              <div className="note">Nothing to remove — TurdMOD doesn&apos;t appear to be installed.</div>
            ) : (
              plan.steps.map((s, i) => (
                <div key={i} className="result pending">
                  <span className="mark">{i + 1}</span>
                  <div className="body">
                    <div className="d">{s}</div>
                  </div>
                </div>
              ))
            )}
          </div>

          {plan.has_manifest && (
            <>
              <h2>Your settings</h2>
              <label className="check">
                <input
                  type="checkbox"
                  checked={removeSettings}
                  onChange={(e) => setRemoveSettings(e.target.checked)}
                />
                Also delete my settings (access key, ports, tuning)
              </label>
              <div className="note">
                Leave this off and <span className="mono">service.json</span> stays put, so if you
                reinstall later everything is exactly as you had it — same access key, so your
                Manager dashboard keeps working without reconfiguring.
              </div>
            </>
          )}
        </>
      )}

      {running && (
        <div className="row">
          <div className="spin" />
          <span>Removing — stopping the service and restoring your files.</span>
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
        <button className="btn ghost" onClick={onClose}>
          {done ? "Close" : "Cancel"}
        </button>
        {!done &&
          (confirming ? (
            <>
              <span className="note" style={{ border: "none", background: "none", padding: 0 }}>
                Sure? This stops the service and reverses the install.
              </span>
              <button className="btn danger" onClick={run} disabled={running}>
                Yes, remove it
              </button>
              <button className="btn ghost small" onClick={() => setConfirming(false)}>
                No
              </button>
            </>
          ) : (
            <button
              className="btn danger"
              onClick={() => setConfirming(true)}
              disabled={running || !plan || plan.steps.length === 0}
            >
              Remove TurdMOD
            </button>
          ))}
      </div>
    </div>
  );
}
