// Step 4 — settings, generated not typed.
//
// @ctx: hand-editing service.json is the single biggest source of "it doesn't
//       work" reports. Nothing on this screen is required input.

import { useEffect, useState } from "react";
import { api, type StepResult } from "../lib/api";
import { useSetup } from "../lib/setup-state";

export function Configure() {
  const { state, set, next, back } = useSetup();
  const [loading, setLoading] = useState(false);
  const [showToken, setShowToken] = useState(false);
  const [port, setPort] = useState(String(state.port));
  const [downloading, setDownloading] = useState(false);
  const [downloadSteps, setDownloadSteps] = useState<StepResult[]>([]);

  // A bare Setup.exe has no pack beside it — fetch it rather than sending the
  // user back to the website to do it by hand.
  async function download() {
    setDownloading(true);
    setDownloadSteps([]);
    try {
      const r = await api.downloadPack();
      setDownloadSteps(r.steps);
      if (r.artifacts_dir) {
        set({ artifactsDir: r.artifacts_dir, lastError: "" });
        await prepare(state.port);
      }
    } catch (e) {
      set({ lastError: String(e) });
    } finally {
      setDownloading(false);
    }
  }

  async function prepare(p: number) {
    setLoading(true);
    try {
      const cfg = await api.prepareConfig(state.serverRoot, p);
      set({
        token: cfg.token,
        port: cfg.port,
        config: cfg.config,
        artifactsDir: cfg.artifacts_dir,
        isUpdate: cfg.is_update,
        tokenPreserved: cfg.token_preserved,
        serviceState: cfg.service_state,
        lastError: "",
      });
    } catch (e) {
      set({ lastError: String(e) });
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (!state.config) void prepare(state.port);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function applyPort() {
    const p = Number(port);
    if (Number.isInteger(p) && p > 1023 && p < 65536 && p !== state.port) void prepare(p);
    else setPort(String(state.port));
  }

  return (
    <div className="pane">
      <h1>{state.isUpdate ? "Updating your existing install." : "Settings — already filled in."}</h1>
      <p className="lede">
        {state.isUpdate
          ? "TurdMOD is already here, so this is an update. Your settings stay exactly as they are — we only refresh the program files."
          : "We generated everything from what we found. You don't need to change any of this; it's here so you know what's about to happen."}
      </p>

      {state.isUpdate && (
        <div className={`verdict ${state.tokenPreserved ? "good" : "warn"}`}>
          {state.tokenPreserved
            ? "Keeping your current access key and settings — your Manager dashboard will keep working after the update."
            : "No access key found in your existing config, so a new one was generated. You'll need to update it in Manager."}
        </div>
      )}

      <div className="stack">
        <div className="result yes">
          <span className="mark">✓</span>
          <div className="body">
            <div className="t">Server folder</div>
            <div className="d mono">{state.serverRoot}</div>
          </div>
        </div>

        <div className={`result ${state.artifactsDir ? "yes" : "no"}`}>
          <span className="mark">{state.artifactsDir ? "✓" : "✕"}</span>
          <div className="body">
            <div className="t">TurdMOD files (Server Pack)</div>
            <div className="d">
              {state.artifactsDir ? (
                <span className="mono">{state.artifactsDir}</span>
              ) : (
                <>
                  Not here yet — but we can fetch them for you.
                  <div style={{ marginTop: 10 }}>
                    <button className="btn small primary" onClick={() => void download()} disabled={downloading}>
                      {downloading ? "Downloading…" : "Download the Server Pack (~16 MB)"}
                    </button>
                  </div>
                </>
              )}
            </div>
          </div>
        </div>

        {downloadSteps.map((r, i) => (
          <div key={`${r.step}-${i}`} className={`result ${r.ok ? "yes" : "no"}`}>
            <span className="mark">{r.ok ? "✓" : "✕"}</span>
            <div className="body">
              <div className="t">{r.step}</div>
              <div className="d">{r.detail}</div>
            </div>
          </div>
        ))}

        <div className="result yes">
          <span className="mark">✓</span>
          <div className="body">
            <div className="t">Access key</div>
            <div className="d">
              {state.tokenPreserved
                ? "Your existing key — unchanged, so nothing you've already set up breaks. Keep it private."
                : "Generated for you — the Manager dashboard uses it to talk to your server. Keep it private."}
              <div style={{ marginTop: 7 }}>
                <span className="mono">
                  {showToken ? state.token : state.token.replace(/./g, "•").slice(0, 40)}
                </span>{" "}
                <button className="setup-link" onClick={() => setShowToken((v) => !v)}>
                  {showToken ? "hide" : "show"}
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <h2>Port</h2>
      <div className="row">
        <input
          className="input short"
          value={port}
          onChange={(e) => setPort(e.target.value)}
          onBlur={applyPort}
        />
        <span className="note" style={{ border: "none", background: "none", padding: 0 }}>
          Only change this if something else on the machine already uses {state.port}.
        </span>
      </div>

      <div className="actions">
        <button className="btn ghost" onClick={back}>
          Back
        </button>
        <button className="btn" onClick={() => void prepare(state.port)} disabled={loading}>
          Re-check
        </button>
        <span className="spacer" />
        <button className="btn primary" disabled={!state.config || loading} onClick={next}>
          {state.isUpdate ? "Update" : "Install"}
        </button>
      </div>
    </div>
  );
}
