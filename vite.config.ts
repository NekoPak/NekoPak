import { defineConfig } from "vitest/config";

// Tauri v2 expects a fixed dev port and a static `dist` output.
// src-tauri/tauri.conf.json should point
//   build.devUrl      -> http://localhost:5173
//   build.frontendDist -> ../dist
export default defineConfig({
  clearScreen: false,
  esbuild: {
    jsx: "automatic",
    jsxImportSource: "preact",
    legalComments: "none",
  },
  server: {
    port: 5173,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    // WebView2 on Windows 10/11 is evergreen Chromium.
    target: "chrome105",
    outDir: "ui-dist",
    emptyOutDir: true,
    sourcemap: false,
    cssCodeSplit: false,
    assetsInlineLimit: 0,
    reportCompressedSize: true,
  },
  test: {
    include: ["tests/unit/**/*.test.{ts,tsx}"],
    setupFiles: ["tests/unit/setup-locales.ts"],
    environment: "node",
  },
});
