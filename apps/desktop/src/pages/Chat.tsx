import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useNavigate, useParams } from "react-router-dom";
import type { CalendarDraft, ChatMessage, ChatReply } from "@anton/shared-types";
import { ipc } from "../lib/ipc";
import { cn } from "../lib/cn";

type Resolved = {
  ens: string;
  wallet: string;
  peerId: string;
  pubkeyPem: string;
  agentServiceName: string;
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

type A2aTool = "draft_reply" | "send_reply" | "summarize_conversation" | "handoff_to_human" | "propose_calendar_event";

type A2aCommand =
  | { kind: "skill"; tool: A2aTool; arguments: Record<string, unknown> }
  | { kind: "unknown"; command: string };

type A2aCommandOption = {
  command: string;
  insertText: string;
  label: string;
  description: string;
};

type LocalSkillMessage = ChatMessage & {
  timelineKind: "skill_call" | "skill_result";
  skillTool: A2aTool;
};

type TimelineMessage = ChatMessage | LocalSkillMessage;

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
  {
    tool: "propose_calendar_event",
    label: "Calendar",
    description: "Ask the peer agent to create a meeting draft.",
    accent: "from-emerald-200 to-lime-300",
  },
];

const A2A_COMMAND_OPTIONS: ReadonlyArray<A2aCommandOption> = [
  {
    command: "summarize",
    insertText: "/summarize",
    label: "Summarize",
    description: "Run summarize_conversation for this chat.",
  },
  {
    command: "draft",
    insertText: "/draft",
    label: "Draft",
    description: "Ask the remote agent for a draft reply.",
  },
  {
    command: "send",
    insertText: "/send ",
    label: "Send reply",
    description: "Ask the remote agent to send a signed reply.",
  },
  {
    command: "handoff",
    insertText: "/handoff",
    label: "Handoff",
    description: "Ask the remote agent to wait for the human.",
  },
  {
    command: "calendar",
    insertText: "/calendar ",
    label: "Calendar proposal",
    description: "Create a remote pending meeting draft.",
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

function localDateTimeInput(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function inputDateTimeToIso(value: string): string {
  return value ? new Date(value).toISOString() : "";
}

function isLocalSkillMessage(message: TimelineMessage): message is LocalSkillMessage {
  return "timelineKind" in message;
}

function skillLabel(tool: A2aTool): string {
  return A2A_SKILLS.find((skill) => skill.tool === tool)?.label ?? tool;
}

function commandTextForTool(tool: A2aTool, args: Record<string, unknown>): string {
  if (tool === "summarize_conversation") return "/summarize";
  if (tool === "draft_reply") return "/draft";
  if (tool === "send_reply") {
    const text = typeof args.text === "string" ? args.text.trim() : "";
    return text ? `/send ${text}` : "/send";
  }
  if (tool === "handoff_to_human") {
    const reason = typeof args.reason === "string" ? args.reason.trim() : "";
    return reason && reason !== "A2A handoff requested from Anton UI." ? `/handoff ${reason}` : "/handoff";
  }
  if (tool === "propose_calendar_event") {
    const title = typeof args.title === "string" ? args.title.trim() : "";
    return title ? `/calendar ${title}` : "/calendar";
  }
  return `/${tool}`;
}

function parseA2aCommand(text: string): A2aCommand | null {
  const trimmed = text.trim();
  if (!trimmed.startsWith("/")) return null;

  const [rawCommand = "", ...rest] = trimmed.slice(1).split(/\s+/);
  const command = rawCommand.toLowerCase();
  const remainder = rest.join(" ").trim();

  if (command === "summarize" || command === "summary" || command === "summerize") {
    return { kind: "skill", tool: "summarize_conversation", arguments: {} };
  }
  if (command === "draft") {
    return { kind: "skill", tool: "draft_reply", arguments: {} };
  }
  if (command === "send") {
    return { kind: "skill", tool: "send_reply", arguments: remainder ? { text: remainder } : {} };
  }
  if (command === "handoff") {
    return {
      kind: "skill",
      tool: "handoff_to_human",
      arguments: { reason: remainder || "A2A handoff requested from Anton UI." },
    };
  }
  if (command === "calendar" || command === "meeting") {
    const tomorrow = new Date(Date.now() + 24 * 60 * 60 * 1000);
    tomorrow.setMinutes(0, 0, 0);
    const end = new Date(tomorrow.getTime() + 30 * 60 * 1000);
    return {
      kind: "skill",
      tool: "propose_calendar_event",
      arguments: {
        title: remainder || "Meeting proposal",
        description: remainder || "Meeting proposed from Anton chat.",
        start: tomorrow.toISOString(),
        end: end.toISOString(),
        timezone: Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC",
        location: "",
        attendees: [],
        requestId: crypto.randomUUID(),
        requiresHumanConfirmation: true,
      },
    };
  }

  return { kind: "unknown", command: rawCommand || "unknown" };
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
  const [localSkillMessages, setLocalSkillMessages] = useState<Record<string, LocalSkillMessage[]>>({});
  const [draft, setDraft] = useState("");
  const [replyTo, setReplyTo] = useState<ChatReply | null>(null);
  const [sendBusy, setSendBusy] = useState(false);
  const [clearBusy, setClearBusy] = useState(false);
  const [clearConfirming, setClearConfirming] = useState(false);
  const [agentEnabled, setAgentEnabled] = useState(false);
  const [agentBusy, setAgentBusy] = useState(false);
  const [agentStatus, setAgentStatus] = useState<string | null>(null);
  const [a2aBusy, setA2aBusy] = useState<A2aTool | null>(null);
  const [a2aStatus, setA2aStatus] = useState<string | null>(null);
  const [displayedA2aStatus, setDisplayedA2aStatus] = useState("");
  const [calendarDrafts, setCalendarDrafts] = useState<CalendarDraft[]>([]);
  const [calendarBusy, setCalendarBusy] = useState<string | null>(null);
  const [proposalTitle, setProposalTitle] = useState("Project sync");
  const [proposalStart, setProposalStart] = useState("");
  const [proposalEnd, setProposalEnd] = useState("");
  const [proposalLocation, setProposalLocation] = useState("");
  const messageListRef = useRef<HTMLDivElement | null>(null);
  const draftInputRef = useRef<HTMLTextAreaElement | null>(null);

  const activeNorm = useMemo(
    () => (activeEns ? activeEns.trim().toLowerCase() : null),
    [activeEns],
  );

  const commandQuery = useMemo(() => {
    const trimmedStart = draft.trimStart();
    if (!trimmedStart.startsWith("/")) return null;
    const token = trimmedStart.slice(1).split(/\s+/)[0] ?? "";
    return trimmedStart.includes(" ") ? null : token.toLowerCase();
  }, [draft]);

  const commandOptions = useMemo(() => {
    if (commandQuery === null) return [];
    return A2A_COMMAND_OPTIONS.filter(
      (option) =>
        option.command.includes(commandQuery) ||
        option.label.toLowerCase().includes(commandQuery),
    );
  }, [commandQuery]);

  const activeSkillMessages = useMemo(
    () => (activeNorm ? (localSkillMessages[activeNorm] ?? []) : []),
    [activeNorm, localSkillMessages],
  );

  const timelineMessages = useMemo<TimelineMessage[]>(
    () => [...messages, ...activeSkillMessages].sort((a, b) => a.ts - b.ts),
    [activeSkillMessages, messages],
  );

  const refreshMessages = useCallback(async (ensKey: string) => {
    const list = await ipc("chat_history", { ens: ensKey });
    setMessages(list);
  }, []);

  const refreshCalendarDrafts = useCallback(async (ensKey: string) => {
    const drafts = await ipc("agent_list_calendar_drafts", { peer: ensKey });
    setCalendarDrafts(drafts);
  }, []);

  const addLocalSkillMessage = useCallback((peer: string, message: LocalSkillMessage) => {
    setLocalSkillMessages((previous) => ({
      ...previous,
      [peer]: [...(previous[peer] ?? []), message],
    }));
  }, []);

  useEffect(() => {
    const start = new Date(Date.now() + 24 * 60 * 60 * 1000);
    start.setMinutes(0, 0, 0);
    const end = new Date(start.getTime() + 30 * 60 * 1000);
    setProposalStart(localDateTimeInput(start));
    setProposalEnd(localDateTimeInput(end));
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
      setCalendarDrafts([]);
      setClearConfirming(false);
      return;
    }
    void (async () => {
      try {
        await ipc("chat_open", { ens: activeNorm });
        await refreshMessages(activeNorm);
        await refreshCalendarDrafts(activeNorm);
        const mode = await ipc("agent_get_conversation_mode", { peer: activeNorm });
        setAgentEnabled(mode.enabled);
      } catch {
        setMessages([]);
      }
    })();
    setClearConfirming(false);
  }, [activeNorm, refreshCalendarDrafts, refreshMessages]);

  useEffect(() => {
    if (!clearConfirming) return;
    const id = window.setTimeout(() => setClearConfirming(false), 3_000);
    return () => window.clearTimeout(id);
  }, [clearConfirming]);

  useLayoutEffect(() => {
    const el = messageListRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [timelineMessages, activeNorm]);

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

  useEffect(() => {
    const unlisten = listen<CalendarDraft>("calendar:draft-created", (ev) => {
      const draft = ev.payload;
      if (!draft?.peer || draft.peer.toLowerCase() !== activeNorm) return;
      void refreshCalendarDrafts(draft.peer);
    });
    return () => {
      void unlisten.then((u) => u());
    };
  }, [activeNorm, refreshCalendarDrafts]);

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

  const clearConversation = async () => {
    if (!activeNorm) return;
    if (!clearConfirming) {
      setClearConfirming(true);
      return;
    }
    setClearBusy(true);
    setResolveError(null);
    try {
      const peer = activeNorm;
      await ipc("chat_clear", { ens: peer });
      setMessages([]);
      setReplyTo(null);
      setA2aStatus(null);
      setLocalSkillMessages((previous) => {
        const next = { ...previous };
        delete next[peer];
        return next;
      });
      setClearConfirming(false);
    } catch (e) {
      setResolveError(e instanceof Error ? e.message : String(e));
    } finally {
      setClearBusy(false);
    }
  };

  const handleSend = async () => {
    const text = draft.trim();
    if (!activeNorm || !text) return;
    const command = parseA2aCommand(text);
    setSendBusy(true);
    try {
      if (command?.kind === "unknown") {
        setResolveError(`Unknown skill command /${command.command}. Try /summarize, /draft, /send, /handoff, or /calendar.`);
        return;
      }
      if (command?.kind === "skill") {
        setResolveError(null);
        setDraft("");
        setReplyTo(null);
        await callA2aTool(command.tool, command.arguments, {
          commandText: text,
          replyTo,
        });
        return;
      }
      await ipc("chat_send", {
        to: activeNorm,
        text,
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
    if (tool === "propose_calendar_event" && typeof result === "object" && result !== null) {
      const proposal = result as { status?: string; draftId?: string; available?: boolean; message?: string };
      return `Calendar proposal: ${proposal.message ?? proposal.status ?? "Draft created."}`;
    }
    return typeof result === "string" ? result : JSON.stringify(result);
  };

  const callA2aTool = async (
    tool: A2aTool,
    extra: Record<string, unknown> = {},
    options: { commandText?: string; replyTo?: ChatReply | null } = {},
  ) => {
    if (!activeNorm) return;
    const peer = activeNorm;
    const caller = currentUserEns ?? "you";
    const callId = `skill-call-${tool}-${Date.now()}-${Math.random().toString(36).slice(2)}`;
    const callMessage: LocalSkillMessage = {
      id: callId,
      from: caller,
      to: peer,
      text: options.commandText ?? commandTextForTool(tool, extra),
      ts: Date.now(),
      state: "sent",
      ...(options.replyTo ? { replyTo: options.replyTo } : {}),
      timelineKind: "skill_call",
      skillTool: tool,
    };
    addLocalSkillMessage(peer, callMessage);
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
      const formatted = formatA2aResponse(tool, res.response);
      setA2aStatus(formatted);
      addLocalSkillMessage(peer, {
        id: `${callId}-result`,
        from: peer,
        to: caller,
        text: formatted,
        ts: Date.now(),
        state: "received",
        replyTo: toReply(callMessage),
        agentGenerated: true,
        timelineKind: "skill_result",
        skillTool: tool,
      });
      if (tool === "send_reply") {
        await refreshMessages(peer);
        window.setTimeout(() => {
          void refreshMessages(peer);
        }, 1_500);
        window.setTimeout(() => {
          void refreshMessages(peer);
        }, 4_000);
      }
      if (tool === "propose_calendar_event") {
        await refreshCalendarDrafts(peer);
      }
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setA2aStatus(message);
      addLocalSkillMessage(peer, {
        id: `${callId}-error`,
        from: peer,
        to: caller,
        text: message,
        ts: Date.now(),
        state: "failed",
        replyTo: toReply(callMessage),
        agentGenerated: true,
        timelineKind: "skill_result",
        skillTool: tool,
      });
    } finally {
      setA2aBusy(null);
    }
  };

  const callCalendarProposal = async () => {
    if (!activeNorm) return;
    const title = proposalTitle.trim() || "Meeting proposal";
    const start = inputDateTimeToIso(proposalStart);
    const end = inputDateTimeToIso(proposalEnd);
    if (!start || !end) {
      setResolveError("Add a start and end time for the calendar proposal.");
      return;
    }
    await callA2aTool("propose_calendar_event", {
      title,
      description: draft.trim() || `Meeting proposed from Anton chat with ${shortEns(activeNorm)}.`,
      start,
      end,
      timezone: Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC",
      location: proposalLocation.trim(),
      attendees: [currentUserEns, activeNorm].filter(Boolean),
      requestId: crypto.randomUUID(),
      requiresHumanConfirmation: true,
    });
  };

  const updateCalendarDraft = async (
    draftId: string,
    action: "accept" | "reject" | "counter",
    counterStart?: string,
    counterEnd?: string,
  ) => {
    if (!activeNorm) return;
    setCalendarBusy(draftId);
    setResolveError(null);
    try {
      await ipc("agent_update_calendar_draft", {
        draftId,
        action,
        ...(counterStart ? { counterStart } : {}),
        ...(counterEnd ? { counterEnd } : {}),
      });
      await refreshCalendarDrafts(activeNorm);
      await refreshMessages(activeNorm);
    } catch (e) {
      setResolveError(e instanceof Error ? e.message : String(e));
    } finally {
      setCalendarBusy(null);
    }
  };

  const counterCalendarDraft = async (draft: CalendarDraft) => {
    const nextStart = window.prompt("Suggest start time (ISO 8601)", draft.start);
    if (!nextStart) return;
    const nextEnd = window.prompt("Suggest end time (ISO 8601)", draft.end);
    if (!nextEnd) return;
    await updateCalendarDraft(draft.id, "counter", nextStart, nextEnd);
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
            <div className="flex shrink-0 items-center gap-3">
              <button
                type="button"
                disabled={!activeNorm || clearBusy || timelineMessages.length === 0}
                onClick={() => void clearConversation()}
                className={cn(
                  "rounded-2xl border px-3 py-2 text-xs font-medium transition disabled:opacity-40",
                  clearConfirming
                    ? "border-red-300/25 bg-red-500/10 text-red-100"
                    : "border-white/10 bg-white/[0.04] text-slate-400 hover:bg-red-500/10 hover:text-red-200",
                )}
              >
                {clearBusy ? "Clearing..." : clearConfirming ? "Confirm clear" : "Clear"}
              </button>
              <div className="flex flex-col items-center gap-1">
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
                  Son of Anton
                </span>
              </div>
            </div>
          </header>

          <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
            <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_48%_12%,rgba(16,185,129,0.08),transparent_28%)]" />
            <div ref={messageListRef} className="relative min-h-0 flex-1 overflow-y-auto px-6 py-6">
              {timelineMessages.length === 0 ? (
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
                  {timelineMessages.map((m) => {
                    const incoming = m.from.toLowerCase() === activeNorm;
                    const skillMessage = isLocalSkillMessage(m) ? m : null;
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
                            <p className={cn("whitespace-pre-wrap", skillMessage?.timelineKind === "skill_call" ? "font-semibold" : null)}>
                              {m.text}
                            </p>
                            <div className="mt-2 flex items-center justify-between gap-3 text-[10px] font-medium uppercase tracking-[0.12em] text-white/55">
                              <div className="flex min-w-0 items-center gap-1.5">
                                {skillMessage?.timelineKind === "skill_call" ? (
                                  <span className="rounded-full border border-emerald-300/20 bg-emerald-300/10 px-2 py-0.5 text-emerald-200">
                                    Skill call
                                  </span>
                                ) : null}
                                {skillMessage?.timelineKind === "skill_result" ? (
                                  <span className="rounded-full border border-cyan-300/20 bg-cyan-300/10 px-2 py-0.5 text-cyan-100">
                                    {skillLabel(skillMessage.skillTool)}
                                  </span>
                                ) : null}
                                {m.agentGenerated ? (
                                  <span className="rounded-full border border-emerald-300/20 bg-emerald-300/10 px-2 py-0.5 text-emerald-200">
                                    Agent
                                  </span>
                                ) : null}
                              </div>
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

            {commandQuery !== null ? (
              <div className="mb-3 overflow-hidden rounded-3xl border border-white/10 bg-[#11141d]/95 p-2 shadow-2xl shadow-black/25 backdrop-blur-xl">
                <div className="px-3 pb-2 pt-1 text-[10px] font-semibold uppercase tracking-[0.22em] text-slate-500">
                  A2A skills
                </div>
                {commandOptions.length > 0 ? (
                  <div className="space-y-1">
                    {commandOptions.map((option) => (
                      <button
                        key={option.command}
                        type="button"
                        onMouseDown={(e) => e.preventDefault()}
                        onClick={() => {
                          setDraft(option.insertText);
                          window.requestAnimationFrame(() => draftInputRef.current?.focus());
                        }}
                        className="flex w-full items-center gap-3 rounded-2xl px-3 py-2 text-left transition hover:bg-white/[0.06]"
                      >
                        <span className="rounded-xl border border-emerald-300/20 bg-emerald-300/10 px-2.5 py-1 font-mono text-xs font-semibold text-emerald-200">
                          /{option.command}
                        </span>
                        <span className="min-w-0">
                          <span className="block text-sm font-medium text-slate-100">{option.label}</span>
                          <span className="block truncate text-xs text-slate-500">{option.description}</span>
                        </span>
                      </button>
                    ))}
                  </div>
                ) : (
                  <p className="px-3 pb-2 text-xs text-slate-500">No skill matches this command.</p>
                )}
              </div>
            ) : null}

            <div className="flex items-end gap-3 rounded-[1.75rem] border border-white/10 bg-white/[0.05] p-2 shadow-2xl shadow-black/20">
              <textarea
                ref={draftInputRef}
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
                      skill.tool === "propose_calendar_event"
                        ? void callCalendarProposal()
                        : void callA2aTool(
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
                <p className="text-[10px] font-semibold uppercase tracking-[0.24em] text-slate-500">Meeting proposal</p>
              </div>
              <div className="space-y-2 rounded-3xl border border-white/10 bg-white/[0.03] p-3">
                <input
                  value={proposalTitle}
                  onChange={(e) => setProposalTitle(e.target.value)}
                  placeholder="Project sync"
                  className="w-full rounded-2xl border border-white/10 bg-black/20 px-3 py-2 text-xs text-slate-100 placeholder:text-slate-600 focus:outline-none"
                />
                <div className="grid grid-cols-2 gap-2">
                  <input
                    type="datetime-local"
                    value={proposalStart}
                    onChange={(e) => setProposalStart(e.target.value)}
                    className="min-w-0 rounded-2xl border border-white/10 bg-black/20 px-3 py-2 text-xs text-slate-100 focus:outline-none"
                  />
                  <input
                    type="datetime-local"
                    value={proposalEnd}
                    onChange={(e) => setProposalEnd(e.target.value)}
                    className="min-w-0 rounded-2xl border border-white/10 bg-black/20 px-3 py-2 text-xs text-slate-100 focus:outline-none"
                  />
                </div>
                <input
                  value={proposalLocation}
                  onChange={(e) => setProposalLocation(e.target.value)}
                  placeholder="Location or meeting link"
                  className="w-full rounded-2xl border border-white/10 bg-black/20 px-3 py-2 text-xs text-slate-100 placeholder:text-slate-600 focus:outline-none"
                />
                <p className="text-[11px] leading-4 text-slate-500">
                  Uses the Calendar skill. The receiver gets a pending draft and must confirm before Google Calendar is written.
                </p>
              </div>
            </section>

            <section>
              <div className="mb-3 flex items-center justify-between">
                <p className="text-[10px] font-semibold uppercase tracking-[0.24em] text-slate-500">Calendar drafts</p>
                {activeNorm ? (
                  <button
                    type="button"
                    onClick={() => void refreshCalendarDrafts(activeNorm)}
                    className="text-[11px] font-medium text-slate-500 transition hover:text-slate-200"
                  >
                    Refresh
                  </button>
                ) : null}
              </div>
              <div className="space-y-2">
                {calendarDrafts.length > 0 ? (
                  calendarDrafts.map((item) => (
                    <div key={item.id} className="rounded-3xl border border-white/10 bg-black/20 p-3">
                      <div className="flex items-start justify-between gap-3">
                        <div className="min-w-0">
                          <p className="truncate text-xs font-semibold text-slate-100">{item.title}</p>
                          <p className="mt-1 text-[11px] leading-4 text-slate-500">
                            {new Date(item.start).toLocaleString([], {
                              month: "short",
                              day: "numeric",
                              hour: "2-digit",
                              minute: "2-digit",
                            })}{" "}
                            -{" "}
                            {new Date(item.end).toLocaleTimeString([], {
                              hour: "2-digit",
                              minute: "2-digit",
                            })}
                          </p>
                        </div>
                        <span
                          className={cn(
                            "shrink-0 rounded-full px-2 py-0.5 text-[10px] uppercase tracking-[0.12em]",
                            item.status === "pending"
                              ? "border border-amber-300/20 bg-amber-300/10 text-amber-100"
                              : "border border-white/10 bg-white/[0.05] text-slate-400",
                          )}
                        >
                          {item.status}
                        </span>
                      </div>
                      <p className="mt-2 text-[11px] leading-4 text-slate-400">{item.message}</p>
                      {item.location ? <p className="mt-1 text-[11px] text-slate-500">{item.location}</p> : null}
                      {item.status === "pending" ? (
                        <div className="mt-3 grid grid-cols-3 gap-2">
                          <button
                            type="button"
                            disabled={calendarBusy === item.id}
                            onClick={() => void updateCalendarDraft(item.id, "accept")}
                            className="rounded-xl bg-emerald-300 px-2 py-1.5 text-[11px] font-semibold text-emerald-950 disabled:opacity-40"
                          >
                            Accept
                          </button>
                          <button
                            type="button"
                            disabled={calendarBusy === item.id}
                            onClick={() => void updateCalendarDraft(item.id, "reject")}
                            className="rounded-xl border border-white/10 bg-white/[0.04] px-2 py-1.5 text-[11px] text-slate-300 disabled:opacity-40"
                          >
                            Reject
                          </button>
                          <button
                            type="button"
                            disabled={calendarBusy === item.id}
                            onClick={() => void counterCalendarDraft(item)}
                            className="rounded-xl border border-cyan-300/20 bg-cyan-300/10 px-2 py-1.5 text-[11px] text-cyan-100 disabled:opacity-40"
                          >
                            Counter
                          </button>
                        </div>
                      ) : null}
                    </div>
                  ))
                ) : (
                  <p className="rounded-3xl border border-dashed border-white/10 bg-white/[0.02] p-4 text-sm leading-6 text-slate-500">
                    Pending meeting drafts for this chat will appear here.
                  </p>
                )}
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
