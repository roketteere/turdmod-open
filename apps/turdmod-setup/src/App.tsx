import { useCallback, useMemo, useState } from "react";
import { AiPanel } from "./ai/AiPanel";
import {
  SetupContext,
  STEPS,
  initialState,
  stepIndex,
  type SetupState,
  type StepId,
} from "./lib/setup-state";
import { Capability } from "./steps/Capability";
import { Client } from "./steps/Client";
import { Configure } from "./steps/Configure";
import { Detect } from "./steps/Detect";
import { Install } from "./steps/Install";
import { Uninstall } from "./steps/Uninstall";
import { Verify } from "./steps/Verify";
import { Welcome } from "./steps/Welcome";

export function App() {
  const [state, setState] = useState<SetupState>(initialState);
  const [aiOpen, setAiOpen] = useState(false);
  // Side views live outside the numbered flow — the modded client is a parallel
  // track, and "remove" must never be something you have to hunt for.
  const [view, setView] = useState<"flow" | "client" | "uninstall">("flow");
  /** Furthest step reached — lets the rail act as back-navigation. */
  const [reached, setReached] = useState<StepId>("welcome");

  const set = useCallback((patch: Partial<SetupState>) => {
    setState((s) => ({ ...s, ...patch }));
  }, []);

  const go = useCallback((step: StepId) => {
    setState((s) => ({ ...s, step }));
    setReached((r) => (stepIndex(step) > stepIndex(r) ? step : r));
  }, []);

  const next = useCallback(() => {
    setState((s) => {
      const i = stepIndex(s.step);
      const step = STEPS[Math.min(i + 1, STEPS.length - 1)].id;
      setReached((r) => (stepIndex(step) > stepIndex(r) ? step : r));
      return { ...s, step };
    });
  }, []);

  const back = useCallback(() => {
    setState((s) => {
      const i = stepIndex(s.step);
      return { ...s, step: STEPS[Math.max(i - 1, 0)].id };
    });
  }, []);

  const store = useMemo(() => ({ state, set, go, next, back }), [state, set, go, next, back]);

  return (
    <SetupContext.Provider value={store}>
      <div className={`app${aiOpen ? " with-ai" : ""}`}>
        <nav className="rail">
          <div className="brand">
            Turd<span className="mod">MOD</span>
          </div>
          <div className="brand-sub">Setup</div>

          {STEPS.map((s, i) => {
            const visitable = stepIndex(s.id) <= stepIndex(reached);
            const done = stepIndex(s.id) < stepIndex(reached);
            return (
              <button
                key={s.id}
                className={`railstep${state.step === s.id && view === "flow" ? " active" : ""}${done ? " done" : ""}`}
                disabled={!visitable}
                onClick={() => {
                  setView("flow");
                  go(s.id);
                }}
              >
                <span className="dot">{done ? "✓" : i + 1}</span>
                {s.label}
              </button>
            );
          })}

          <div className="rail-foot">
            <button className="btn small" onClick={() => setAiOpen((v) => !v)}>
              {aiOpen ? "Hide assistant" : "Ask the assistant"}
            </button>
            <button
              className={`railstep${view === "client" ? " active" : ""}`}
              style={{ marginTop: 8, width: "100%" }}
              onClick={() => setView("client")}
            >
              {/* Plain glyphs, not emoji — 🤖 rendered as tofu in WebView2. */}
              <span className="dot">▶</span>
              Modded client
            </button>
            <button
              className={`railstep${view === "uninstall" ? " active" : ""}`}
              style={{ width: "100%" }}
              onClick={() => setView("uninstall")}
            >
              <span className="dot">↺</span>
              Remove TurdMOD
            </button>
          </div>
        </nav>

        <main className="main">
          {view === "uninstall" && <Uninstall onClose={() => setView("flow")} />}
          {view === "client" && <Client onClose={() => setView("flow")} />}
          {view === "flow" && (
            <>
              {state.step === "welcome" && <Welcome />}
              {state.step === "detect" && <Detect />}
              {state.step === "capability" && <Capability />}
              {state.step === "configure" && <Configure />}
              {state.step === "install" && <Install />}
              {state.step === "verify" && <Verify />}
            </>
          )}
        </main>

        {aiOpen && <AiPanel onClose={() => setAiOpen(false)} />}
      </div>
    </SetupContext.Provider>
  );
}
