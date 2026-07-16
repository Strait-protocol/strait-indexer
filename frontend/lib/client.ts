// Client-side (browser) API access.
//
// The explorer pages call the Strait API from the server (lib/strait.ts), but
// webhook registration/management runs in the browser on purpose: the one-time
// signing secret and management token travel straight from the indexer to the
// user, never through the Next.js server.

export const PUBLIC_API_URL =
  process.env.NEXT_PUBLIC_STRAIT_API_URL ?? "https://strait-indexer.onrender.com";
