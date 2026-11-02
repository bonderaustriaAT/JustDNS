import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { VitePWA } from "vite-plugin-pwa";

// The Rust web crate embeds the built assets and serves them under /. Absolute
// asset paths keep working on client-routed pages like /einstellungen, which
// are all served the same index.html.
// The release version is the git tag, passed in by the image build as the
// APP_VERSION env var and baked into the bundle. Local builds show "dev".
// Read via globalThis so no @types/node dependency is needed just for this.
const appVersion =
  (globalThis as { process?: { env?: Record<string, string | undefined> } }).process?.env
    ?.APP_VERSION ?? "dev";

export default defineConfig({
  base: "/",
  define: {
    __APP_VERSION__: JSON.stringify(appVersion),
  },
  plugins: [
    react(),
    // Installable PWA: generate the manifest and a Workbox service worker that
    // precaches the app shell. The API is live, per-session data behind a login,
    // so it is never cached — navigations fall back to index.html for the SPA,
    // but anything under /api/ is left to the network.
    VitePWA({
      registerType: "autoUpdate",
      injectRegister: "auto",
      includeAssets: ["favicon.svg", "icons/apple-touch-icon.png"],
      workbox: {
        globPatterns: ["**/*.{js,css,html,svg,png,woff,woff2}"],
        navigateFallback: "/index.html",
        navigateFallbackDenylist: [/^\/api\//],
      },
      manifest: {
        name: "JustDNS",
        short_name: "JustDNS",
        description: "Network-wide DNS filter and rewrites for your home network.",
        lang: "de",
        start_url: "/",
        scope: "/",
        display: "standalone",
        background_color: "#0f2b2a",
        theme_color: "#0f2b2a",
        icons: [
          { src: "icons/icon.svg", type: "image/svg+xml", sizes: "any" },
          { src: "icons/icon-192.png", type: "image/png", sizes: "192x192" },
          { src: "icons/icon-512.png", type: "image/png", sizes: "512x512" },
          {
            src: "icons/icon-maskable-512.png",
            type: "image/png",
            sizes: "512x512",
            purpose: "maskable",
          },
        ],
      },
    }),
  ],
  server: {
    host: "127.0.0.1",
    port: 5173,
    // Forward API calls to a locally running justdns binary during development;
    // in production the Rust crate serves the UI and API on the same origin.
    proxy: {
      "/api": "http://127.0.0.1:8080",
    },
  },
  build: {
    outDir: "dist",
  },
});
