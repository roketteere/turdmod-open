import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @tauri-apps/cli drives this; the fixed port lets tauri.conf.json's devUrl
// point at it. Distinct from turdmod-manager's 5173 so both can run at once.
// Override with the standard Vite --port flag if 5180 is taken.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5180,
    strictPort: true,
    host: false,
  },
  envPrefix: ["VITE_", "TAURI_"],
});
