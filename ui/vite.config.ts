import { defineConfig } from "vite";

// The Vite config for B2's frontend. Tauri loads http://localhost:5173 in dev (see
// tauri.conf.json build.devUrl) and embeds ./dist for release, so the port is fixed
// and the output stays a self-contained bundle (no remote assets — CSP/local-first,
// specs/completed/desktop-ui-mvp.md §6).
export default defineConfig({
  // Tauri drives the terminal; don't let Vite clear its output.
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    // The OS webview is modern (WebKit / WebView2 / WebKitGTK) — target a recent
    // baseline and keep the bundle lean.
    target: "es2021",
    outDir: "dist",
    emptyOutDir: true,
  },
});
