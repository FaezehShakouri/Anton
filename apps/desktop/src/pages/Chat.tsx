import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useNavigate, useParams } from "react-router-dom";
import type { ChatMessage, ChatReply } from "@anton/shared-types";
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

type AgentStatus = {
  peer: string;
  status: "thinking" | "sent" | "error" | string;
  error?: string | null;
  messageId?: string | null;
  agentEnabled?: boolean | null;
  disabledUntil?: number | null;
};

type A2aTool = "draft_reply" | "send_reply" | "summarize_conversation" | "handoff_to_human";

function toReply(message: ChatMessage): ChatReply {
  return {
    id: message.id,
    from: message.from,
    text: message.text.length > 160 ? `${message.text.slice(0, 157)}...` : message.text,
  };
}

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
  const [replyTo, setReplyTo] = useState<ChatReply | null>(null);
  const [sendBusy, setSendBusy] = useState(false);
  const [ensUpdateBusy, setEnsUpdateBusy] = useState(false);
  const [ensUpdateStatus, setEnsUpdateStatus] = useState<string | null>(null);
  const [agentEnabled, setAgentEnabled] = useState(false);
  const [agentBusy, setAgentBusy] = useState(false);
  const [agentStatus, setAgentStatus] = useState<string | null>(null);
  const [a2aBusy, setA2aBusy] = useState<A2aTool | null>(null);
  const [a2aStatus, setA2aStatus] = useState<string | null>(null);
  const messageEndRef = useRef<HTMLDivElement | null>(null);

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
      setReplyTo(null);
      setAgentEnabled(false);
      setAgentStatus(null);
      setA2aStatus(null);
      return;
    }
    void (async () => {
      try {
        await ipc("chat_open", { ens: activeNorm });
        await refreshMessages(activeNorm);
        const mode = await ipc("agent_get_conversation_mode", { peer: activeNorm });
        setAgentEnabled(mode.enabled);
      } catch {
        setMessages([]);
      }
    })();
  }, [activeNorm, refreshMessages]);

  useEffect(() => {
    messageEndRef.current?.scrollIntoView({ block: "end", behavior: "smooth" });
  }, [messages, activeNorm]);

  useEffect(() => {
    const unlisten = listen<{ peer: string; message: ChatMessage }>("chat:message-received", (ev) => {
      const peer = ev.payload?.peer?.toLowerCase();
      if (!peer) return;
      setSessions((s) => (s.includes(peer) ? s : [...s, peer]));
      void (async () => {
        try {
          const opened = await ipc("chat_open", { ens: peer });
          setActiveEns(peer);
          setReplyTo(null);
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

  useEffect(() => {
    const unlisten = listen<AgentStatus>("agent:status", (ev) => {
      const payload = ev.payload;
      if (!payload?.peer || payload.peer.toLowerCase() !== activeNorm) return;
      if (payload.status === "thinking") {
        setAgentStatus("Agent is thinking…");
      } else if (payload.status === "sent") {
        setAgentStatus("Agent replied");
        void refreshMessages(payload.peer);
      } else if (payload.status === "disabled") {
        setAgentEnabled(false);
        setAgentStatus(payload.error ?? "Agent mode switched to Manual");
      } else if (payload.status === "error") {
        setAgentStatus(payload.error ?? "Agent reply failed");
      }
    });
    return () => {
      void unlisten.then((u) => u());
    };
  }, [activeNorm, refreshMessages]);

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
    setReplyTo(null);
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
      setReplyTo(null);
      navigate("/chat");
      setMessages([]);
    }
  };

  const handleSend = async () => {
    if (!activeNorm || !draft.trim()) return;
    setSendBusy(true);
    try {
      await ipc("chat_send", {
        to: activeNorm,
        text: draft.trim(),
        ...(replyTo ? { replyTo } : {}),
      });
      setDraft("");
      setReplyTo(null);
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

  const toggleAgent = async () => {
    if (!activeNorm) return;
    const next = !agentEnabled;
    setAgentBusy(true);
    setAgentStatus(null);
    try {
      const res = await ipc("agent_set_conversation_mode", { peer: activeNorm, enabled: next });
      setAgentEnabled(res.enabled);
    } catch (e) {
      setAgentStatus(e instanceof Error ? e.message : String(e));
    } finally {
      setAgentBusy(false);
    }
  };

  const formatA2aResponse = (tool: A2aTool, response: unknown): string => {
    const text =
      typeof response === "object" && response !== null
        ? (((response as { result?: { message?: { parts?: Array<{ text?: unknown }> } } }).result?.message?.parts?.[0]
            ?.text as string | undefined) ?? JSON.stringify(response))
        : String(response);
    let parsed: unknown = text;
    try {
      parsed = JSON.parse(text);
    } catch {
      // Some A2A bridges return direct text. Show it as-is.
    }
    const result =
      typeof parsed === "object" && parsed !== null && "result" in parsed
        ? (parsed as { result?: unknown }).result
        : parsed;
    if (tool === "draft_reply" && typeof result === "object" && result !== null && "text" in result) {
      return `Remote draft: ${(result as { text?: string }).text ?? ""}`;
    }
    if (tool === "summarize_conversation" && typeof result === "object" && result !== null && "summary" in result) {
      return `Remote summary: ${(result as { summary?: string }).summary ?? ""}`;
    }
    if (tool === "handoff_to_human") {
      return "Remote agent switched to human handoff.";
    }
    if (tool === "send_reply") {
      return "Remote agent sent a signed reply.";
    }
    return typeof result === "string" ? result : JSON.stringify(result);
  };

  const callA2aTool = async (tool: A2aTool, extra: Record<string, unknown> = {}) => {
    if (!activeNorm) return;
    setA2aBusy(tool);
    setA2aStatus(null);
    try {
      const res = await ipc("agent_a2a_call_tool", {
        request: {
          peer: activeNorm,
          tool,
          arguments: extra,
        },
      });
      setA2aStatus(formatA2aResponse(tool, res.response));
      if (tool === "send_reply") {
        await refreshMessages(activeNorm);
      }
    } catch (e) {
      setA2aStatus(e instanceof Error ? e.message : String(e));
    } finally {
      setA2aBusy(null);
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
            {activeNorm ? (
              <span className="w-fit rounded-full bg-slate-800 px-2 py-0.5 text-[10px] text-slate-300">
                {agentEnabled ? "Agent replies enabled" : "Manual replies"}
              </span>
            ) : null}
            {ensUpdateStatus ? <span className="text-[10px] text-emerald-400">{ensUpdateStatus}</span> : null}
            {agentStatus ? <span className="text-[10px] text-slate-400">{agentStatus}</span> : null}
          </div>
          <div className="flex shrink-0 gap-2">
            <button
              type="button"
              disabled={!activeNorm || agentBusy}
              onClick={() => void toggleAgent()}
              className={cn(
                "rounded-md border px-3 py-1.5 text-xs disabled:opacity-50",
                agentEnabled
                  ? "border-emerald-700 bg-emerald-500/10 text-emerald-300 hover:bg-emerald-500/20"
                  : "border-slate-700 text-slate-300 hover:bg-slate-900",
              )}
            >
              {agentBusy ? "Saving…" : agentEnabled ? "Agent replies" : "Manual"}
            </button>
            <button
              type="button"
              disabled={ensUpdateBusy}
              onClick={() => void handleUpdateEnsRecords()}
              className="rounded-md border border-slate-700 px-3 py-1.5 text-xs text-slate-300 hover:bg-slate-900 disabled:opacity-50"
            >
              {ensUpdateBusy ? "Updating ENS…" : "Update my ENS records"}
            </button>
          </div>
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
                {m.replyTo ? (
                  <div className="mt-2 rounded border-l-2 border-emerald-400/60 bg-black/20 px-2 py-1 text-xs text-slate-300">
                    <p className="font-mono text-[10px] text-slate-500">Reply to {m.replyTo.from}</p>
                    <p className="mt-0.5 line-clamp-2 whitespace-pre-wrap">{m.replyTo.text}</p>
                  </div>
                ) : null}
                <p className="mt-1 whitespace-pre-wrap">{m.text}</p>
                <button
                  type="button"
                  onClick={() => setReplyTo(toReply(m))}
                  className="mt-2 text-[10px] font-medium text-slate-400 hover:text-slate-100"
                >
                  Reply
                </button>
              </div>
            ))}
            <div ref={messageEndRef} />
          </div>
        </div>
        <footer className="border-t border-slate-800 p-3">
          {activeNorm ? (
            <div className="mb-3 rounded-lg border border-slate-800 bg-slate-950/60 p-3">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-xs font-medium text-slate-400">AXL A2A</span>
                <button
                  type="button"
                  disabled={a2aBusy != null}
                  onClick={() => void callA2aTool("draft_reply")}
                  className="rounded-md border border-slate-700 px-2 py-1 text-xs text-slate-300 hover:bg-slate-900 disabled:opacity-50"
                >
                  {a2aBusy === "draft_reply" ? "Drafting…" : "Remote draft"}
                </button>
                <button
                  type="button"
                  disabled={a2aBusy != null}
                  onClick={() => void callA2aTool("send_reply", draft.trim() ? { text: draft.trim() } : {})}
                  className="rounded-md border border-slate-700 px-2 py-1 text-xs text-slate-300 hover:bg-slate-900 disabled:opacity-50"
                >
                  {a2aBusy === "send_reply" ? "Sending…" : "Remote send reply"}
                </button>
                <button
                  type="button"
                  disabled={a2aBusy != null}
                  onClick={() => void callA2aTool("summarize_conversation")}
                  className="rounded-md border border-slate-700 px-2 py-1 text-xs text-slate-300 hover:bg-slate-900 disabled:opacity-50"
                >
                  {a2aBusy === "summarize_conversation" ? "Summarizing…" : "Remote summary"}
                </button>
                <button
                  type="button"
                  disabled={a2aBusy != null}
                  onClick={() => void callA2aTool("handoff_to_human", { reason: "A2A handoff requested from Anton UI." })}
                  className="rounded-md border border-slate-700 px-2 py-1 text-xs text-slate-300 hover:bg-slate-900 disabled:opacity-50"
                >
                  {a2aBusy === "handoff_to_human" ? "Handing off…" : "Remote handoff"}
                </button>
              </div>
              {a2aStatus ? <p className="mt-2 whitespace-pre-wrap text-xs text-slate-400">{a2aStatus}</p> : null}
            </div>
          ) : null}
          {replyTo ? (
            <div className="mb-2 flex items-start justify-between gap-3 rounded-md border border-slate-800 bg-slate-900/70 px-3 py-2 text-xs">
              <div className="min-w-0">
                <p className="font-mono text-slate-500">Replying to {replyTo.from}</p>
                <p className="mt-1 truncate text-slate-300">{replyTo.text}</p>
              </div>
              <button
                type="button"
                onClick={() => setReplyTo(null)}
                className="shrink-0 text-slate-500 hover:text-slate-200"
              >
                Cancel
              </button>
            </div>
          ) : null}
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
