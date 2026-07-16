import NavBar from "../components/NavBar";
import WebhooksClient from "./WebhooksClient";

export const metadata = {
  title: "Strait — Webhooks",
  description:
    "Register and manage Strait webhook subscriptions: push notifications for tunnel transfer lifecycle events.",
};

export default function WebhooksPage() {
  return (
    <div className="min-h-screen bg-[#0a0a0a] text-white font-sans">
      <NavBar
        extras={
          <a href="/docs#webhooks" className="text-zinc-400 hover:text-white transition-colors">
            Webhook docs
          </a>
        }
      />
      <main className="max-w-3xl mx-auto px-6 py-12">
        <header className="mb-8">
          <div className="text-xs font-mono text-orange-400 uppercase tracking-widest mb-3">
            Developers
          </div>
          <h1 className="text-3xl md:text-4xl font-bold tracking-tight">Webhooks</h1>
          <p className="mt-3 text-zinc-400 max-w-2xl">
            Get an HMAC-signed POST whenever a matching transfer changes — no polling.
            Register below, store the credentials it returns (they are shown once), and
            point them at your backend. See the{" "}
            <a href="/docs#webhooks" className="text-orange-300 underline decoration-orange-300/40 hover:decoration-orange-300">
              docs
            </a>{" "}
            for receiver code and signature verification.
          </p>
        </header>
        <WebhooksClient />
      </main>
    </div>
  );
}
