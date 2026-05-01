import { useCallback, useEffect, useState } from "react";
import { ipc } from "../lib/ipc";

export function SettingsPage() {
  const [topology, setTopology] = useState<{
    selfPeerId: string;
    bootstrapPeers: string[];
    connectedPeers: number;
  } | null>(null);
  const [bootstrapText, setBootstrapText] = useState("");
  const [saveMsg, setSaveMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refreshTopology = useCallback(async () => {
    const t = await ipc("axl_topology");
    setTopology(t);
  }, []);

  useEffect(() => {
    void refreshTopology();
    const id = window.setInterval(() => void refreshTopology(), 10_000);
    return () => window.clearInterval(id);
  }, [refreshTopology]);

  const saveBootstrap = async () => {
    setBusy(true);
    setSaveMsg(null);
    try {
      const lines = bootstrapText
        .split("\n")
        .map((l) => l.trim())
        .filter(Boolean);
      await ipc("settings_set_bootstrap_peers", { peers: lines });
      setSaveMsg("Saved. Restart the sidecar (unlock again) to apply merged bootstrap list.");
      await refreshTopology();
    } catch (e) {
      setSaveMsg(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mx-auto max-w-2xl px-6 py-10">
      <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>
      <p className="mt-2 text-sm text-slate-400">
        Theme and identity tools will grow here. Bootstrap overrides are written to{" "}
        <code className="font-mono">settings.json</code> — no chat content.
      </p>

      <section className="mt-8 rounded-lg border border-slate-800 bg-slate-900/40 p-4">
        <h2 className="text-sm font-medium text-slate-200">AXL sidecar</h2>
        {topology ? (
          <dl className="mt-3 space-y-2 font-mono text-xs text-slate-400">
            <div>
              <dt className="text-slate-500">self peer</dt>
              <dd className="break-all text-slate-300">{topology.selfPeerId}</dd>
            </div>
            <div>
              <dt className="text-slate-500">connected peers</dt>
              <dd>{topology.connectedPeers}</dd>
            </div>
            <div>
              <dt className="text-slate-500">bootstrap (runtime)</dt>
              <dd className="whitespace-pre-wrap break-all">{topology.bootstrapPeers.join("\n") || "—"}</dd>
            </div>
          </dl>
        ) : (
          <p className="mt-2 text-xs text-slate-500">Sidecar not running yet — finish onboarding / unlock first.</p>
        )}
        <button
          type="button"
          onClick={() => void refreshTopology()}
          className="mt-3 rounded-md border border-slate-600 px-3 py-1.5 text-xs text-slate-200 hover:bg-slate-900"
        >
          Refresh topology
        </button>
      </section>

      <section className="mt-6 rounded-lg border border-slate-800 bg-slate-900/40 p-4">
        <h2 className="text-sm font-medium text-slate-200">Bootstrap peer overrides</h2>
        <p className="mt-1 text-xs text-slate-500">
          One <code className="font-mono">tls://host:9001</code> per line. Merged after ENS{" "}
          <code className="font-mono">anton.eth</code> → <code className="font-mono">axl_bootstrap_peers</code> using the same
          ENS RPC as <code className="font-mono">ENS_RPC_URL</code> / <code className="font-mono">ENS_NETWORK</code> (see README).
        </p>
        <textarea
          value={bootstrapText}
          onChange={(e) => setBootstrapText(e.target.value)}
          rows={5}
          placeholder={"tls://your-node:9001"}
          className="mt-3 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 font-mono text-xs text-slate-100 focus:border-emerald-600 focus:outline-none"
        />
        <button
          type="button"
          disabled={busy}
          onClick={() => void saveBootstrap()}
          className="mt-2 rounded-md bg-emerald-500/90 px-3 py-1.5 text-xs font-medium text-emerald-950 disabled:opacity-50"
        >
          Save overrides
        </button>
        {saveMsg ? <p className="mt-2 text-xs text-slate-400">{saveMsg}</p> : null}
      </section>

      <section className="mt-6 rounded-lg border border-slate-800 bg-slate-900/40 p-4">
        <h2 className="text-sm font-medium text-slate-200">Privacy</h2>
        <p className="mt-2 text-xs text-slate-500">
          Chat is ephemeral by design — nothing on this page persists message bodies to disk.
        </p>
      </section>
    </div>
  );
}
