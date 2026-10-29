import React from "react";
import ReactDOM from "react-dom/client";
import "./styles/globals.css";
import "./styles/overlay.css";
import "./i18n";
import { BubbleApp } from "./app/overlay/BubbleApp";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <BubbleApp />
  </React.StrictMode>,
);
