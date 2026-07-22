import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { QuickPanel } from "./QuickPanel";

export function selectRootComponent(search: string): typeof App {
  return new URLSearchParams(search).get("mode") === "quick" ? QuickPanel : App;
}

const RootComponent = selectRootComponent(
  typeof window === "undefined" ? "" : window.location.search,
);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <RootComponent />
  </React.StrictMode>,
);
