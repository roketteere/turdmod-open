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
import { Configure } from "./steps/Configure";
import { Detect } from "./steps/Detect";
import { Install } from "./steps/Install";
import { Uninstall } from "./steps/Uninstall";
import { Verify } from "./steps/Verify";
import { Welcome } from "./steps/Welcome";

export function App() {
  const [state, setState] = useState<SetupState>(initialState);
  const [aiOpen, setAiOpen] = useState(false);
  // Outside the numbered flow — reachable any time, so backing out is never
  // a thing you have to hunt for.
  const [removing, setRemoving] = useState(false);
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
                className={`railstep${state.step === s.id && !removing ? " active" : ""}${done ? " done" : ""}`}
                disabled={!visitable}
                onClick={() => {
                  setRemoving(false);
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
              className={`railstep${removing ? " active" : ""}`}
              style={{ marginTop: 8, width: "100%" }}
              onClick={() => setRemoving(true)}
            >
              <span className="dot">↺</span>
              Remove TurdMOD
            </button>
          </div>
        </nav>

        <main className="main">
          {removing ? (
            <Uninstall onClose={() => setRemoving(false)} />
          ) : (
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
