/// <reference types="vite/client" />

/** The release version, injected at build time (the git tag, e.g. "v0.1.25")
 *  via Vite `define`; falls back to "dev" for local builds. */
declare const __APP_VERSION__: string;
