export function OnboardingPage() {
  return (
    <div className="mx-auto flex h-full max-w-xl flex-col justify-center px-6 py-10">
      <h1 className="text-2xl font-semibold tracking-tight">Welcome to Axen</h1>
      <p className="mt-2 text-sm text-slate-400">
        Onboarding will create or import a 12-word mnemonic, derive your
        wallet + AXL key, and mint <code className="font-mono">name.chat.eth</code>{" "}
        in a single transaction.
      </p>

      <div className="mt-8 space-y-4">
        <div className="rounded-lg border border-slate-800 bg-slate-900/40 p-4">
          <h2 className="text-sm font-medium text-slate-200">Create a new identity</h2>
          <p className="mt-1 text-xs text-slate-500">
            Generates a BIP39 mnemonic locally, encrypts it with your passphrase,
            and stores it in <code className="font-mono">vault.bin</code>.
          </p>
          <button
            type="button"
            disabled
            className="mt-3 rounded-md bg-emerald-500/90 px-3 py-1.5 text-sm font-medium text-emerald-950 disabled:cursor-not-allowed disabled:opacity-50"
            title="Wired up in a later scaffold step"
          >
            Create identity
          </button>
        </div>

        <div className="rounded-lg border border-slate-800 bg-slate-900/40 p-4">
          <h2 className="text-sm font-medium text-slate-200">Import an existing mnemonic</h2>
          <p className="mt-1 text-xs text-slate-500">
            Re-importing the same 12 words on a new device re-derives the same
            wallet + AXL peer ID — there is nothing to sync.
          </p>
          <button
            type="button"
            disabled
            className="mt-3 rounded-md border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-300 disabled:cursor-not-allowed disabled:opacity-50"
            title="Wired up in a later scaffold step"
          >
            Import mnemonic
          </button>
        </div>
      </div>
    </div>
  );
}
