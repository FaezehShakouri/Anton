import React from "react";
import ReactDOM from "react-dom/client";
import { createBrowserRouter, RouterProvider, Navigate } from "react-router-dom";
import App from "./App.tsx";
import { OnboardingPage } from "./pages/Onboarding.tsx";
import { ChatPage } from "./pages/Chat.tsx";
import { SettingsPage } from "./pages/Settings.tsx";
import "./styles.css";

const router = createBrowserRouter([
  {
    path: "/",
    element: <App />,
    children: [
      { index: true, element: <Navigate to="/onboarding" replace /> },
      { path: "onboarding", element: <OnboardingPage /> },
      { path: "chat", element: <ChatPage /> },
      { path: "chat/:ens", element: <ChatPage /> },
      { path: "settings", element: <SettingsPage /> },
    ],
  },
]);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <RouterProvider router={router} />
  </React.StrictMode>,
);
