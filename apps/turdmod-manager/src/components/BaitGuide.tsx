import { useMemo, useState } from 'react';
import DraggablePanel from './DraggablePanel';
import baitData from '../data/bait-feeder.json';

// Bait feeder reference — what food/seed/meat attracts which animal + the spawn ring.
// Data extracted live from AnimalBaitDataAsset (bridge dumpAnimalBait). @dep: data/bait-feeder.json
// @inv there is NO per-bait probability in the game data: bait is set-membership, the only
// spatial knobs are min/max distance (cm) and the half-angle (180 = full 360 ring).

interface AnimalBait { animal: string; baits: string[]; minCm: number; maxCm: number; halfAngleDeg: number }

// class -> {emoji, label}. Falls back to a cleaned class name.
const ANIMAL_META: Record<string, { icon: string; label: string }> = {
  BP_Bear2_C: { icon: '🐻', label: 'Bear' },
  BP_Wolf3_C: { icon: '🐺', label: 'Wolf' },
  BP_Deer2_C: { icon: '🦌', label: 'Deer' },
  BP_Horse2_C: { icon: '🐴', label: 'Horse' },
  BP_Goat2_C: { icon: '🐐', label: 'Goat' },
  BP_Boar_C: { icon: '🐗', label: 'Boar' },
  BP_Rabbit_C: { icon: '🐰', label: 'Rabbit' },
  BP_Donkey2_C: { icon: '🫏', label: 'Donkey' },
  BP_Chicken_C: { icon: '🐔', label: 'Chicken' },
};

// "Catfish_Fillet_ES_C" -> "Catfish Fillet"
const niceBait = (code: string) =>
  code.replace(/_ES_C$/, '').replace(/_C$/, '').replace(/_/g, ' ').replace(/\s+/g, ' ').trim();

const meta = (cls: string) => ANIMAL_META[cls] ?? { icon: '🐾', label: cls.replace(/^BP_/, '').replace(/\d?_C$/, '') };

export function BaitGuide({ onClose }: { onClose?: () => void }) {
  const animals = (baitData as { animals: AnimalBait[] }).animals ?? [];
  const [q, setQ] = useState('');
  const query = q.trim().toLowerCase();

  // When searching, filter to animals whose name OR any bait matches, and within
  // a matched animal only surface the baits that hit (so "corn" shows deer+chicken).
  const view = useMemo(() => {
    if (!query) return animals.map((a) => ({ a, baits: a.baits }));
    return animals
      .map((a) => {
        const animalHit = meta(a.animal).label.toLowerCase().includes(query);
        const baits = animalHit ? a.baits : a.baits.filter((b) => niceBait(b).toLowerCase().includes(query));
        return { a, baits };
      })
      .filter((x) => x.baits.length > 0);
  }, [animals, query]);

  return (
    <DraggablePanel title="🪤 Bait Feeder Guide" defaultCorner="tr" defaultWidth={320} defaultHeight={520} minH={200} onClose={onClose}>
      <div className="flex h-full flex-col">
        <div className="shrink-0 px-2.5 pb-1.5 pt-2">
          <input
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Search food or animal… (e.g. corn, wolf)"
            className="w-full rounded border border-turd-bronze/40 bg-turd-bg-deep/70 px-2 py-1 font-mono text-[11px] text-turd-cream placeholder:text-turd-cream-dim/40 focus:border-turd-mustard/60 focus:outline-none"
          />
        </div>
        <div className="min-h-0 flex-1 overflow-auto px-2 pb-2">
          {view.length === 0 && <p className="px-2 py-3 text-[10px] text-turd-cream-dim">No bait or animal matches “{q}”.</p>}
          {view.map(({ a, baits }) => {
            const m = meta(a.animal);
            return (
              <div key={a.animal} className="mb-2 rounded-md border border-turd-bronze/25 bg-turd-bg-deep/40 p-2">
                <div className="mb-1 flex items-center justify-between gap-2">
                  <span className="font-display text-sm text-turd-cream">
                    <span className="mr-1">{m.icon}</span>{m.label}
                  </span>
                  <span className="shrink-0 font-mono text-[9px] text-turd-green" title="Spawn ring around the feeder (360°)">
                    spawns {Math.round(a.minCm / 100)}–{Math.round(a.maxCm / 100)} m
                  </span>
                </div>
                <div className="flex flex-wrap gap-1">
                  {baits.map((b) => (
                    <span key={b} className="rounded border border-turd-bronze/30 bg-turd-bg-soft px-1.5 py-0.5 font-mono text-[10px] text-turd-cream-dim">
                      {niceBait(b)}
                    </span>
                  ))}
                </div>
              </div>
            );
          })}
          <p className="px-1 py-1 text-[9px] leading-relaxed text-turd-cream-dim/50">
            Drop any listed item in a Wild Animal Feeder to attract that animal; it spawns anywhere in the
            ring at the listed distance. Attraction is yes/no — there's no per-bait chance.
          </p>
        </div>
      </div>
    </DraggablePanel>
  );
}

export default BaitGuide;
