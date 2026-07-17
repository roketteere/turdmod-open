export function WelcomePage() {
  return (
    <div className="mx-auto max-w-2xl space-y-4">
      <h1 className="font-display text-2xl tracking-widest text-turd-mustard-bright">
        Welcome to TurdMOD Lite
      </h1>
      <p className="text-sm leading-relaxed text-turd-cream">
        TurdMOD Lite is the free, open-source SCUM admin client for managed-host
        servers — G-Portal, Nitrado, Host Havoc, and any other host that exposes{' '}
        <code className="font-mono text-turd-bronze-light">/SCUM/Saved/</code> over
        FTP and opens an RCON port.
      </p>
      <p className="text-sm leading-relaxed text-turd-cream">
        Connect to a server in the Servers tab to push config edits
        (Notifications, Server Settings, Economy, Raid Times) and run RCON commands
        live. For full engine-tier modding (custom widgets, real-time reflection,
        UFunction calls), see{' '}
        <a
          href="https://turdmod.com/pro"
          className="text-turd-mustard-bright underline hover:text-turd-mustard"
        >
          TurdMOD Pro
        </a>
        .
      </p>
      <div className="rounded border border-turd-bronze/30 bg-turd-bg-mid/40 p-4 text-xs text-turd-cream-dim">
        <p className="mb-2 font-medium text-turd-cream">Status: scaffold</p>
        <p>
          This is the initial scaffold (2026-05-18). The Servers, ServerSettings,
          Notifications, EconomyOverride, RaidTimes, and Console pages will be
          ported here from the Admin codebase as the Lite product spins up.
        </p>
      </div>
    </div>
  );
}
