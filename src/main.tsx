import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { Providers } from "./app/providers";
import "./styles/globals.css";

const root = document.getElementById("root");
if (root) {
  ReactDOM.createRoot(root).render(
    <React.StrictMode>
      <Providers>
        <App />
      </Providers>
    </React.StrictMode>,
  );
}
