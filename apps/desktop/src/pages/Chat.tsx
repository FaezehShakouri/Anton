import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useNavigate, useParams } from "react-router-dom";
import type { ChatMessage } from "@anton/shared-types";
import { ipc } from "../lib/ipc";
import { cn } from "../lib/cn";

type Resolved = {
  ens: string;
  wallet: string;
  peerId: string;
  pubkeyPem: string;
  avatar?: string;
  description?: string;
};

export function ChatPage() {
  const { ens: routeEns } = useParams<{ ens: string }>();
  const navigate = useNavigate();

  const [query, setQuery] = useState("");
  const [resolved, setResolved] = useState<Resolved | null>(null);
  const [resolveError, setResolveError] = useState<string | null>(null);
  const [resolving, setResolving] = useState(false);

  const [sessions, setSessions] = useState<string[]>([]);
  const [activeEns, setActiveEns] = useState<string | null>(routeEns ?? null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [sendBusy, setSendBusy] = useState(false);
  const [ensUpdateBusy, setEnsUpdateBusy] = useState(false);
  const [ensUpdateStatus, setEnsUpdateStatus] = useState<string | null>(null);

  const activeNorm = useMemo(
    () => (activeEns ? activeEns.trim().toLowerCase() : null),
    [activeEns],
  );

  const refreshMessages = useCallback(async (ensKey: string) => {
    const list = await ipc("chat_history", { ens: ensKey });
    setMessages(list);
  }, []);

  useEffect(() => {
    if (routeEns) {
      const n = routeEns.trim().toLowerCase();
      setActiveEns(n);
      setSessions((s) => (s.includes(n) ? s : [...s, n]));
    }
  }, [routeEns]);

  useEffect(() => {
    if (!activeNorm) {
      setMessages([]);
      return;
    }
    void (async () => {
      try {
        await ipc("chat_open", { ens: activeNorm });
        await refreshMessages(activeNorm);
      } catch {
        setMessages([]);
      }
    })();
  }, [activeNorm, refreshMessages]);

  useEffect(() => {
    const unlisten = listen<{ peer: string; message: ChatMessage }>("chat:message-received", (ev) => {
      const peer = ev.payload?.peer?.toLowerCase();
      if (!peer) return;
      setSessions((s) => (s.includes(peer) ? s : [...s, peer]));
      void (async () => {
        try {
          const opened = await ipc("chat_open", { ens: peer });
          setActiveEns(peer);
          navigate(`/chat/${encodeURIComponent(peer)}`);
          setMessages(opened.messages);
        } catch {
          if (activeNorm === peer) {
            await refreshMessages(peer);
          }
        }
      })();
    });
    return () => {
      void unlisten.then((u) => u());
    };
  }, [activeNorm, navigate, refreshMessages]);

  const handleResolve = async () => {
    const name = query.trim();
    if (!name) return;
    setResolving(true);
    setResolveError(null);
    setResolved(null);
    try {
      console.debug("[Chat] ens_resolve request", name);
      const id = await ipc("ens_resolve", { name });
      setResolved(id as Resolved);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      console.warn("[Chat] ens_resolve failed", name, msg);
      setResolveError(msg);
    } finally {
      setResolving(false);
    }
  };

  const openConversation = async (ens: string) => {
    const n = ens.trim().toLowerCase();
    setSessions((s) => (s.includes(n) ? s : [...s, n]));
    setActiveEns(n);
    navigate(`/chat/${encodeURIComponent(n)}`);
    await ipc("chat_open", { ens: n });
    await refreshMessages(n);
  };

  const closeConversation = async (ens: string) => {
    const n = ens.trim().toLowerCase();
    await ipc("chat_close", { ens: n });
    setSessions((s) => s.filter((x) => x !== n));
    if (activeNorm === n) {
      setActiveEns(null);
      navigate("/chat");
      setMessages([]);
    }
  };

  const handleSend = async () => {
    if (!activeNorm || !draft.trim()) return;
    setSendBusy(true);
    try {
      await ipc("chat_send", { to: activeNorm, text: draft.trim() });
      setDraft("");
      await refreshMessages(activeNorm);
    } catch (e) {
      await refreshMessages(activeNorm);
      setResolveError(e instanceof Error ? e.message : String(e));
    } finally {
      setSendBusy(false);
    }
  };

  const handleUpdateEnsRecords = async () => {
    setEnsUpdateBusy(true);
    setEnsUpdateStatus(null);
    setResolveError(null);
    try {
      const res = await ipc("update_current_ens_records");
      setEnsUpdateStatus(`Updated ${res.ens}`);
    } catch (e) {
      setResolveError(e instanceof Error ? e.message : String(e));
    } finally {
      setEnsUpdateBusy(false);
    }
  };

  return (
    <div className="grid h-full grid-cols-[18rem_1fr]">
      <aside className="flex flex-col border-r border-slate-800 bg-slate-950/40">
        <div className="border-b border-slate-800 p-3">
          <p className="mb-2 text-xs font-medium text-slate-400">New conversation</p>
          <div className="flex gap-1">
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && void handleResolve()}
              placeholder="alice.anton.eth"
              className="min-w-0 flex-1 rounded-md border border-slate-800 bg-slate-900 px-2 py-1.5 text-xs placeholder:text-slate-500 focus:border-emerald-500 focus:outline-none"
            />
            <button
              type="button"
              disabled={resolving || !query.trim()}
              onClick={() => void handleResolve()}
              className="shrink-0 rounded-md bg-slate-800 px-2 py-1.5 text-xs text-slate-200 hover:bg-slate-700 disabled:opacity-50"
            >
              Resolve
            </button>
          </div>
          {resolveError ? <p className="mt-2 text-xs text-red-400">{resolveError}</p> : null}
          {resolved ? (
            <div className="mt-3 rounded-md border border-slate-800 bg-slate-900/60 p-2 text-xs">
              <p className="font-mono text-slate-200">{resolved.ens}</p>
              <p className="mt-1 break-all text-slate-500">wallet {resolved.wallet}</p>
              <p className="mt-1 break-all text-slate-500">peer {resolved.peerId}</p>
              <button
                type="button"
                onClick={() => void openConversation(resolved.ens)}
                className="mt-2 w-full rounded-md bg-emerald-500/90 py-1.5 text-xs font-medium text-emerald-950"
              >
                Open chat
              </button>
            </div>
          ) : null}
        </div>
        <div className="flex-1 overflow-auto px-2 py-2">
          <p className="px-1 pb-2 text-[10px] uppercase tracking-wide text-slate-500">This session</p>
          {sessions.length === 0 ? (
            <p className="px-2 text-xs text-slate-500">No open conversations.</p>
          ) : (
            <ul className="space-y-1">
              {sessions.map((s) => (
                <li key={s}>
                  <div className="flex items-center gap-1">
                    <button
                      type="button"
                      onClick={() => void openConversation(s)}
                      className={cn(
                        "min-w-0 flex-1 truncate rounded-md px-2 py-1.5 text-left text-xs font-mono",
                        activeNorm === s ? "bg-slate-800 text-white" : "text-slate-400 hover:bg-slate-900",
                      )}
                    >
                      {s}
                    </button>
                    <button
                      type="button"
                      title="Close"
                      onClick={() => void closeConversation(s)}
                      className="shrink-0 rounded px-1.5 text-xs text-slate-500 hover:bg-slate-900 hover:text-slate-300"
                    >
                      ×
                    </button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </div>
      </aside>
      <section className="flex min-w-0 flex-col">
        <header className="flex items-center justify-between border-b border-slate-800 px-4 py-3">
          <div className="flex min-w-0 flex-col gap-1">
            <span className="truncate font-mono text-sm text-slate-200">
              {activeNorm ?? "(no conversation open)"}
            </span>
            {activeNorm ? (
              <span className="w-fit rounded-full bg-emerald-500/10 px-2 py-0.5 text-[10px] text-emerald-400">
                ENS + wallet signature on receive
              </span>
            ) : null}
            {ensUpdateStatus ? <span className="text-[10px] text-emerald-400">{ensUpdateStatus}</span> : null}
          </div>
          <button
            type="button"
            disabled={ensUpdateBusy}
            onClick={() => void handleUpdateEnsRecords()}
            className="shrink-0 rounded-md border border-slate-700 px-3 py-1.5 text-xs text-slate-300 hover:bg-slate-900 disabled:opacity-50"
          >
            {ensUpdateBusy ? "Updating ENS…" : "Update my ENS records"}
          </button>
        </header>
        <div className="flex flex-1 flex-col overflow-hidden">
          <div className="flex-1 space-y-2 overflow-y-auto px-4 py-3">
            {messages.map((m) => (
              <div
                key={m.id}
                className={cn(
                  "max-w-[85%] rounded-lg px-3 py-2 text-sm",
                  m.from.toLowerCase() === activeNorm ? "ml-auto bg-emerald-900/40 text-emerald-50" : "bg-slate-800/80 text-slate-100",
                )}
              >
                <p className="text-xs text-slate-500">
                  {m.state} · {new Date(Number(m.ts)).toLocaleString()}
                </p>
                <p className="mt-1 whitespace-pre-wrap">{m.text}</p>
              </div>
            ))}
          </div>
        </div>
        <footer className="border-t border-slate-800 p-3">
          <div className="flex gap-2">
            <input
              type="text"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && !e.shiftKey && void handleSend()}
              disabled={!activeNorm || sendBusy}
              placeholder={activeNorm ? "Message…" : "Open a conversation first"}
              className="flex-1 rounded-md border border-slate-800 bg-slate-900 px-3 py-2 text-sm placeholder:text-slate-500 focus:border-emerald-500 focus:outline-none disabled:opacity-50"
            />
            <button
              type="button"
              disabled={!activeNorm || sendBusy || !draft.trim()}
              onClick={() => void handleSend()}
              className="rounded-md bg-emerald-500/90 px-4 py-2 text-sm font-medium text-emerald-950 disabled:opacity-50"
            >
              Send
            </button>
          </div>
        </footer>
      </section>
    </div>
  );
}
