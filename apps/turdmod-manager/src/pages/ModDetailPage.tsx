import { Link, useParams } from 'react-router-dom';
import { InstallButton } from '../components/InstallButton';
import { PriceBadge } from '../components/PriceBadge';
import { useInstalledMods, useMod } from '../hooks/useMods';

export function ModDetailPage() {
  const params = useParams<{ slug: string }>();
  const slug = params.slug;
  const modQuery = useMod(slug);
  const installedQuery = useInstalledMods();
  const installed = (installedQuery.data ?? []).find((m) => m.slug === slug) ?? null;

  return (
    <div className="space-y-8">
      <Link
        to="/"
        className="inline-block text-sm text-turd-mustard transition-colors hover:text-turd-mustard-bright"
      >
        ← All mods
      </Link>

      {modQuery.isLoading && (
        <p className="py-12 text-center text-sm text-turd-cream-dim">Loading…</p>
      )}

      {modQuery.isError && (
        <div className="rounded-lg border border-turd-red/40 bg-turd-bg-mid/40 p-6">
          <h2 className="font-display text-lg text-turd-red">
            Couldn't load this mod.
          </h2>
          <p className="mt-2 text-sm text-turd-cream-dim">
            {modQuery.error.message}
          </p>
          <button
            type="button"
            onClick={() => modQuery.refetch()}
            className="mt-4 rounded border border-turd-bronze px-4 py-1.5 text-sm text-turd-cream transition-colors hover:border-turd-mustard-bright hover:text-turd-mustard-bright"
          >
            Retry
          </button>
        </div>
      )}

      {modQuery.isSuccess && (() => {
        const mod = modQuery.data;
        return (
          <>
            <section className="glass rounded-xl p-6 md:p-8">
              <div className="flex items-center gap-3">
                <span className="rounded bg-turd-bg-soft px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider text-turd-mustard">
                  {mod.tag}
                </span>
                {mod.version && (
                  <span className="font-mono text-[10px] text-turd-cream-dim">
                    v{mod.version}
                  </span>
                )}
                <PriceBadge
                  isPremium={mod.isPremium}
                  isPremiumBundled={mod.isPremiumBundled}
                  priceCents={mod.priceCents}
                />
              </div>
              <h1 className="mt-3 font-display text-4xl text-turd-cream md:text-5xl">
                {mod.name}
              </h1>
              <p className="mt-4 max-w-3xl text-base text-turd-cream-dim">
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
            </section>

            <div className="grid gap-6 md:grid-cols-3">
              <div className="space-y-6 md:col-span-2">
                <section className="glass rounded-xl p-5">
                  <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
                    About
                  </h2>
                  <p className="mt-3 whitespace-pre-line text-sm leading-relaxed text-turd-cream-dim">
                    {mod.description || mod.summary}
                  </p>
                </section>
              </div>

              <aside className="space-y-6">
                <section className="glass rounded-xl p-5">
                  <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
                    {installed ? 'Manage' : 'Install'}
                  </h2>
                  <div className="mt-3">
                    <InstallButton
                      slug={mod.slug}
                      latestVersion={mod.version}
                      installed={installed}
                      variant="full"
                    />
                  </div>
                  {installed && mod.version && installed.version !== mod.version && (
                    <p className="mt-3 text-xs text-turd-mustard-bright">
                      You have v{installed.version} installed. v{mod.version} is
                      available.
                    </p>
                  )}
                </section>

                <section className="glass rounded-xl p-5">
                  <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
                    Quick facts
                  </h2>
                  <dl className="mt-3 space-y-3 text-sm">
                    <Fact label="Author" value={mod.author?.displayName ?? 'TurdMOD'} />
                    {mod.proposedBy && (
                      <Fact label="Idea credit" value={mod.proposedBy} />
                    )}
                    <Fact label="Category" value={mod.category} />
                    {mod.version && (
                      <Fact
                        label="Version"
                        value={`v${mod.version}`}
                        mono
                      />
                    )}
                    <Fact
                      label="Premium"
                      value={
                        mod.isPremiumBundled
                          ? 'Bundled with Premium'
                          : mod.isPremium
                            ? 'Paid'
                            : 'Free'
                      }
                    />
                    {mod.battleyePolicy && (
                      <Fact
                        label="BattlEye policy"
                        value={mod.battleyePolicy}
                      />
                    )}
                  </dl>
                </section>
              </aside>
            </div>
          </>
        );
      })()}
    </div>
  );
}

function Fact({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div>
      <dt className="font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim">
        {label}
      </dt>
      <dd className={`mt-1 text-turd-cream ${mono ? 'font-mono' : ''}`}>{value}</dd>
    </div>
  );
}
