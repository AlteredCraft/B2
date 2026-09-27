import { defineConfig } from "vite";

// Tauri loads http://localhost:5173 in dev (tauri.conf.json build.devUrl) and embeds ./dist
// for release, so the port is fixed and the bundle self-contained (CSP;
// crates/b2-desktop/CLAUDE.md).
export default defineConfig({
  // Tauri drives the terminal; don't let Vite clear its output.
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    // The OS webviews are all modern.
    target: "es2021",
    outDir: "dist",
    emptyOutDir: true,
  },
});
