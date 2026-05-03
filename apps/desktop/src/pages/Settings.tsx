import { useCallback, useEffect, useState } from "react";
import { ipc } from "../lib/ipc";

type AgentProvider = "open_router" | "local_open_ai";

export function SettingsPage() {
  const [topology, setTopology] = useState<{
    selfPeerId: string;
    bootstrapPeers: string[];
    connectedPeers: number;
  } | null>(null);
  const [bootstrapText, setBootstrapText] = useState("");
  const [saveMsg, setSaveMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [agentServiceName, setAgentServiceName] = useState("");
  const [serviceNameMsg, setServiceNameMsg] = useState<string | null>(null);
  const [serviceNameBusy, setServiceNameBusy] = useState(false);
  const [agentProvider, setAgentProvider] = useState<AgentProvider>("local_open_ai");
  const [agentModel, setAgentModel] = useState("Llama3");
  const [agentBaseUrl, setAgentBaseUrl] = useState("http://localhost:11434/v1");
  const [agentSystemPrompt, setAgentSystemPrompt] = useState("");
  const [agentMaxRepliesPerHour, setAgentMaxRepliesPerHour] = useState(30);
  const [agentApiKey, setAgentApiKey] = useState("");
  const [agentKeyConfigured, setAgentKeyConfigured] = useState(false);
  const [agentMsg, setAgentMsg] = useState<string | null>(null);
  const [agentBusy, setAgentBusy] = useState(false);
  const [ensSyncBusy, setEnsSyncBusy] = useState(false);
  const [ensSyncMsg, setEnsSyncMsg] = useState<string | null>(null);
  const [googleClientId, setGoogleClientId] = useState("");
  const [googleClientSecret, setGoogleClientSecret] = useState("");
  const [googleRefreshToken, setGoogleRefreshToken] = useState("");
  const [googleCalendarId, setGoogleCalendarId] = useState("primary");
  const [googleSecretConfigured, setGoogleSecretConfigured] = useState(false);
  const [googleRefreshConfigured, setGoogleRefreshConfigured] = useState(false);
  const [googleMsg, setGoogleMsg] = useState<string | null>(null);
  const [googleBusy, setGoogleBusy] = useState(false);

  const refreshTopology = useCallback(async () => {
    const t = await ipc("axl_topology");
    setTopology(t);
  }, []);

  useEffect(() => {
    void refreshTopology();
    void (async () => {
      try {
        const service = await ipc("settings_get_agent_service_name");
        setAgentServiceName(service.serviceName);
        const settings = await ipc("agent_get_settings");
        setAgentProvider(settings.provider);
        setAgentModel(settings.model);
        setAgentBaseUrl(settings.baseUrl);
        setAgentSystemPrompt(settings.systemPrompt);
        setAgentMaxRepliesPerHour(settings.maxRepliesPerHour);
        setAgentKeyConfigured(settings.apiKeyConfigured);
      } catch (e) {
        setAgentMsg(e instanceof Error ? e.message : String(e));
      }
    })();
    void (async () => {
      try {
        const settings = await ipc("agent_get_google_calendar_settings");
        setGoogleClientId(settings.clientId);
        setGoogleCalendarId(settings.calendarId || "primary");
        setGoogleSecretConfigured(settings.clientSecretConfigured);
        setGoogleRefreshConfigured(settings.refreshTokenConfigured);
      } catch (e) {
        setGoogleMsg(e instanceof Error ? e.message : String(e));
      }
    })();
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

  const saveAgentServiceName = async () => {
    setServiceNameBusy(true);
    setServiceNameMsg(null);
    try {
      const res = await ipc("settings_set_agent_service_name", {
        update: { serviceName: agentServiceName },
      });
      setAgentServiceName(res.serviceName);
      setServiceNameMsg("Saved. Use Sync ENS to publish this service name, then restart/unlock to update the local A2A advertisement.");
    } catch (e) {
      setServiceNameMsg(e instanceof Error ? e.message : String(e));
    } finally {
      setServiceNameBusy(false);
    }
  };

  const saveAgentSettings = async () => {
    setAgentBusy(true);
    setAgentMsg(null);
    try {
      const settings = await ipc("agent_update_settings", {
        settings: {
          provider: agentProvider,
          model: agentModel,
          baseUrl: agentBaseUrl,
          systemPrompt: agentSystemPrompt,
          maxRepliesPerHour: agentMaxRepliesPerHour,
          ...(agentApiKey.trim() ? { apiKey: agentApiKey.trim() } : {}),
        },
      });
      setAgentKeyConfigured(settings.apiKeyConfigured);
      setAgentApiKey("");
      setAgentMsg("Saved agent settings.");
    } catch (e) {
      setAgentMsg(e instanceof Error ? e.message : String(e));
    } finally {
      setAgentBusy(false);
    }
  };

  const testAgentProvider = async () => {
    setAgentBusy(true);
    setAgentMsg(null);
    try {
      const res = await ipc("agent_test_provider");
      setAgentMsg(res.ok ? `Provider OK: ${res.message}` : res.message);
    } catch (e) {
      setAgentMsg(e instanceof Error ? e.message : String(e));
    } finally {
      setAgentBusy(false);
    }
  };

  const syncEnsRecords = async () => {
    setEnsSyncBusy(true);
    setEnsSyncMsg(null);
    try {
      const res = await ipc("update_current_ens_records");
      setEnsSyncMsg(`Updated ${res.ens} including A2A service name.`);
    } catch (e) {
      setEnsSyncMsg(e instanceof Error ? e.message : String(e));
    } finally {
      setEnsSyncBusy(false);
    }
  };

  const saveGoogleCalendar = async () => {
    setGoogleBusy(true);
    setGoogleMsg(null);
    try {
      const settings = await ipc("agent_update_google_calendar_settings", {
        settings: {
          clientId: googleClientId,
          calendarId: googleCalendarId || "primary",
          ...(googleClientSecret.trim() ? { clientSecret: googleClientSecret.trim() } : {}),
          ...(googleRefreshToken.trim() ? { refreshToken: googleRefreshToken.trim() } : {}),
        },
      });
      setGoogleSecretConfigured(settings.clientSecretConfigured);
      setGoogleRefreshConfigured(settings.refreshTokenConfigured);
      setGoogleClientSecret("");
      setGoogleRefreshToken("");
      setGoogleMsg("Saved Google Calendar settings.");
    } catch (e) {
      setGoogleMsg(e instanceof Error ? e.message : String(e));
    } finally {
      setGoogleBusy(false);
    }
  };

  const testGoogleCalendar = async () => {
    setGoogleBusy(true);
    setGoogleMsg(null);
    try {
      const res = await ipc("agent_test_google_calendar");
      setGoogleMsg(res.ok ? res.message : `Google Calendar failed: ${res.message}`);
    } catch (e) {
      setGoogleMsg(e instanceof Error ? e.message : String(e));
    } finally {
      setGoogleBusy(false);
    }
  };

  return (
    <div className="anton-screen overflow-y-auto px-6 py-10">
      <div className="mx-auto max-w-3xl">
      <p className="text-xs font-medium uppercase tracking-[0.28em] text-emerald-300/70">Control room</p>
      <h1 className="mt-2 text-4xl font-semibold tracking-tight text-white">Settings</h1>
      <p className="mt-3 max-w-2xl text-sm leading-6 text-slate-400">
        Theme and identity tools will grow here. Bootstrap overrides are written to{" "}
        <code className="font-mono">settings.json</code> — no chat content.
      </p>

      <section className="mt-8 p-5">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h2 className="text-sm font-medium text-slate-200">Identity</h2>
            <p className="mt-1 text-xs leading-5 text-slate-500">
              Publish the latest wallet, AXL peer ID, and public key records for your current ENS subname.
            </p>
          </div>
          <button
            type="button"
            disabled={ensSyncBusy}
            onClick={() => void syncEnsRecords()}
            className="shrink-0 rounded-2xl bg-emerald-300 px-4 py-2 text-xs font-semibold text-emerald-950 transition hover:bg-emerald-200 disabled:opacity-40"
          >
            {ensSyncBusy ? "Syncing..." : "Sync ENS"}
          </button>
        </div>
        {ensSyncMsg ? <p className="mt-3 text-xs text-slate-400">{ensSyncMsg}</p> : null}
      </section>

      <section className="mt-6 p-5">
        <h2 className="text-sm font-medium text-slate-200">A2A service</h2>
        <p className="mt-1 text-xs leading-5 text-slate-500">
          Other AXL nodes use this MCP service name with your peer ID. It is published as the{" "}
          <code className="font-mono">anton_agent_service</code> ENS text record when you sync ENS.
        </p>
        <div className="mt-4 flex gap-2">
          <input
            type="text"
            value={agentServiceName}
            onChange={(e) => setAgentServiceName(e.target.value)}
            className="min-w-0 flex-1 rounded-md border border-slate-700 bg-slate-950 px-3 py-2 text-sm text-slate-100 focus:border-emerald-600 focus:outline-none"
          />
          <button
            type="button"
            disabled={serviceNameBusy}
            onClick={() => void saveAgentServiceName()}
            className="rounded-md bg-emerald-500/90 px-3 py-1.5 text-xs font-medium text-emerald-950 disabled:opacity-50"
          >
            Save service
          </button>
        </div>
        {serviceNameMsg ? <p className="mt-2 text-xs text-slate-400">{serviceNameMsg}</p> : null}
      </section>

      <section className="mt-6 p-5">
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

      <section className="mt-6 p-5">
        <h2 className="text-sm font-medium text-slate-200">Personal agent</h2>
        <p className="mt-1 text-xs text-slate-500">
          Configure the local auto-reply provider. OpenRouter uses{" "}
          <code className="font-mono">OPENROUTER_API_KEY</code> first; local models can point at any OpenAI-compatible
          LLaMA server.
        </p>
        <div className="mt-4 grid gap-3">
          <label className="block text-xs font-medium text-slate-300">
            Provider
            <select
              value={agentProvider}
              onChange={(e) => {
                const next = e.target.value as AgentProvider;
                setAgentProvider(next);
                if (next === "open_router") {
                  setAgentBaseUrl("https://openrouter.ai/api/v1");
                  setAgentModel("openai/gpt-4o-mini");
                } else {
                  setAgentBaseUrl("http://localhost:11434/v1");
                  setAgentModel("Llama3");
                }
              }}
              className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 text-sm text-slate-100 focus:border-emerald-600 focus:outline-none"
            >
              <option value="open_router">OpenRouter</option>
              <option value="local_open_ai">Local OpenAI-compatible</option>
            </select>
          </label>
          <label className="block text-xs font-medium text-slate-300">
            Model
            <input
              type="text"
              value={agentModel}
              onChange={(e) => setAgentModel(e.target.value)}
              className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 text-sm text-slate-100 focus:border-emerald-600 focus:outline-none"
            />
          </label>
          <label className="block text-xs font-medium text-slate-300">
            Base URL
            <input
              type="text"
              value={agentBaseUrl}
              onChange={(e) => setAgentBaseUrl(e.target.value)}
              className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 text-sm text-slate-100 focus:border-emerald-600 focus:outline-none"
            />
          </label>
          <label className="block text-xs font-medium text-slate-300">
            API key {agentKeyConfigured ? <span className="text-slate-500">(saved)</span> : null}
            <input
              type="password"
              value={agentApiKey}
              onChange={(e) => setAgentApiKey(e.target.value)}
              placeholder={agentProvider === "open_router" ? "Uses OPENROUTER_API_KEY if blank" : "Optional"}
              className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 text-sm text-slate-100 focus:border-emerald-600 focus:outline-none"
            />
          </label>
          <label className="block text-xs font-medium text-slate-300">
            Max agent replies per hour per chat
            <input
              type="number"
              min={1}
              max={500}
              value={agentMaxRepliesPerHour}
              onChange={(e) => setAgentMaxRepliesPerHour(Number(e.target.value) || 1)}
              className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 text-sm text-slate-100 focus:border-emerald-600 focus:outline-none"
            />
          </label>
          <label className="block text-xs font-medium text-slate-300">
            System prompt
            <textarea
              value={agentSystemPrompt}
              onChange={(e) => setAgentSystemPrompt(e.target.value)}
              rows={4}
              className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 text-sm text-slate-100 focus:border-emerald-600 focus:outline-none"
            />
          </label>
        </div>
        <div className="mt-3 flex gap-2">
          <button
            type="button"
            disabled={agentBusy}
            onClick={() => void saveAgentSettings()}
            className="rounded-md bg-emerald-500/90 px-3 py-1.5 text-xs font-medium text-emerald-950 disabled:opacity-50"
          >
            Save agent settings
          </button>
          <button
            type="button"
            disabled={agentBusy}
            onClick={() => void testAgentProvider()}
            className="rounded-md border border-slate-600 px-3 py-1.5 text-xs text-slate-200 hover:bg-slate-900 disabled:opacity-50"
          >
            Test provider
          </button>
        </div>
        {agentMsg ? <p className="mt-2 text-xs text-slate-400">{agentMsg}</p> : null}
      </section>

      <section className="mt-6 p-5">
        <h2 className="text-sm font-medium text-slate-200">Google Calendar</h2>
        <p className="mt-1 text-xs leading-5 text-slate-500">
          Used by calendar proposal skills to check FreeBusy and write events only after you accept a draft.
          Create an OAuth client in Google Cloud and paste a refresh token with Calendar access.
        </p>
        <div className="mt-4 grid gap-3">
          <label className="block text-xs font-medium text-slate-300">
            OAuth client ID
            <input
              type="text"
              value={googleClientId}
              onChange={(e) => setGoogleClientId(e.target.value)}
              className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 text-sm text-slate-100 focus:border-emerald-600 focus:outline-none"
            />
          </label>
          <label className="block text-xs font-medium text-slate-300">
            OAuth client secret {googleSecretConfigured ? <span className="text-slate-500">(saved)</span> : null}
            <input
              type="password"
              value={googleClientSecret}
              onChange={(e) => setGoogleClientSecret(e.target.value)}
              placeholder={googleSecretConfigured ? "Leave blank to keep saved secret" : ""}
              className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 text-sm text-slate-100 focus:border-emerald-600 focus:outline-none"
            />
          </label>
          <label className="block text-xs font-medium text-slate-300">
            Refresh token {googleRefreshConfigured ? <span className="text-slate-500">(saved)</span> : null}
            <input
              type="password"
              value={googleRefreshToken}
              onChange={(e) => setGoogleRefreshToken(e.target.value)}
              placeholder={googleRefreshConfigured ? "Leave blank to keep saved token" : ""}
              className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 text-sm text-slate-100 focus:border-emerald-600 focus:outline-none"
            />
          </label>
          <label className="block text-xs font-medium text-slate-300">
            Calendar ID
            <input
              type="text"
              value={googleCalendarId}
              onChange={(e) => setGoogleCalendarId(e.target.value)}
              placeholder="primary"
              className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 text-sm text-slate-100 focus:border-emerald-600 focus:outline-none"
            />
          </label>
        </div>
        <div className="mt-3 flex gap-2">
          <button
            type="button"
            disabled={googleBusy}
            onClick={() => void saveGoogleCalendar()}
            className="rounded-md bg-emerald-500/90 px-3 py-1.5 text-xs font-medium text-emerald-950 disabled:opacity-50"
          >
            Save calendar access
          </button>
          <button
            type="button"
            disabled={googleBusy}
            onClick={() => void testGoogleCalendar()}
            className="rounded-md border border-slate-600 px-3 py-1.5 text-xs text-slate-200 hover:bg-slate-900 disabled:opacity-50"
          >
            Test calendar
          </button>
        </div>
        {googleMsg ? <p className="mt-2 text-xs text-slate-400">{googleMsg}</p> : null}
      </section>

      <section className="mt-6 p-5">
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

      <section className="mt-6 p-5">
        <h2 className="text-sm font-medium text-slate-200">Privacy</h2>
        <p className="mt-2 text-xs text-slate-500">
          Chat is ephemeral by design — nothing on this page persists message bodies to disk.
        </p>
      </section>
      </div>
    </div>
  );
}
