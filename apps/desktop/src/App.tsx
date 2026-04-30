import { NavLink, Outlet } from "react-router-dom";
import { cn } from "./lib/cn";

const NAV_ITEMS: ReadonlyArray<{ to: string; label: string }> = [
  { to: "/onboarding", label: "Onboarding" },
  { to: "/chat", label: "Chat" },
  { to: "/settings", label: "Settings" },
];

export default function App() {
  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden">
      <header className="flex h-12 shrink-0 items-center justify-between border-b border-slate-800 bg-slate-950/80 px-4 backdrop-blur">
        <div className="flex items-center gap-2">
          <div className="size-2 rounded-full bg-emerald-400" aria-hidden />
          <span className="font-semibold tracking-tight">Axen</span>
          <span className="text-xs text-slate-500">v0.0.0 — scaffold</span>
        </div>
        <nav className="flex gap-1 text-sm">
          {NAV_ITEMS.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              className={({ isActive }) =>
                cn(
                  "rounded-md px-3 py-1.5 transition",
                  isActive
                    ? "bg-slate-800 text-slate-50"
                    : "text-slate-400 hover:bg-slate-900 hover:text-slate-200",
                )
              }
            >
              {item.label}
            </NavLink>
          ))}
        </nav>
      </header>
      <main className="flex-1 overflow-auto">
        <Outlet />
      </main>
    </div>
  );
}
