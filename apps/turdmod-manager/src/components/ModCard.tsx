import { Link } from 'react-router-dom';
import type { InstalledMod, ModSummary } from '../lib/tauri';
import { InstallButton } from './InstallButton';
import { PriceBadge } from './PriceBadge';

interface ModCardProps {
  mod: ModSummary;
  installed?: InstalledMod | null;
}

// Branded mod icon by category (public/mod-icons/<cat>.svg). Unknown -> default.
// @dep tools/brand/gen-mod-icons.mjs writes these from the ModCategory list.
const MOD_ICON_CATEGORIES = new Set([
  'ui', 'hud', 'squad', 'admin', 'gameplay', 'map', 'vehicle', 'npc',
]);
function modIconSrc(category?: string | null): string {
  const c = (category ?? '').toLowerCase();
  return `/mod-icons/${MOD_ICON_CATEGORIES.has(c) ? c : 'default'}.svg`;
}

export function ModCard({ mod, installed }: ModCardProps) {
  return (
    <li className="group flex flex-col glass rounded-xl transition-colors hover:border-turd-mustard/60 hover:bg-turd-bg-soft/60">
      <Link to={`/mods/${mod.slug}`} className="flex flex-1 flex-col p-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <img
              src={modIconSrc(mod.category)}
              alt=""
              className="h-11 w-11 shrink-0 rounded-md"
              loading="lazy"
            />
            <span className="rounded bg-turd-bg-soft px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider text-turd-mustard">
              {mod.tag}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <PriceBadge
              isPremium={mod.isPremium}
              isPremiumBundled={mod.isPremiumBundled}
              priceCents={mod.priceCents}
            />
            {mod.version && (
              <span className="font-mono text-[10px] text-turd-cream-dim">
                v{mod.version}
              </span>
            )}
          </div>
        </div>
        <h3 className="mt-3 font-display text-lg text-turd-cream-dim transition-colors group-hover:text-turd-mustard-bright">
          {mod.name}
        </h3>
        <p
          className="mt-2 flex-1 text-sm text-turd-cream-dim/90"
          style={{
            display: '-webkit-box',
            WebkitLineClamp: 3,
            WebkitBoxOrient: 'vertical',
            overflow: 'hidden',
          }}
        >
          {mod.summary}
        </p>
        <p className="mt-4 text-xs text-turd-cream-dim">
          by <span className="text-turd-cream">{mod.author?.displayName ?? 'TurdMOD'}</span>
          {mod.proposedBy && (
            <>
              {' · '}
              idea by <span className="text-turd-cream">{mod.proposedBy}</span>
            </>
          )}
        </p>
      </Link>
      <div className="border-t border-turd-bronze/20 px-5 py-3">
        <InstallButton
          slug={mod.slug}
          latestVersion={mod.version}
          installed={installed ?? null}
        />
      </div>
    </li>
  );
}
