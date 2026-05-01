import { jsx as _jsx } from "react/jsx-runtime";
import React from "react";
import ReactDOM from "react-dom/client";
import { createBrowserRouter, RouterProvider, Navigate } from "react-router-dom";
import App from "./App";
import { OnboardingPage } from "./pages/Onboarding";
import { ChatPage } from "./pages/Chat";
import { SettingsPage } from "./pages/Settings";
import "./styles.css";
const router = createBrowserRouter([
    {
        path: "/",
        element: _jsx(App, {}),
        children: [
            { index: true, element: _jsx(Navigate, { to: "/onboarding", replace: true }) },
            { path: "onboarding", element: _jsx(OnboardingPage, {}) },
            { path: "chat", element: _jsx(ChatPage, {}) },
            { path: "chat/:ens", element: _jsx(ChatPage, {}) },
            { path: "settings", element: _jsx(SettingsPage, {}) },
        ],
    },
]);
ReactDOM.createRoot(document.getElementById("root")).render(_jsx(React.StrictMode, { children: _jsx(RouterProvider, { router: router }) }));
//# sourceMappingURL=main.js.map