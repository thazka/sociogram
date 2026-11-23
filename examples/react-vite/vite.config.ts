import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// No sociogram-specific configuration is needed: the engine's WebAssembly file
// is found through `new URL(..., import.meta.url)`, which Vite supports in
// development and in production builds.
export default defineConfig({
  plugins: [react()],
});
