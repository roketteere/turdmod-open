// Modded client — build an isolated copy so the Steam install stays pristine.
//
// @ctx: the safety story is the headline here, not a footnote. Vanilla stays
//   playable on official servers precisely because we never touch it; if the
//   user doesn't understand that, they'll ask why we don't "just mod the game".

import { open } from "@tauri-apps/plugin-dialog";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { useEffect, useState } from "react";
import { api, type ClientPlan, type StepResult } from "../lib/api";
import { useSetup } from "../lib/setup-state";

const gb = (b: number) => b / 1024 ** 3;
const fmt = (b: number) => (gb(b) >= 1 ? `${gb(b).toFixed(1)} GB` : `${(b / 1024 ** 2).toFixed(0)} MB`);

export function Client({ onClose }: { onClose: () => void }) {
  const { state, set } = useSetup();
  const [plan, setPlan] = useState<ClientPlan | null>(null);
  const [loading, setLoading] = useState(true);
  const [drive, setDrive] = useState<string>("");
  const [dest, setDest] = useState("");
  const [running, setRunning] = useState(false);
  const [results, setResults] = useState<StepResult[]>([]);

  const [source, setSource] = useState(state.detected?.game ?? "");

  useEffect(() => {
    void (async () => {
      try {
        // Reachable straight from the rail, so detection may not have run yet.
        let game = state.detected?.game ?? "";
        if (!state.detected) {
          const d = await api.detectInstalls();
          set({ detected: d });
          game = d.game ?? "";
        }
        setSource(game);
        if (!game) return;

        const p = await api.clientPlan(game);
        setPlan(p);
        // Default to the drive that can share files — cheapest and fastest.
        const best = p.drives.find((d) => d.can_hardlink && d.fits) ?? p.drives.find((d) => d.fits);
        if (best) {
          setDrive(best.name);
          setDest(`${best.name}\\SCUM-Modded`);
        }
      } catch (e) {
        set({ lastError: String(e) });
      } finally {
        setLoading(false);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function pickDrive(name: string) {
    setDrive(name);
    setDest(`${name}\\SCUM-Modded`);
  }

  async function browse() {
    const picked = await open({ directory: true, title: "Where should the modded copy go?" });
    if (typeof picked === "string") {
      setDest(picked.endsWith("SCUM-Modded") ? picked : `${picked}\\SCUM-Modded`);
      setDrive("");
    }
  }

  async function create() {
    setRunning(true);
    try {
      const r = await api.clientCreateCopy(source, dest);
      setResults(r);
      set({ lastError: r.filter((x) => !x.ok).map((x) => `${x.step}: ${x.detail}`).join("; ") });
    } catch (e) {
      set({ lastError: String(e) });
    } finally {
      setRunning(false);
    }
  }

  const done = results.length > 0 && !running;
  const failed = results.filter((r) => !r.ok);
  const chosen = plan?.drives.find((d) => d.name === drive);

  if (!source) {
    return (
      <div className="pane">
        <h1>No SCUM game found on this PC.</h1>
        <p className="lede">
          The modded client is built from your own copy of the game — we never distribute game
          files. Install SCUM through Steam on this machine first, then come back.
        </p>
        <div className="actions">
          <button className="btn ghost" onClick={onClose}>
            Back
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="pane">
      <h1>{done ? (failed.length ? "That didn't finish." : "Modded copy ready.") : "Set up the modded client"}</h1>
      <p className="lede">
        {done
          ? failed.length
            ? "Here's where it stopped. Your Steam install was not touched."
            : "Your modded copy is built and the Launcher knows where to find it. Your Steam install is untouched — Play from Steam still gives you vanilla with BattlEye on."
          : "This makes a separate, moddable copy of the game from your own install. Your Steam copy is never modified, so you can still play on official servers normally."}
      </p>

      {!done && (
        <div className="note" style={{ marginBottom: 22 }}>
          <b>Why a copy?</b> Modding the Steam install in place would break official-server play
          and get flagged by Steam. Keeping them separate is what lets Steam&apos;s Play button stay
          safe. The modded copy is only ever launched by the TurdMOD Launcher, which refuses to
          connect to BattlEye servers.
        </div>
      )}

      {loading && (
        <div className="row">
          <div className="spin" />
          <span>Measuring your game install…</span>
        </div>
      )}

      {plan && !done && (
        <>
          <div className="stack">
            <div className="result yes">
              <span className="mark">✓</span>
              <div className="body">
                <div className="t">Your game</div>
                <div className="d">
                  <span className="mono">{plan.source}</span>
                  <br />
                  {fmt(plan.total_bytes)} across {plan.file_count} files
                </div>
              </div>
            </div>
          </div>

          <h2>Where should it go?</h2>
          <div className="stack">
            {plan.drives.map((d) => (
              <button
                key={d.name}
                className={`choice${drive === d.name ? " selected" : ""}`}
                disabled={!d.fits}
                onClick={() => pickDrive(d.name)}
              >
                <div className="t">
                  {d.name} — {fmt(d.free_bytes)} free
                  {d.can_hardlink && <span style={{ color: "var(--green)" }}> · recommended</span>}
                  {!d.fits && <span style={{ color: "var(--red)" }}> · not enough room</span>}
                </div>
                <div className="d">{d.note}</div>
              </button>
            ))}
          </div>

          <h2>Folder</h2>
          <div className="row">
            <input
              className="input"
              style={{ flex: 1, minWidth: 260 }}
              value={dest}
              onChange={(e) => setDest(e.target.value)}
            />
            <button className="btn" onClick={browse}>
              Browse…
            </button>
          </div>

          {chosen?.can_hardlink && (
            <div className="note" style={{ marginTop: 12 }}>
              Because this is the same drive as your game, the {fmt(plan.linkable_bytes)} of game
              content is <b>shared rather than duplicated</b> — so this only uses about{" "}
              {fmt(plan.copy_bytes)} and finishes in seconds.
            </div>
          )}
        </>
      )}

      {running && (
        <div className="row">
          <div className="spin" />
          <span>
            Building the copy{chosen?.can_hardlink ? " — should be quick" : " — this will take a while"}…
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

      {done && !failed.length && (
        <>
          <h2>What now</h2>
          <div className="note">
            Get the <b>TurdMOD Launcher</b> — it starts this modded copy and connects you to
            TurdMOD servers. It only lists servers with BattlEye off, and refuses to launch against
            a BattlEye server.
          </div>
          <div className="row" style={{ marginTop: 14 }}>
            <button className="btn" onClick={() => void openUrl("https://turdmod.com/downloads")}>
              Get the Launcher
            </button>
          </div>
        </>
      )}

      <div className="actions">
        <button className="btn ghost" onClick={onClose}>
          {done ? "Close" : "Cancel"}
        </button>
        {!done && (
          <button className="btn primary" onClick={create} disabled={running || !dest || !plan}>
            Build the modded copy
          </button>
        )}
      </div>
    </div>
  );
}
