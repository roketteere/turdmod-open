import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Port 5190 — distinct from turdmod-manager (5173) and turdmod-launcher (5180)
// so all three can run at once during development.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5190, strictPort: true, host: false },
  envPrefix: ["VITE_", "TAURI_"],
});
