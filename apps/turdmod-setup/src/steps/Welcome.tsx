// Step 1 — the only question that actually matters up front: where does the
// server live? Everything downstream branches off this answer, and getting it
// wrong is what wastes people's evenings.

import type { HostKind } from "../lib/api";
import { useSetup } from "../lib/setup-state";

const CHOICES: Array<{ id: HostKind; title: string; desc: string }> = [
  {
    id: "local",
    title: "On this PC",
    desc: "The SCUM dedicated server runs on the same computer you're using right now. Everything works — this is the easiest setup.",
  },
  {
    id: "own-vps",
    title: "On my own server box",
    desc: "A VPS or dedicated machine you can log into (Remote Desktop or SSH) and install programs on. Everything works.",
  },
  {
    id: "rented-ftp",
    title: "Rented from a game host",
    desc: "You got a server from a hosting company and manage it through their website. You upload files with FTP. Limited — we'll show you exactly what you can still do.",
  },
  {
    id: "unknown",
    title: "I'm not sure",
    desc: "We'll look around and tell you what you've got.",
  },
];

export function Welcome() {
  const { state, set, next } = useSetup();

  return (
    <div className="pane">
      <h1>Let's get TurdMOD running on your server.</h1>
      <p className="lede">
        TurdMOD adds live modding to a SCUM server — custom commands, events, spawning, 90+ mods — without
        needing an admin account logged into the game. This takes about two minutes. First question:
      </p>

      <h2>Where does your SCUM server live?</h2>
      <div className="stack">
        {CHOICES.map((c) => (
          <button
            key={c.id}
            className={`choice${state.hostKind === c.id ? " selected" : ""}`}
            onClick={() => set({ hostKind: c.id })}
          >
            <div className="t">{c.title}</div>
            <div className="d">{c.desc}</div>
          </button>
        ))}
      </div>

      <div className="actions">
        <button className="btn primary" disabled={!state.hostKind} onClick={next}>
          Continue
        </button>
        {!state.hostKind && <span className="note" style={{ border: "none", background: "none", padding: 0 }}>Pick one to continue.</span>}
      </div>
    </div>
  );
}
