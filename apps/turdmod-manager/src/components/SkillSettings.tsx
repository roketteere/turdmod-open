// Live skill XP-multiplier control for the Admin Map. Sets the
// scum.<Skill>SkillMultiplier console variables at RUNTIME via the bridge's
// setConsoleVarFloat handler — the same values the in-game Server Settings
// sliders drive, but WITHOUT the in-game 0-10 cap (we allow up to 100).
// LIVE — no restart. @dep engineRpc('setConsoleVarFloat') -> bridge.
// @brk Whether SCUM HONORS a multiplier >10 (vs clamping internally) is an
//   engine question — verify by watching a player's XP gain after setting it.
// @ctx Live-only: NOT yet persisted to ServerSettings.ini, so a server restart
//   reverts to the .ini value. (ini-write persistence = follow-up.)
import { useState, type CSSProperties } from 'react';
import { engineRpc } from '../lib/tauri-engine';
import DraggablePanel from './DraggablePanel';

// The 21 per-skill XP-gain multipliers. CVar name = "scum.<key>SkillMultiplier"
// (verified against the live OVH ServerSettings.ini, 2026-06-17).
const SKILLS = [
  'Archery', 'Aviation', 'Awareness', 'Brawling', 'Camouflage', 'Cooking',
  'Demolition', 'Driving', 'Endurance', 'Engineering', 'Farming', 'Handgun',
  'Medical', 'MeleeWeapons', 'Motorcycle', 'Rifles', 'Running', 'Sniping',
  'Stealth', 'Survival', 'Thievery',
] as const;

const MAX = 100; // in-game UI caps at 10; we expose up to 100 (pending honors-it verification)

const clamp = (n: number) => Math.max(0, Math.min(MAX, isFinite(n) ? n : 0));

const inputStyle: CSSProperties = {
  width: 52, background: '#26262b', color: '#ddd', border: '1px solid #3a3a40',
  borderRadius: 4, padding: '2px 4px', fontSize: 12, textAlign: 'right',
};
const setBtn = (disabled?: boolean): CSSProperties => ({
  background: disabled ? '#3a3a3a' : '#9e6a03', color: disabled ? '#888' : '#fff',
  border: 'none', borderRadius: 4, padding: '2px 8px', fontSize: 11,
  fontWeight: 600, cursor: disabled ? 'not-allowed' : 'pointer',
});

export default function SkillSettings({ target, onClose }: { target: string; onClose?: () => void }) {
  const host = target === 'local' ? 'Local' : 'OVH';
  const [vals, setVals] = useState<Record<string, number>>(
    () => Object.fromEntries(SKILLS.map((s) => [s, 10])),
  );
  const [all, setAll] = useState(10);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const flash = (m: string, ms = 6000) => {
    setMsg(m);
    setTimeout(() => setMsg(null), ms);
  };

  const apply = async (name: string, value: number) =>
    engineRpc('setConsoleVarFloat', { name: `scum.${name}SkillMultiplier`, value: String(value) });

  const setOne = async (skill: string) => {
    setBusy(true);
    try {
      await apply(skill, vals[skill]);
      flash(`✓ ${skill} → ${vals[skill]}×`);
    } catch (e) {
      flash(`✗ ${skill}: ${String(e)}`, 9000);
    } finally {
      setBusy(false);
    }
  };

  const setAllSkills = async () => {
    if (!window.confirm(`Set ALL 21 skill multipliers to ${all}× on ${host}? (live — applies now)`)) return;
    setBusy(true);
    let ok = 0;
    try {
      for (const s of SKILLS) {
        await apply(s, all); // serialized through the single bridge pipe anyway
        ok++;
      }
      setVals(Object.fromEntries(SKILLS.map((s) => [s, all])));
      flash(`✓ all ${ok}/${SKILLS.length} skills → ${all}×`, 8000);
    } catch (e) {
      flash(`✗ stopped after ${ok}/${SKILLS.length}: ${String(e)}`, 9000);
    } finally {
      setBusy(false);
    }
  };

  return (
    <DraggablePanel
      title={`Skill Multipliers · ${host}`}
      icon="🎯"
      defaultCorner="tr"
      defaultWidth={272}
      defaultHeight={420}
      minH={200}
      onClose={onClose}
    >
      <div style={{ padding: 10, color: '#ddd', fontFamily: 'system-ui, sans-serif' }}>
        <div style={{ fontSize: 10, color: '#9e6a03', marginBottom: 8, lineHeight: 1.35 }}>
          Live XP-gain rate per skill. In-game caps at 10× — here up to {MAX}×.
          <span style={{ color: '#777' }}> Live only; reverts to ini on restart.</span>
        </div>

        {/* Set-all row */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8,
          borderBottom: '1px solid #3a3a40', paddingBottom: 8 }}>
          <span style={{ fontSize: 12, color: '#bbb', flex: 1 }}>Set ALL skills</span>
          <input
            type="number" min={0} max={MAX} step={1} value={all}
            disabled={busy}
            onChange={(e) => setAll(clamp(Number(e.target.value)))}
            style={inputStyle}
          />
          <span style={{ color: '#777', fontSize: 12 }}>×</span>
          <button disabled={busy} style={setBtn(busy)} onClick={setAllSkills}>
            {busy ? '…' : 'Apply all'}
          </button>
        </div>

        {/* Per-skill rows */}
        <div style={{ maxHeight: 260, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 3 }}>
          {SKILLS.map((s) => (
            <div key={s} style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <span style={{ flex: 1, fontSize: 12, color: '#ccc' }}>{s}</span>
              <input
                type="number" min={0} max={MAX} step={1} value={vals[s]}
                disabled={busy}
                onChange={(e) => setVals((v) => ({ ...v, [s]: clamp(Number(e.target.value)) }))}
                style={inputStyle}
              />
              <span style={{ color: '#777', fontSize: 12 }}>×</span>
              <button disabled={busy} style={setBtn(busy)} onClick={() => setOne(s)}>set</button>
            </div>
          ))}
        </div>

        {msg && (
          <div style={{ marginTop: 9, fontSize: 12, wordBreak: 'break-word',
            color: msg.startsWith('✓') ? '#3fb950' : '#f85149' }}>
            {msg}
          </div>
        )}
      </div>
    </DraggablePanel>
  );
}
