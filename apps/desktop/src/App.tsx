import { useEffect, useState } from "react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router-dom";
import { cn } from "./lib/cn";
import { ipc } from "./lib/ipc";

type NavIcon = "chat" | "settings" | "lock";

const NAV_ITEMS: ReadonlyArray<{ to: string; label: string; icon: NavIcon }> = [
  { to: "/chat", label: "Conversations", icon: "chat" },
  { to: "/settings", label: "Settings", icon: "settings" },
];

function conversationInitial(name: string): string {
  return name.trim().charAt(0).toUpperCase() || "A";
}

function shortEns(name: string): string {
  return name.replace(/\.anton\.eth$/i, "");
}

function SidebarIcon({ icon }: { icon: NavIcon }) {
  if (icon === "chat") {
    return (
      <svg viewBox="0 0 24 24" aria-hidden className="size-5" fill="none" stroke="currentColor" strokeWidth="1.8">
        <path d="M5.5 6.75A4.75 4.75 0 0 1 10.25 2h3.5a4.75 4.75 0 0 1 4.75 4.75v3.5A4.75 4.75 0 0 1 13.75 15H10l-4.5 4v-4.75A4.75 4.75 0 0 1 1.5 9.5V6.75" strokeLinecap="round" strokeLinejoin="round" />
        <path d="M8 7.5h8M8 11h5" strokeLinecap="round" />
      </svg>
    );
  }

  if (icon === "settings") {
    return (
      <svg viewBox="0 0 24 24" aria-hidden className="size-5" fill="none" stroke="currentColor" strokeWidth="1.8">
        <path d="M12 15.25A3.25 3.25 0 1 0 12 8.75a3.25 3.25 0 0 0 0 6.5Z" />
        <path d="M18.1 13.5c.08-.49.08-1.01 0-1.5l2.05-1.6-2-3.46-2.56 1a6.4 6.4 0 0 0-1.3-.75L13.9 4.5h-4l-.38 2.69c-.46.2-.9.45-1.3.75l-2.56-1-2 3.46L5.72 12a6.7 6.7 0 0 0 0 1.5l-2.05 1.6 2 3.46 2.56-1c.4.3.84.55 1.3.75l.38 2.69h4l.38-2.69c.46-.2.9-.45 1.3-.75l2.56 1 2-3.46-2.05-1.6Z" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    );
  }

  return (
    <svg viewBox="0 0 24 24" aria-hidden className="size-5" fill="none" stroke="currentColor" strokeWidth="1.8">
      <path d="M7 10V7a5 5 0 0 1 10 0v3" strokeLinecap="round" />
      <path d="M6.5 10h11A2.5 2.5 0 0 1 20 12.5v6A2.5 2.5 0 0 1 17.5 21h-11A2.5 2.5 0 0 1 4 18.5v-6A2.5 2.5 0 0 1 6.5 10Z" strokeLinejoin="round" />
      <path d="M12 15v2" strokeLinecap="round" />
    </svg>
  );
}

export default function App() {
  const location = useLocation();
  const navigate = useNavigate();
  const showSidebar = !location.pathname.startsWith("/onboarding");
  const [currentUserEns, setCurrentUserEns] = useState<string | null>(null);

  useEffect(() => {
    if (!showSidebar) return;
    void (async () => {
      try {
        const current = await ipc("chat_current_user");
        setCurrentUserEns(current.ens ? current.ens.toLowerCase() : null);
      } catch {
        setCurrentUserEns(null);
      }
    })();
  }, [showSidebar]);

  const toggleChatSidebar = () => {
    if (!location.pathname.startsWith("/chat")) {
      navigate("/chat");
      window.setTimeout(() => {
        window.dispatchEvent(new Event("anton:open-chat-sidebar"));
      }, 0);
      return;
    }
    window.setTimeout(() => {
      window.dispatchEvent(new Event("anton:toggle-chat-sidebar"));
    }, 0);
  };

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-[#090b12] text-slate-100">
      {showSidebar ? (
        <aside className="flex w-[4.75rem] shrink-0 flex-col items-center border-r border-white/10 bg-black/30 px-3 py-5 backdrop-blur-xl">
          <button
            type="button"
            title={currentUserEns ? `Open conversations for ${shortEns(currentUserEns)}` : "Open conversations"}
            onClick={toggleChatSidebar}
            className="grid size-11 place-items-center rounded-2xl bg-gradient-to-br from-emerald-300 via-cyan-300 to-violet-400 text-sm font-black text-slate-950 shadow-[0_0_34px_rgba(45,212,191,0.2)] transition hover:scale-105"
          >
            {currentUserEns ? conversationInitial(currentUserEns) : "A"}
          </button>
          <nav className="mt-8 flex flex-1 flex-col gap-3">
            {NAV_ITEMS.map((item) => (
              <NavLink
                key={item.to}
                to={item.to}
                title={item.label}
                className={({ isActive }) =>
                  cn(
                    "grid size-10 place-items-center rounded-2xl transition",
                    isActive
                      ? "bg-emerald-400 text-emerald-950 shadow-[0_0_24px_rgba(52,211,153,0.28)]"
                      : "border border-white/10 bg-white/[0.04] text-slate-400 hover:bg-white/[0.08] hover:text-slate-100",
                  )
                }
              >
                <SidebarIcon icon={item.icon} />
              </NavLink>
            ))}
          </nav>
          <NavLink
            to="/onboarding"
            title="Lock app"
            className={({ isActive }) =>
              cn(
                "mb-1 grid size-10 place-items-center rounded-2xl transition",
                isActive
                  ? "bg-emerald-400 text-emerald-950 shadow-[0_0_24px_rgba(52,211,153,0.28)]"
                  : "border border-white/10 bg-white/[0.04] text-slate-400 hover:bg-white/[0.08] hover:text-slate-100",
              )
            }
          >
            <SidebarIcon icon="lock" />
          </NavLink>
        </aside>
      ) : null}
      <main className="min-w-0 flex-1 overflow-auto">
        <Outlet />
      </main>
    </div>
  );
}
