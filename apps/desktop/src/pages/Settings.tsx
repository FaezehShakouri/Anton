export function SettingsPage() {
  return (
    <div className="mx-auto max-w-2xl px-6 py-10">
      <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>
      <p className="mt-2 text-sm text-slate-400">
        Theme, last-used username, and advanced bootstrap-peer overrides will
        live here. None of these contain chat content — they're written to
        <code className="font-mono"> settings.json</code> in the app data dir.
      </p>

      <div className="mt-8 space-y-4">
        <SettingsCard title="Identity">
          <p className="text-xs text-slate-500">
            Change passphrase and export mnemonic — both gated by re-auth.
          </p>
        </SettingsCard>
        <SettingsCard title="Network">
          <p className="text-xs text-slate-500">
            Bundled <code className="font-mono">axl</code> sidecar status,
            advanced bootstrap-peer overrides, and topology debug.
          </p>
        </SettingsCard>
        <SettingsCard title="Privacy">
          <p className="text-xs text-slate-500">
            Chat is ephemeral by design — nothing on this page persists chat
            content, contacts, or message metadata to disk.
          </p>
        </SettingsCard>
      </div>
    </div>
  );
}

function SettingsCard({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-lg border border-slate-800 bg-slate-900/40 p-4">
      <h2 className="text-sm font-medium text-slate-200">{title}</h2>
      <div className="mt-2">{children}</div>
    </section>
  );
}
