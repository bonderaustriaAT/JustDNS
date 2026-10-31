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
  },
  build: {
    outDir: "dist",
  },
});
