import { useWizard } from '../WizardContext';

export function StepWelcome() {
  const { next } = useWizard();

  return (
    <div className="flex h-full flex-col">
      <div className="flex-1 space-y-4">
        <p className="text-sm leading-relaxed text-turd-cream">
          TurdMOD Manager connects to your SCUM server and handles mod
          installs, live-event reactions, and RCON admin commands — all
          from one place.
        </p>
        <p className="text-sm leading-relaxed text-turd-cream-dim">
          This wizard takes about two minutes. It sets up a SCUM install
          that lives <strong className="text-turd-cream">on this machine</strong> — your local
          game install or a dedicated server running here.
        </p>
        <div className="rounded-lg border border-turd-mustard/20 bg-turd-mustard/5 px-4 py-3">
          <p className="font-display text-[10px] uppercase tracking-wider text-turd-mustard">
            Running on a remote host? (g-portal, Nitrado, your own VPS)
          </p>
          <p className="mt-1.5 text-xs text-turd-cream-dim">
            Skip this wizard and add the server later on the{' '}
            <span className="text-turd-mustard">Servers</span> page —
            that's where remote SFTP / FTP / RCON profiles live.
          </p>
        </div>
        <div className="rounded-lg border border-turd-bronze/20 bg-turd-bg-soft/40 px-4 py-3">
          <p className="font-display text-[10px] uppercase tracking-wider text-turd-mustard">
            What you'll need
          </p>
          <ul className="mt-2 space-y-1 text-xs text-turd-cream-dim">
            <li>• SCUM installed locally (client or dedicated server)</li>
            <li>• Node.js on your system (we'll check in the last step)</li>
            <li>• RCON password, if your server runs locally (optional)</li>
          </ul>
        </div>
      </div>

      <div className="mt-6 flex justify-end">
        <button
          type="button"
          autoFocus
          onClick={next}
          className="rounded border border-turd-mustard bg-turd-bg-soft px-8 py-2.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:bg-turd-bronze/40 focus:outline-none focus:ring-2 focus:ring-turd-mustard/50"
        >
          Get started
        </button>
      </div>
    </div>
  );
}
