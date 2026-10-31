import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The Rust web crate embeds the built assets and serves them under /. Absolute
// asset paths keep working on client-routed pages like /einstellungen, which
// are all served the same index.html.
export default defineConfig({
  base: "/",
  plugins: [react()],
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
