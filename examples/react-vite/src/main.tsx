import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";

// StrictMode mounts, unmounts and mounts every component again in development.
// The sociogram wrapper is written to survive that.
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
