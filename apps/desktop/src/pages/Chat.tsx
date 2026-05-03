import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
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

const A2A_SKILLS: ReadonlyArray<{ tool: A2aTool; label: string; description: string; accent: string }> = [
  {
    tool: "draft_reply",
    label: "Remote draft",
    description: "Ask the peer agent to draft a reply without sending.",
    accent: "from-cyan-300 to-emerald-300",
  },
  {
    tool: "send_reply",
    label: "Remote send",
    description: "Have the peer agent send a signed message back.",
    accent: "from-violet-300 to-cyan-300",
  },
  {
    tool: "summarize_conversation",
    label: "Summary",
    description: "Request a concise summary from the remote agent.",
    accent: "from-amber-200 to-orange-300",
  },
  {
    tool: "handoff_to_human",
    label: "Handoff",
    description: "Ask the agent to stop and wait for the human.",
    accent: "from-rose-300 to-violet-300",
  },
];

function toReply(message: ChatMessage): ChatReply {
  return {
    id: message.id,
    from: message.from,
    text: message.text.length > 160 ? `${message.text.slice(0, 157)}...` : message.text,
  };
}

function friendlyResolveError(name: string, raw: string): string {
  const lower = raw.toLowerCase();
  if (
    lower.includes("missing") ||
    lower.includes("not found") ||
    lower.includes("no resolver") ||
    lower.includes("ens")
  ) {
    return `Could not find ${name}. Check the subname spelling or register it first.`;
  }
  return "Could not resolve this name right now. Check the subname and try again.";
}

function conversationInitial(name: string): string {
  return name.trim().charAt(0).toUpperCase() || "A";
}

function shortEns(name: string): string {
  return name.replace(/\.anton\.eth$/i, "");
}

export function ChatPage() {
  const { ens: routeEns } = useParams<{ ens: string }>();
  const navigate = useNavigate();

  const [query, setQuery] = useState("");
  const [resolved, setResolved] = useState<Resolved | null>(null);
  const [resolveError, setResolveError] = useState<string | null>(null);
  const [resolving, setResolving] = useState(false);
  const [currentUserEns, setCurrentUserEns] = useState<string | null>(null);
  const [conversationPanelOpen, setConversationPanelOpen] = useState(!routeEns);

  const [sessions, setSessions] = useState<string[]>([]);
  const [activeEns, setActiveEns] = useState<string | null>(routeEns ?? null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [replyTo, setReplyTo] = useState<ChatReply | null>(null);
  const [sendBusy, setSendBusy] = useState(false);
  const [agentEnabled, setAgentEnabled] = useState(false);
  const [agentBusy, setAgentBusy] = useState(false);
  const [agentStatus, setAgentStatus] = useState<string | null>(null);
  const [a2aBusy, setA2aBusy] = useState<A2aTool | null>(null);
  const [a2aStatus, setA2aStatus] = useState<string | null>(null);
  const [displayedA2aStatus, setDisplayedA2aStatus] = useState("");
  const messageListRef = useRef<HTMLDivElement | null>(null);

  const activeNorm = useMemo(
    () => (activeEns ? activeEns.trim().toLowerCase() : null),
    [activeEns],
  );

  const refreshMessages = useCallback(async (ensKey: string) => {
    const list = await ipc("chat_history", { ens: ensKey });
    setMessages(list);
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        const current = await ipc("chat_current_user");
        setCurrentUserEns(current.ens ? current.ens.toLowerCase() : null);
        const peers = await ipc("chat_list_conversations");
        setSessions(peers.map((p) => p.toLowerCase()));
      } catch {
        // Chat storage is best-effort; normal resolve/open still works.
      }
    })();
  }, []);

  useEffect(() => {
    if (!a2aStatus) {
      setDisplayedA2aStatus("");
      return;
    }

    setDisplayedA2aStatus("");
    let cursor = 0;
    const step = Math.max(1, Math.ceil(a2aStatus.length / 260));
    const id = window.setInterval(() => {
      cursor = Math.min(a2aStatus.length, cursor + step);
      setDisplayedA2aStatus(a2aStatus.slice(0, cursor));
      if (cursor >= a2aStatus.length) {
        window.clearInterval(id);
      }
    }, 28);

    return () => window.clearInterval(id);
  }, [a2aStatus]);

  useEffect(() => {
    const openPanel = () => setConversationPanelOpen(true);
    const togglePanel = () => setConversationPanelOpen((open) => !open);
    window.addEventListener("anton:open-chat-sidebar", openPanel);
    window.addEventListener("anton:toggle-chat-sidebar", togglePanel);
    return () => {
      window.removeEventListener("anton:open-chat-sidebar", openPanel);
      window.removeEventListener("anton:toggle-chat-sidebar", togglePanel);
    };
  }, []);

  useEffect(() => {
    if (routeEns) {
      const n = routeEns.trim().toLowerCase();
      setActiveEns(n);
      setSessions((s) => (s.includes(n) ? s : [...s, n]));
      setConversationPanelOpen(false);
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

  useLayoutEffect(() => {
    const el = messageListRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
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
          setConversationPanelOpen(false);
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
      setResolveError(friendlyResolveError(name, msg));
    } finally {
      setResolving(false);
    }
  };

  const openConversation = async (ens: string) => {
    const n = ens.trim().toLowerCase();
    setSessions((s) => (s.includes(n) ? s : [...s, n]));
    setActiveEns(n);
    setReplyTo(null);
    setConversationPanelOpen(false);
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
      setConversationPanelOpen(true);
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
    if (typeof response === "object" && response !== null && "error" in response) {
      const error = (response as { error?: { message?: string } }).error;
      return `Remote A2A failed: ${error?.message ?? JSON.stringify(error)}`;
    }
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
    if (typeof parsed === "object" && parsed !== null && "error" in parsed) {
      const error = (parsed as { error?: { message?: string } }).error;
      return `Remote ${tool} failed: ${error?.message ?? JSON.stringify(error)}`;
    }
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
      const messageId =
        typeof result === "object" && result !== null && "id" in result ? (result as { id?: string }).id : null;
      return messageId
        ? `Remote agent sent signed reply ${messageId}. Waiting for AXL delivery…`
        : "Remote agent sent a signed reply. Waiting for AXL delivery…";
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
        window.setTimeout(() => {
          void refreshMessages(activeNorm);
        }, 1_500);
        window.setTimeout(() => {
          void refreshMessages(activeNorm);
        }, 4_000);
      }
    } catch (e) {
      setA2aStatus(e instanceof Error ? e.message : String(e));
    } finally {
      setA2aBusy(null);
    }
  };

  return (
    <div className="relative h-full min-h-0 overflow-hidden bg-[#090b12] text-slate-100">
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_16%_10%,rgba(16,185,129,0.16),transparent_28%),radial-gradient(circle_at_88%_18%,rgba(124,58,237,0.16),transparent_26%),linear-gradient(135deg,rgba(15,23,42,0.2),rgba(2,6,23,0.78))]" />
      <div className="relative flex h-full min-h-0">
        <aside
          aria-hidden={!conversationPanelOpen}
          className={cn(
            "min-h-0 shrink-0 overflow-hidden transition-[width,opacity] duration-300 ease-out",
            conversationPanelOpen ? "w-[22rem] opacity-100" : "w-0 opacity-0",
          )}
        >
          <div
            className={cn(
              "flex h-full w-[22rem] min-h-0 flex-col border-r border-white/10 bg-[#11141d]/88 shadow-2xl backdrop-blur-xl transition-transform duration-300 ease-out",
              conversationPanelOpen ? "translate-x-0" : "-translate-x-full",
            )}
          >
          <div className="px-5 pb-4 pt-6">
            <div className="flex items-center justify-between gap-3">
              <div className="flex min-w-0 items-center gap-3">
                <div className="grid size-12 shrink-0 place-items-center rounded-2xl bg-gradient-to-br from-emerald-300 via-cyan-300 to-violet-300 text-sm font-black text-slate-950">
                  {currentUserEns ? conversationInitial(currentUserEns) : "A"}
                </div>
                <div className="min-w-0">
                  <h1 className="mt-0.5 truncate text-xl font-semibold tracking-tight text-white">
                    {currentUserEns ? shortEns(currentUserEns) : "Anton user"}
                  </h1>
                  <p className="truncate font-mono text-[11px] text-slate-500">
                    {currentUserEns ?? "No ENS saved yet"}
                  </p>
                </div>
              </div>
            </div>

            <div className="mt-5 rounded-3xl border border-white/10 bg-white/[0.04] p-2 shadow-inner shadow-black/20">
              <div className="flex items-center gap-2 rounded-2xl bg-black/20 px-3 py-2">
                <span className="text-xs text-slate-500">Search ENS</span>
                <input
                  type="text"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && void handleResolve()}
                  placeholder="gilfoyle.anton.eth"
                  className="min-w-0 flex-1 bg-transparent text-sm text-slate-100 placeholder:text-slate-600 focus:outline-none"
                />
                <button
                  type="button"
                  disabled={resolving || !query.trim()}
                  onClick={() => void handleResolve()}
                  className="rounded-xl bg-emerald-300 px-3 py-1.5 text-xs font-semibold text-emerald-950 transition hover:bg-emerald-200 disabled:opacity-40"
                >
                  {resolving ? "..." : "Go"}
                </button>
              </div>
            </div>

            {resolveError ? (
              <div className="mt-3 rounded-2xl border border-red-400/20 bg-red-500/10 px-3 py-2 text-xs text-red-200">
                {resolveError}
              </div>
            ) : null}

            {resolved ? (
              <div className="mt-3 overflow-hidden rounded-3xl border border-emerald-300/20 bg-emerald-300/[0.06] p-3 text-xs">
                <div className="flex items-center gap-3">
                  <div className="grid size-10 place-items-center rounded-2xl bg-gradient-to-br from-emerald-300 to-cyan-300 font-bold text-slate-950">
                    {conversationInitial(resolved.ens)}
                  </div>
                  <div className="min-w-0">
                    <p className="truncate font-mono text-slate-100">{resolved.ens}</p>
                    <p className="mt-0.5 truncate text-slate-500">peer {resolved.peerId}</p>
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => void openConversation(resolved.ens)}
                  className="mt-3 w-full rounded-2xl bg-white px-3 py-2 text-xs font-semibold text-slate-950 transition hover:bg-emerald-100"
                >
                  Open secure chat
                </button>
              </div>
            ) : null}
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto px-3 pb-4">
            <div className="mb-2 flex items-center justify-between px-2">
              <p className="text-[10px] font-semibold uppercase tracking-[0.24em] text-slate-500">Conversations</p>
              <span className="rounded-full bg-white/[0.06] px-2 py-0.5 text-[10px] text-slate-400">
                {sessions.length}
              </span>
            </div>
            {sessions.length === 0 ? (
              <div className="mx-2 rounded-3xl border border-dashed border-white/10 bg-white/[0.03] p-5 text-sm text-slate-500">
                Resolve an ENS subname to start your first encrypted thread.
              </div>
            ) : (
              <ul className="space-y-2">
                {sessions.map((s) => (
                  <li key={s}>
                    <div
                      className={cn(
                        "group flex items-center gap-3 rounded-3xl border p-3 transition",
                        activeNorm === s
                          ? "border-emerald-300/30 bg-emerald-300/[0.08] shadow-[0_16px_50px_rgba(16,185,129,0.08)]"
                          : "border-transparent bg-white/[0.03] hover:border-white/10 hover:bg-white/[0.06]",
                      )}
                    >
                      <button
                        type="button"
                        onClick={() => void openConversation(s)}
                        className="flex min-w-0 flex-1 items-center gap-3 text-left"
                      >
                        <div className="relative grid size-12 shrink-0 place-items-center rounded-2xl bg-gradient-to-br from-slate-700 to-slate-900 font-semibold text-slate-100 ring-1 ring-white/10">
                          {conversationInitial(s)}
                          <span className="absolute -bottom-0.5 -right-0.5 size-3 rounded-full border-2 border-[#11141d] bg-emerald-300" />
                        </div>
                        <div className="min-w-0">
                          <p className="truncate text-sm font-semibold text-slate-100">{shortEns(s)}</p>
                          <p className="mt-0.5 truncate font-mono text-[11px] text-slate-500">{s}</p>
                        </div>
                      </button>
                      <button
                        type="button"
                        title="Close"
                        onClick={() => void closeConversation(s)}
                        className="grid size-7 shrink-0 place-items-center rounded-full text-slate-600 opacity-0 transition hover:bg-white/10 hover:text-slate-200 group-hover:opacity-100"
                      >
                        x
                      </button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>
          </div>
        </aside>

        <section className="flex min-h-0 min-w-0 flex-1 flex-col bg-[#0b0e15]/82 backdrop-blur">
          <header
            className={cn(
              "flex shrink-0 items-center justify-between border-b px-6 py-4 transition-colors",
              agentEnabled && activeNorm
                ? "border-emerald-300/25 bg-emerald-300/[0.08] shadow-[0_18px_58px_rgba(16,185,129,0.08)]"
                : "border-white/10 bg-[#11141d]/88 shadow-[0_18px_50px_rgba(0,0,0,0.22)]",
            )}
          >
            <div className="flex min-w-0 items-center gap-4">
              <div
                className={cn(
                  "grid size-14 place-items-center rounded-3xl bg-gradient-to-br text-lg font-black text-slate-950 transition-shadow",
                  agentEnabled && activeNorm
                    ? "from-emerald-200 via-cyan-300 to-teal-300 shadow-[0_0_44px_rgba(45,212,191,0.28)]"
                    : "from-violet-400 via-cyan-300 to-emerald-300 shadow-[0_0_40px_rgba(34,211,238,0.14)]",
                )}
              >
                {activeNorm ? conversationInitial(activeNorm) : "S"}
              </div>
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <h2 className="truncate text-lg font-semibold tracking-tight text-white">
                    {activeNorm ? shortEns(activeNorm) : "Select a conversation"}
                  </h2>
                  {activeNorm ? (
                    <span className="rounded-full border border-emerald-300/20 bg-emerald-300/10 px-2 py-0.5 text-[10px] font-medium uppercase tracking-[0.16em] text-emerald-200">
                      Signed
                    </span>
                  ) : null}
                </div>
                <p className="mt-1 truncate font-mono text-xs text-slate-500">
                  {activeNorm ?? "Resolve an ENS name to begin"}
                </p>
                <div className="mt-2 flex flex-wrap gap-2">
                  {agentStatus ? <span className="text-[11px] text-slate-400">{agentStatus}</span> : null}
                </div>
              </div>
            </div>
            <div className="flex shrink-0 flex-col items-center gap-1">
              <button
                type="button"
                disabled={!activeNorm || agentBusy}
                onClick={() => void toggleAgent()}
                title={agentEnabled ? "Personal agent online" : "Personal agent off"}
                aria-pressed={agentEnabled}
                aria-label={agentEnabled ? "Turn personal agent off" : "Turn personal agent on"}
                className={cn(
                  "grid size-10 place-items-center rounded-2xl border transition disabled:opacity-40",
                  agentEnabled
                    ? "border-emerald-300/20 bg-emerald-300/10 text-emerald-200 shadow-[0_0_22px_rgba(52,211,153,0.12)]"
                    : "border-white/10 bg-white/[0.04] text-slate-500 hover:bg-white/[0.08] hover:text-slate-200",
                )}
              >
                <svg viewBox="0 0 24 24" aria-hidden className="size-5" fill="none" stroke="currentColor" strokeWidth="1.8">
                  <path d="M12 3v8" strokeLinecap="round" />
                  <path d="M7.1 6.5a7 7 0 1 0 9.8 0" strokeLinecap="round" />
                </svg>
              </button>
              <span className={cn("text-[10px] font-medium", agentEnabled ? "text-emerald-200" : "text-slate-500")}>
                Agent mode
              </span>
            </div>
          </header>

          <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
            <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_48%_12%,rgba(16,185,129,0.08),transparent_28%)]" />
            <div ref={messageListRef} className="relative min-h-0 flex-1 overflow-y-auto px-6 py-6">
              {messages.length === 0 ? (
                <div className="mx-auto mt-16 max-w-md rounded-[2rem] border border-white/10 bg-white/[0.04] p-8 text-center shadow-2xl shadow-black/20">
                  <div className="mx-auto grid size-14 place-items-center rounded-3xl bg-gradient-to-br from-emerald-300 to-violet-300 font-black text-slate-950">
                    AI
                  </div>
                  <h3 className="mt-4 text-lg font-semibold text-white">No messages yet</h3>
                  <p className="mt-2 text-sm leading-6 text-slate-400">
                    Start the secure thread, or ask the remote agent for a draft, summary, or handoff.
                  </p>
                </div>
              ) : (
                <div className="space-y-5">
                  {messages.map((m) => {
                    const incoming = m.from.toLowerCase() === activeNorm;
                    return (
                      <div
                        key={m.id}
                        className={cn("flex gap-3", incoming ? "justify-start" : "justify-end")}
                      >
                        {incoming ? (
                          <div className="mt-1 grid size-9 shrink-0 place-items-center rounded-2xl bg-slate-800 text-xs font-semibold text-slate-300 ring-1 ring-white/10">
                            {conversationInitial(m.from)}
                          </div>
                        ) : null}
                        <div className={cn("max-w-[72%]", incoming ? "items-start" : "items-end")}>
                          <div
                            className={cn(
                              "rounded-[1.45rem] px-4 py-3 text-sm leading-6 shadow-lg ring-1",
                              incoming
                                ? "rounded-tl-md bg-[#253342] text-slate-100 shadow-black/20 ring-white/10"
                                : "rounded-tr-md bg-[#405982] text-white shadow-black/20 ring-white/10",
                            )}
                          >
                            {m.replyTo ? (
                              <div
                                className={cn(
                                  "mb-2 rounded-2xl border-l-2 px-3 py-2 text-xs",
                                  incoming
                                    ? "border-cyan-300/50 bg-black/15 text-slate-300"
                                    : "border-cyan-200/45 bg-white/10 text-blue-50",
                                )}
                              >
                                <p className="font-mono text-[10px] opacity-70">Reply to {m.replyTo.from}</p>
                                <p className="mt-0.5 line-clamp-2 whitespace-pre-wrap">{m.replyTo.text}</p>
                              </div>
                            ) : null}
                            <p className="whitespace-pre-wrap">{m.text}</p>
                            <div className="mt-2 flex items-center justify-between gap-3 text-[10px] font-medium uppercase tracking-[0.12em] text-white/55">
                              {m.agentGenerated ? (
                                <span className="rounded-full border border-emerald-300/20 bg-emerald-300/10 px-2 py-0.5 text-emerald-200">
                                  Agent
                                </span>
                              ) : (
                                <span />
                              )}
                              <div className="flex items-center justify-end gap-2">
                                <span>{m.state}</span>
                                <span>
                                  {new Date(Number(m.ts)).toLocaleTimeString([], {
                                    hour: "2-digit",
                                    minute: "2-digit",
                                  })}
                                </span>
                              </div>
                            </div>
                          </div>
                          <button
                            type="button"
                            onClick={() => setReplyTo(toReply(m))}
                            className="mt-1 px-2 text-[11px] font-medium text-slate-500 transition hover:text-slate-200"
                          >
                            Reply
                          </button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          </div>

          <footer className="shrink-0 border-t border-white/10 bg-[#0b0e15]/95 px-6 py-4 backdrop-blur-xl">
            {replyTo ? (
              <div className="mb-3 flex items-start justify-between gap-3 rounded-3xl border border-emerald-300/15 bg-emerald-300/[0.06] px-4 py-3 text-xs">
                <div className="min-w-0">
                  <p className="font-mono text-emerald-200/80">Replying to {replyTo.from}</p>
                  <p className="mt-1 truncate text-slate-300">{replyTo.text}</p>
                </div>
                <button
                  type="button"
                  onClick={() => setReplyTo(null)}
                  className="shrink-0 rounded-full px-2 text-slate-500 transition hover:bg-white/10 hover:text-slate-200"
                >
                  Cancel
                </button>
              </div>
            ) : null}

            <div className="flex items-end gap-3 rounded-[1.75rem] border border-white/10 bg-white/[0.05] p-2 shadow-2xl shadow-black/20">
              <textarea
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    void handleSend();
                  }
                }}
                disabled={!activeNorm || sendBusy}
                rows={1}
                placeholder={activeNorm ? "Type a secure message..." : "Open a conversation first"}
                className="max-h-32 min-h-11 flex-1 resize-none bg-transparent px-4 py-3 text-sm text-slate-100 placeholder:text-slate-500 focus:outline-none disabled:opacity-50"
              />
              <button
                type="button"
                disabled={!activeNorm || sendBusy || !draft.trim()}
                onClick={() => void handleSend()}
                className="grid size-11 shrink-0 place-items-center rounded-2xl bg-emerald-300 text-sm font-black text-emerald-950 transition hover:bg-emerald-200 disabled:opacity-40"
              >
                Go
              </button>
            </div>
          </footer>
        </section>

        <aside className="flex min-h-0 w-[22rem] shrink-0 flex-col border-l border-white/10 bg-[#0d1018]/92 shadow-2xl shadow-black/30 backdrop-blur-xl">
          <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-4 py-5">
            <section>
              <div className="mb-3 flex items-center justify-between">
                <p className="text-[10px] font-semibold uppercase tracking-[0.24em] text-slate-500">AXL A2A Skills</p>
                {a2aBusy ? <span className="text-[11px] text-emerald-300">Working</span> : null}
              </div>
              <div className="space-y-2">
                {A2A_SKILLS.map((skill) => (
                  <button
                    key={skill.tool}
                    type="button"
                    disabled={!activeNorm || a2aBusy != null}
                    onClick={() =>
                      void callA2aTool(
                        skill.tool,
                        skill.tool === "send_reply" && draft.trim()
                          ? { text: draft.trim() }
                          : skill.tool === "handoff_to_human"
                            ? { reason: "A2A handoff requested from Anton UI." }
                            : {},
                      )
                    }
                    className="group flex w-full items-center gap-2.5 rounded-2xl border border-white/10 bg-white/[0.04] p-2.5 text-left transition hover:border-white/15 hover:bg-white/[0.07] disabled:cursor-not-allowed disabled:opacity-40"
                  >
                    <span className={cn("grid size-8 shrink-0 place-items-center rounded-xl bg-gradient-to-br text-xs font-black text-slate-950", skill.accent)}>
                      {skill.label.charAt(0)}
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block text-xs font-semibold text-slate-100">
                        {a2aBusy === skill.tool ? "Working..." : skill.label}
                      </span>
                      <span className="mt-0.5 block text-[11px] leading-4 text-slate-500">{skill.description}</span>
                    </span>
                  </button>
                ))}
              </div>
            </section>

            <section>
              <div className="mb-3 flex items-center justify-between">
                <p className="text-[10px] font-semibold uppercase tracking-[0.24em] text-slate-500">Remote Result</p>
                {a2aStatus ? (
                  <button
                    type="button"
                    onClick={() => setA2aStatus(null)}
                    className="text-[11px] font-medium text-slate-500 transition hover:text-slate-200"
                  >
                    Clear
                  </button>
                ) : null}
              </div>
              <div className="min-h-28 rounded-3xl border border-white/10 bg-black/20 p-4">
                {a2aStatus ? (
                  <p className="whitespace-pre-wrap text-sm leading-6 text-slate-200">
                    {displayedA2aStatus}
                    {displayedA2aStatus.length < a2aStatus.length ? (
                      <span className="ml-0.5 inline-block h-4 w-1 translate-y-0.5 animate-pulse rounded-full bg-emerald-300" />
                    ) : null}
                  </p>
                ) : (
                  <p className="text-sm leading-6 text-slate-500">
                    Run a remote skill to see drafts, summaries, handoff responses, or delivery status here.
                  </p>
                )}
              </div>
            </section>
          </div>
        </aside>
      </div>
    </div>
  );
}
