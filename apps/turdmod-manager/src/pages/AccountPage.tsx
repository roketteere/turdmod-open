import { useState, type FormEvent, type ReactNode } from 'react';
import { useAccount } from '../hooks/useAccount';

export function AccountPage() {
  const account = useAccount();

  return (
    <div className="mx-auto flex max-w-2xl flex-col gap-6">
      <header>
        <h1 className="font-display text-3xl tracking-wide text-turd-cream">
          Account
        </h1>
        <p className="mt-1 text-sm text-turd-cream-dim">
          Connect this manager to your turdmod.com account so premium mods,
          purchases, and creator tools light up.
        </p>
      </header>

      {account.status === 'unknown' ? (
        <Loading />
      ) : account.status === 'signed-in' && account.user ? (
        <SignedInCard
          user={account.user}
          onSignOut={() => void account.signOut()}
          isRefreshing={account.isRefreshing}
        />
      ) : (
        <SignedOutCard
          onOpenBrowser={() => void account.signIn()}
          onPaste={(token) => account.pasteToken.mutateAsync(token)}
          pasteError={account.pasteToken.error?.message ?? null}
          isPasting={account.pasteToken.isPending}
        />
      )}

      {account.error && account.status !== 'signed-in' && (
        <p className="rounded glass p-3 text-xs text-turd-cream-dim">
          Could not reach the account service: {account.error.message}
        </p>
      )}
    </div>
  );
}

function Loading() {
  return (
    <div className="glass rounded-xl p-6">
      <p className="text-sm text-turd-cream-dim">Checking saved credentials…</p>
    </div>
  );
}

interface SignedInCardProps {
  user: NonNullable<ReturnType<typeof useAccount>['user']>;
  onSignOut: () => void;
  isRefreshing: boolean;
}

function SignedInCard({ user, onSignOut, isRefreshing }: SignedInCardProps) {
  const display = user.displayName ?? user.username ?? 'Survivor';
  const initials = display
    .split(/\s+/)
    .map((p) => p[0])
    .filter(Boolean)
    .slice(0, 2)
    .join('')
    .toUpperCase();

  return (
    <section className="glass rounded-xl p-6 shadow-turd-glow">
      <div className="flex items-center gap-4">
        {user.avatarUrl ? (
          <img
            src={user.avatarUrl}
            alt=""
            className="h-16 w-16 rounded-full border border-turd-bronze/40 object-cover"
          />
        ) : (
          <div className="flex h-16 w-16 items-center justify-center rounded-full border border-turd-bronze/40 bg-turd-bg-soft font-display text-xl text-turd-mustard-bright">
            {initials || '??'}
          </div>
        )}
        <div className="min-w-0 flex-1">
          <p className="font-display text-xl text-turd-cream">{display}</p>
          {user.username && user.username !== display && (
            <p className="text-xs text-turd-cream-dim">@{user.username}</p>
          )}
          {user.email && (
            <p className="mt-0.5 truncate text-xs text-turd-cream-dim">
              {user.email}
            </p>
          )}
        </div>
        <span
          className="rounded bg-turd-bg-soft px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider text-turd-mustard"
          aria-live="polite"
        >
          {isRefreshing ? 'Syncing' : 'Connected'}
        </span>
      </div>

      <dl className="mt-6 grid grid-cols-2 gap-x-6 gap-y-3 text-xs">
        <Field label="Account ID" value={user.id?.toString() ?? '—'} />
        <Field label="Discord ID" value={user.discordId ?? '—'} mono />
      </dl>

      <div className="mt-6 flex justify-end">
        <button
          type="button"
          onClick={onSignOut}
          className="rounded border border-turd-bronze/50 px-4 py-2 font-display text-xs uppercase tracking-widest text-turd-cream-dim transition-colors hover:border-turd-mustard hover:text-turd-mustard-bright"
        >
          Sign out
        </button>
      </div>
    </section>
  );
}

function Field({
  label,
  value,
  mono,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div>
      <dt className="text-[10px] uppercase tracking-widest text-turd-cream-dim">
        {label}
      </dt>
      <dd
        className={`mt-1 truncate text-turd-cream ${mono ? 'font-mono' : ''}`}
      >
        {value}
      </dd>
    </div>
  );
}

interface SignedOutCardProps {
  onOpenBrowser: () => void;
  onPaste: (token: string) => Promise<unknown>;
  pasteError: string | null;
  isPasting: boolean;
}

function SignedOutCard({
  onOpenBrowser,
  onPaste,
  pasteError,
  isPasting,
}: SignedOutCardProps) {
  const [token, setToken] = useState('');

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const trimmed = token.trim();
    if (!trimmed) return;
    try {
      await onPaste(trimmed);
      setToken('');
    } catch {
      // useMutation tracks the error; no extra UI here.
    }
  };

  return (
    <section className="glass rounded-xl p-6 shadow-turd-glow">
      <h2 className="font-display text-lg tracking-wide text-turd-mustard-bright">
        Connect manager to turdmod.com
      </h2>
      <p className="mt-2 text-sm text-turd-cream-dim">
        Sign in once. The manager keeps your token in the OS keychain so you
        don&apos;t see this screen again.
      </p>

      <ol className="mt-5 space-y-3 text-sm text-turd-cream-dim">
        <Step n={1}>
          <button
            type="button"
            onClick={onOpenBrowser}
            className="rounded bg-turd-mustard px-4 py-2 font-display text-xs uppercase tracking-widest text-turd-bg-deep transition-colors hover:bg-turd-mustard-bright"
          >
            Open turdmod.com sign-in
          </button>
        </Step>
        <Step n={2}>
          Sign in with Discord, then copy the one-time code shown on the
          Account page.
        </Step>
        <Step n={3}>Paste the code below.</Step>
      </ol>

      <form onSubmit={submit} className="mt-6 space-y-3">
        <label
          htmlFor="account-token"
          className="block text-[10px] uppercase tracking-widest text-turd-cream-dim"
        >
          One-time code
        </label>
        <div className="flex gap-2">
          <input
            id="account-token"
            type="text"
            spellCheck={false}
            autoComplete="off"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            placeholder="Paste code here"
            className="flex-1 rounded border border-turd-bronze/40 bg-turd-bg-deep px-3 py-2 font-mono text-sm text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none"
          />
          <button
            type="submit"
            disabled={isPasting || token.trim().length === 0}
            className="rounded bg-turd-bronze px-4 py-2 font-display text-xs uppercase tracking-widest text-turd-cream transition-colors hover:bg-turd-mustard hover:text-turd-bg-deep disabled:cursor-not-allowed disabled:opacity-50"
          >
            {isPasting ? 'Linking…' : 'Link'}
          </button>
        </div>
        {pasteError && (
          <p className="text-xs text-turd-red" role="alert">
            {pasteError}
          </p>
        )}
      </form>
    </section>
  );
}

function Step({ n, children }: { n: number; children: ReactNode }) {
  return (
    <li className="flex items-start gap-3">
      <span className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full border border-turd-bronze/50 font-mono text-[10px] text-turd-mustard">
        {n}
      </span>
      <div className="flex-1">{children}</div>
    </li>
  );
}
