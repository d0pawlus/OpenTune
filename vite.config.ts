/// <reference types="vitest" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },

  // `.worktrees/**` holds sibling git worktrees for parallel task branches
  // (e.g. `offline-tune`) — each has its own `node_modules`/React copy, and
  // vitest's default exclude list (`node_modules`, `.git`) doesn't cover it,
  // so an unscoped `vitest run` collects and runs their tests too, crashing
  // on duplicate React instances. Pre-existing gap, unrelated to any one
  // task; excluded here so `npm test` reflects only this checkout.
  test: {
    environment: "jsdom",
    globals: true,
    exclude: ["**/node_modules/**", "**/.git/**", "**/.worktrees/**"],
  },
}));
