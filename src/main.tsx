import React from "react";
import ReactDOM from "react-dom/client";
import "./styles/globals.css";
import "./i18n";
import App from "./app/hub/App";

function render() {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

// Outside Tauri in development (screenshots, UI review), fake the IPC with sample data.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  void import("./lib/devMock").then(render);
} else {
  render();
}
