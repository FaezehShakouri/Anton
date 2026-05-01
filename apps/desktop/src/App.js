import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import { NavLink, Outlet } from "react-router-dom";
import { cn } from "./lib/cn";
const NAV_ITEMS = [
    { to: "/onboarding", label: "Onboarding" },
    { to: "/chat", label: "Chat" },
    { to: "/settings", label: "Settings" },
];
export default function App() {
    return (_jsxs("div", { className: "flex h-screen w-screen flex-col overflow-hidden", children: [_jsxs("header", { className: "flex h-12 shrink-0 items-center justify-between border-b border-slate-800 bg-slate-950/80 px-4 backdrop-blur", children: [_jsxs("div", { className: "flex items-center gap-2", children: [_jsx("div", { className: "size-2 rounded-full bg-emerald-400", "aria-hidden": true }), _jsx("span", { className: "font-semibold tracking-tight", children: "Anton" }), _jsx("span", { className: "text-xs text-slate-500", children: "v0.0.0 \u2014 scaffold" })] }), _jsx("nav", { className: "flex gap-1 text-sm", children: NAV_ITEMS.map((item) => (_jsx(NavLink, { to: item.to, className: ({ isActive }) => cn("rounded-md px-3 py-1.5 transition", isActive
                                ? "bg-slate-800 text-slate-50"
                                : "text-slate-400 hover:bg-slate-900 hover:text-slate-200"), children: item.label }, item.to))) })] }), _jsx("main", { className: "flex-1 overflow-auto", children: _jsx(Outlet, {}) })] }));
}
//# sourceMappingURL=App.js.map