import { useParams } from "react-router-dom";

export function ChatPage() {
  const { ens } = useParams<{ ens: string }>();
  return (
    <div className="grid h-full grid-cols-[18rem_1fr]">
      <aside className="border-r border-slate-800 bg-slate-950/40">
        <div className="border-b border-slate-800 p-3">
          <input
            type="text"
            placeholder="alice.chat.eth"
            className="w-full rounded-md border border-slate-800 bg-slate-900 px-3 py-2 text-sm placeholder:text-slate-500 focus:border-emerald-500 focus:outline-none"
            disabled
          />
        </div>
        <div className="px-3 py-6 text-xs text-slate-500">
          Conversations open in this session will appear here. Closing one
          drops it; restarting the app starts the sidebar empty (chat is
          ephemeral by design).
        </div>
      </aside>
      <section className="flex flex-col">
        <header className="flex items-center justify-between border-b border-slate-800 px-4 py-3">
          <div className="flex items-center gap-2">
            <span className="font-mono text-sm">
              {ens ?? "(no conversation open)"}
            </span>
            {ens && (
              <span className="rounded-full bg-emerald-500/10 px-2 py-0.5 text-xs text-emerald-400">
                verified by ENS
              </span>
            )}
          </div>
        </header>
        <div className="flex flex-1 items-center justify-center text-sm text-slate-500">
          {ens
            ? "Message thread will render here once the Rust core is wired up."
            : "Resolve a *.chat.eth name from the sidebar to start a conversation."}
        </div>
        <footer className="border-t border-slate-800 p-3">
          <input
            type="text"
            placeholder="Message…"
            className="w-full rounded-md border border-slate-800 bg-slate-900 px-3 py-2 text-sm placeholder:text-slate-500 focus:border-emerald-500 focus:outline-none"
            disabled
          />
        </footer>
      </section>
    </div>
  );
}
