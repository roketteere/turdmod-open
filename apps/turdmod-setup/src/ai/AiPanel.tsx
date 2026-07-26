// The assistant side panel: provider sign-in, chat, and confirm cards.
//
// @inv: the API key lives in the Tauri store on this machine and goes to one
//       place — the provider the user picked. Nothing routes through us.

import { load, type Store } from "@tauri-apps/plugin-store";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { useEffect, useMemo, useRef, useState } from "react";
import { useSetup, describeState } from "../lib/setup-state";
import { run, type ChatItem } from "./agent";
import { PROVIDERS, type ProviderName, type Turn } from "./providers";
import { buildTools } from "./tools";

const SUGGESTIONS = [
  "Just install it for me",
  "What can my host actually run?",
  "Why did that fail?",
  "What is TurdMOD, in plain English?",
];

interface Settings {
  provider: ProviderName;
  model: string;
  apiKey: string;
  ollamaHost: string;
  autoRun: boolean;
}

const DEFAULTS: Settings = {
  provider: "anthropic",
  model: PROVIDERS[0].defaultModel,
  apiKey: "",
  ollamaHost: "http://127.0.0.1:11434",
  autoRun: false,
};

export function AiPanel({ onClose }: { onClose: () => void }) {
  const store = useSetup();
  const [cfg, setCfg] = useState<Settings>(DEFAULTS);
  const [loaded, setLoaded] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [items, setItems] = useState<ChatItem[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const turnsRef = useRef<Turn[]>([]);
  const storeRef = useRef<Store | null>(null);
  const pendingRef = useRef<((ok: boolean) => void) | null>(null);
  const bodyRef = useRef<HTMLDivElement>(null);

  const provider = useMemo(
    () => PROVIDERS.find((p) => p.id === cfg.provider) ?? PROVIDERS[0],
    [cfg.provider],
  );
  const ready = !provider.needsKey || cfg.apiKey.trim().length > 0;

  useEffect(() => {
    void (async () => {
      try {
        const s = await load("assistant.json", { autoSave: true, defaults: {} });
        storeRef.current = s;
        const saved = await s.get<Settings>("settings");
        if (saved) setCfg({ ...DEFAULTS, ...saved });
        else setShowSettings(true);
      } catch {
        setShowSettings(true);
      } finally {
        setLoaded(true);
      }
    })();
  }, []);

  useEffect(() => {
    if (loaded && !ready) setShowSettings(true);
  }, [loaded, ready]);

  useEffect(() => {
    bodyRef.current?.scrollTo({ top: bodyRef.current.scrollHeight, behavior: "smooth" });
  }, [items]);

  async function saveCfg(patch: Partial<Settings>) {
    const nxt = { ...cfg, ...patch };
    setCfg(nxt);
    await storeRef.current?.set("settings", nxt);
  }

  function answerConfirm(ok: boolean) {
    pendingRef.current?.(ok);
    pendingRef.current = null;
  }

  async function send(text: string) {
    if (!text.trim() || busy) return;
    setInput("");
    setBusy(true);
    try {
      turnsRef.current = await run({
        provider: cfg.provider,
        model: cfg.model,
        apiKey: cfg.apiKey,
        ollamaHost: cfg.ollamaHost,
        tools: buildTools(store),
        stateSummary: describeState(store.state),
        turns: turnsRef.current,
        userMessage: text,
        autoRun: cfg.autoRun,
        confirm: () => new Promise<boolean>((resolve) => (pendingRef.current = resolve)),
        emit: (item) => setItems((xs) => [...xs, item]),
        update: (id, patch) =>
          setItems((xs) =>
            xs.map((x) => (x.id === id && x.kind === "tool" ? { ...x, ...patch } : x)),
          ),
      });
    } finally {
      setBusy(false);
      pendingRef.current = null;
    }
  }

  return (
    <aside className="ai">
      <div className="ai-head">
        <span className="t">Setup assistant</span>
        <button className="btn ghost small" onClick={() => setShowSettings((v) => !v)}>
          {showSettings ? "Chat" : "⚙"}
        </button>
        <button className="btn ghost small" onClick={onClose}>
          ✕
        </button>
      </div>

      {showSettings ? (
        <div className="ai-body">
          <p style={{ fontSize: 13, lineHeight: 1.6, color: "var(--muted)" }}>
            The assistant can walk you through the install — or just do it. Pick an AI you have an account
            with. Your key stays on this PC and only ever goes to that provider.
          </p>

          <div className="field">
            <label>AI provider</label>
            <select
              className="input"
              value={cfg.provider}
              onChange={(e) => {
                const id = e.target.value as ProviderName;
                const p = PROVIDERS.find((x) => x.id === id)!;
                void saveCfg({ provider: id, model: p.defaultModel });
              }}
            >
              {PROVIDERS.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </select>
            <span style={{ fontSize: 12.5, color: "var(--muted)", lineHeight: 1.5 }}>
              {provider.note}
            </span>
          </div>

          {provider.needsKey ? (
            <div className="field">
              <label>API key</label>
              <input
                className="input"
                type="password"
                placeholder="paste your key"
                value={cfg.apiKey}
                onChange={(e) => void saveCfg({ apiKey: e.target.value })}
              />
              {provider.keyUrl && (
                <button className="setup-link" onClick={() => void openUrl(provider.keyUrl!)}>
                  Get a key from {provider.label} →
                </button>
              )}
            </div>
          ) : (
            <div className="field">
              <label>Ollama address</label>
              <input
                className="input"
                value={cfg.ollamaHost}
                onChange={(e) => void saveCfg({ ollamaHost: e.target.value })}
              />
            </div>
          )}

          <div className="field">
            <label>Model</label>
            <input
              className="input"
              value={cfg.model}
              onChange={(e) => void saveCfg({ model: e.target.value })}
            />
          </div>

          <label className="check">
            <input
              type="checkbox"
              checked={cfg.autoRun}
              onChange={(e) => void saveCfg({ autoRun: e.target.checked })}
            />
            Let it install without asking me each time
          </label>
          <div className="note">
            Leave that off and the assistant asks before anything that changes your PC. Turn it on for a
            hands-off install.
          </div>

          <button className="btn primary" disabled={!ready} onClick={() => setShowSettings(false)}>
            Start chatting
          </button>
        </div>
      ) : (
        <>
          <div className="ai-body" ref={bodyRef}>
            {items.length === 0 && (
              <p style={{ fontSize: 13.5, lineHeight: 1.6, color: "var(--muted)" }}>
                Ask me anything about setting this up — or tell me to do it and I'll run the steps for
                you, checking with you before anything that changes your PC.
              </p>
            )}

            {items.map((m) =>
              m.kind === "tool" ? (
                <div key={m.id} className={`toolcard ${m.status}`}>
                  <div className="t">
                    {m.status === "running" && <div className="spin" />}
                    <span>{m.summary}</span>
                  </div>
                  {m.status === "awaiting" && (
                    <>
                      <div className="why">The assistant wants to do this. It changes your PC.</div>
                      <div className="confirm">
                        <button className="btn small primary" onClick={() => answerConfirm(true)}>
                          Allow
                        </button>
                        <button className="btn small danger" onClick={() => answerConfirm(false)}>
                          Not now
                        </button>
                      </div>
                    </>
                  )}
                  {m.status === "denied" && <div className="why">Skipped.</div>}
                  {m.status === "failed" && <div className="why">Failed: {m.result}</div>}
                  {m.status === "done" && <div className="why">Done.</div>}
                </div>
              ) : (
                <div key={m.id} className={`msg ${m.kind}`}>
                  {m.text}
                </div>
              ),
            )}

            {busy && !pendingRef.current && (
              <div className="row">
                <div className="spin" />
                <span style={{ fontSize: 13, color: "var(--muted)" }}>Thinking…</span>
              </div>
            )}
          </div>

          <div className="ai-foot">
            {items.length === 0 && (
              <div className="ai-suggest">
                {SUGGESTIONS.map((s) => (
                  <button key={s} className="chip" onClick={() => void send(s)}>
                    {s}
                  </button>
                ))}
              </div>
            )}
            <div className="ai-input">
              <textarea
                value={input}
                placeholder={ready ? "Ask, or say “install it for me”" : "Set up a provider first →"}
                disabled={!ready}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    void send(input);
                  }
                }}
              />
              <button className="btn primary" disabled={!ready || busy || !input.trim()} onClick={() => void send(input)}>
                Send
              </button>
            </div>
          </div>
        </>
      )}
    </aside>
  );
}
