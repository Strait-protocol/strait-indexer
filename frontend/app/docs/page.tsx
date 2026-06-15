import Link from "next/link";

export const metadata = {
  title: "Strait — Docs",
  description:
    "Developer documentation for Strait: the project, the read API (GraphQL + REST), the data model, the contract addresses it indexes, and how to run your own node.",
};

export default function DocsPage() {
  return (
    <div className="min-h-screen bg-[#0a0a0a] text-white font-sans">
      <DocsNav />
      <main className="max-w-4xl mx-auto px-6 pt-28 pb-24 space-y-16">
        <Hero />
        <Overview />
        <DataModel />
        <Lifecycle />
        <UsingTheApi />
        <Contracts />
        <SelfHost />
        <Footer />
      </main>
    </div>
  );
}

/* ── Nav ─────────────────────────────────────────────────────────────────── */

function DocsNav() {
  return (
    <nav className="fixed top-0 inset-x-0 z-50 border-b border-white/[0.06] bg-[#0a0a0a]/80 backdrop-blur-md">
      <div className="max-w-4xl mx-auto px-6 h-16 flex items-center justify-between">
        <Link href="/" className="text-orange-400 font-mono text-xl font-bold tracking-tight">
          ⊕ Strait
        </Link>
        <div className="flex items-center gap-6 text-sm text-zinc-400">
          <Link href="/dashboard/mainnet" className="hover:text-white transition-colors">
            Explorer
          </Link>
          <a
            href="https://github.com/Godbrand0/strait"
            target="_blank"
            rel="noopener noreferrer"
            className="hover:text-white transition-colors"
          >
            GitHub
          </a>
        </div>
      </div>
    </nav>
  );
}

/* ── Sections ────────────────────────────────────────────────────────────── */

function Hero() {
  return (
    <header>
      <div className="text-xs font-mono text-orange-400 uppercase tracking-widest mb-3">
        Developer Docs
      </div>
      <h1 className="text-4xl md:text-5xl font-bold tracking-tight">Build on Strait</h1>
      <p className="mt-4 text-lg text-zinc-400 max-w-2xl">
        Strait indexes every cross-chain transfer through Hemi&apos;s Bitcoin and Ethereum
        tunnels and serves it as a plain HTTP read API. No SDK required — integrate from any
        language with a normal HTTP client.
      </p>
      <nav className="mt-6 flex flex-wrap gap-2 text-sm">
        {[
          ["Overview", "overview"],
          ["The data", "data"],
          ["Lifecycle", "lifecycle"],
          ["Using the API", "api"],
          ["Contracts", "contracts"],
          ["Self-host", "self-host"],
        ].map(([label, id]) => (
          <a
            key={id}
            href={`#${id}`}
            className="rounded-full border border-white/10 px-3 py-1 text-zinc-300 hover:border-white/30 hover:text-white transition-colors"
          >
            {label}
          </a>
        ))}
      </nav>
    </header>
  );
}

function Overview() {
  return (
    <Section id="overview" title="What Strait is">
      <p>
        Hemi tunnels move assets across three chains — Bitcoin, Hemi (an OP-Stack L2), and
        Ethereum. Strait watches the tunnel contracts on all three and reconstructs each
        transfer as a single record, tracked from initiation to finality — including
        Bitcoin-anchored (Proof-of-Proof) finality for BTC routes.
      </p>
      <p>
        It runs one node per network (mainnet, testnet). Each node is an indexer plus an HTTP
        API; all state lives in Postgres/Supabase, so the dashboard and any app you build read
        from the same source of truth.
      </p>
      <Callout>
        This is a <strong>read</strong> API — it reports on-chain reality. It never holds
        funds or initiates bridges.
      </Callout>
    </Section>
  );
}

function DataModel() {
  const rows: [string, string, string][] = [
    ["id", "UUID", "Deterministic id (Hemi tx hash + log index). Stable — use as a key."],
    ["asset", "String", "BTC, ETH, or an ERC-20 symbol."],
    ["direction", "Enum", "IN (into Hemi) or OUT (out of Hemi)."],
    ["route", "Enum", "BTC_TO_HEMI · HEMI_TO_BTC · ETH_TO_HEMI · HEMI_TO_ETH."],
    ["amount", "String", "Atomic units (satoshis or wei) as a decimal string."],
    ["sender / recipient", "String", "Origin / destination address (EVM 0x… or Bitcoin)."],
    ["status", "Enum", "INITIATED · ANCHORED · FINALIZED · FAILED · REORGED."],
    ["sourceChain / sourceTxHash / sourceBlock", "—", "The initiating leg."],
    ["destChain / destTxHash / destBlock", "—", "The counterpart leg (null until it confirms)."],
    ["popAnchored / popKeystoneBlock / popScore", "—", "Bitcoin (PoP) anchoring fields."],
    ["initiatedAt / finalizedAt", "DateTime", "Real on-chain block times of each milestone."],
  ];
  return (
    <Section id="data" title="The data: a Transfer">
      <p>
        Every result is a <Code>Transfer</Code> — the core resource. These are the fields you
        get (GraphQL camelCase; the REST API returns the same fields in snake_case).
      </p>
      <div className="overflow-hidden rounded-xl border border-white/[0.07]">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-xs uppercase tracking-wide text-zinc-500 border-b border-white/[0.07]">
              <th className="font-medium px-4 py-3">Field</th>
              <th className="font-medium px-4 py-3">Type</th>
              <th className="font-medium px-4 py-3">Meaning</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(([f, t, m]) => (
              <tr key={f} className="border-b border-white/[0.04] last:border-0">
                <td className="px-4 py-3 font-mono text-orange-300 align-top">{f}</td>
                <td className="px-4 py-3 text-zinc-400 align-top">{t}</td>
                <td className="px-4 py-3 text-zinc-300 align-top">{m}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <Callout>
        <strong>Amounts are atomic units.</strong> BTC → ÷10<sup>8</sup> (sats), ETH → ÷10
        <sup>18</sup> (wei), ERC-20 → ÷10<sup>token decimals</sup>. They&apos;re strings to
        avoid float precision loss.
      </Callout>
    </Section>
  );
}

function Lifecycle() {
  return (
    <Section id="lifecycle" title="The transfer lifecycle">
      <p>
        <Code>status</Code> is what you poll for. The progression depends on the route:
      </p>
      <Pre>{`ETH_TO_HEMI  (deposit) :  INITIATED ─────────────► FINALIZED
BTC_TO_HEMI  (deposit) :  INITIATED ──► ANCHORED ─► FINALIZED
HEMI_TO_ETH  (withdraw):  INITIATED ─────────────► FINALIZED  (after L1 challenge)
HEMI_TO_BTC  (withdraw):  INITIATED ──► ANCHORED ─► FINALIZED`}</Pre>
      <ul className="space-y-1.5 text-zinc-300">
        <li>
          <Code>INITIATED</Code> — seen on its source chain.
        </li>
        <li>
          <Code>ANCHORED</Code> — (BTC routes) committed to Bitcoin via Proof-of-Proof.
        </li>
        <li>
          <Code>FINALIZED</Code> — complete and irreversible.
        </li>
        <li>
          <Code>FAILED</Code> / <Code>REORGED</Code> — terminal failure / rolled back by a reorg.
        </li>
      </ul>
      <p className="text-zinc-400">
        A robust integration treats <Code>FINALIZED</Code> as &quot;done&quot; and anything
        else as &quot;in flight.&quot;
      </p>
    </Section>
  );
}

function UsingTheApi() {
  return (
    <Section id="api" title="Using the API">
      <p>
        The node serves on <Code>API_HOST:API_PORT</Code> (default <Code>:8080</Code>). Open{" "}
        <Code>/graphql</Code> in a browser for the interactive GraphiQL playground.
      </p>

      <h3 className="text-lg font-semibold text-white pt-2">Endpoints</h3>
      <div className="overflow-hidden rounded-xl border border-white/[0.07]">
        <table className="w-full text-sm">
          <tbody>
            {[
              ["POST /graphql", "Execute GraphQL queries"],
              ["GET /graphql", "GraphiQL playground"],
              ["GET /transfers?limit=&offset=", "REST: list transfers"],
              ["GET /transfers/:id", "REST: one transfer by UUID"],
              ["GET /health · /health/db", "Liveness · DB connectivity"],
            ].map(([e, d]) => (
              <tr key={e} className="border-b border-white/[0.04] last:border-0">
                <td className="px-4 py-3 font-mono text-orange-300 whitespace-nowrap">{e}</td>
                <td className="px-4 py-3 text-zinc-300">{d}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <h3 className="text-lg font-semibold text-white pt-2">GraphQL queries</h3>
      <Pre>{`# Recent transfers (newest first; limit clamped 1–500, default 50)
{ transfers(limit: 20) { id route asset amount status initiatedAt finalizedAt } }

# A single transfer by id
{ transfer(id: "a2ce3b2d-…") { id route status destChain destTxHash recipient } }

# All transfers for a recipient address (a wallet view)
{ transfersByRecipient(recipient: "0x64ea…951f", limit: 50) { id route amount status } }

# Search by address / tx hash / id, with optional status & route filters
{ searchTransfers(query: "bc1qwql2…", status: "FINALIZED", route: "HEMI_TO_BTC") {
    id amount status finalizedAt } }

# Aggregate stats
{ stats { totalTransfers } }`}</Pre>

      <h3 className="text-lg font-semibold text-white pt-2">Examples</h3>
      <Pre>{`// TypeScript — search via GraphQL
const res = await fetch("http://localhost:8080/graphql", {
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify({
    query: \`query($r:String!){ transfersByRecipient(recipient:$r){ id route status } }\`,
    variables: { r: recipient },
  }),
});
const { data } = await res.json();`}</Pre>
      <Pre>{`# curl — REST
curl 'http://localhost:8080/transfers?limit=20'
curl 'http://localhost:8080/transfers/a2ce3b2d-7110-520c-8999-d21d1f88d1e5'`}</Pre>

      <Callout>
        <strong>Polling for finality:</strong> there&apos;s no subscription/webhook endpoint
        yet — poll <Code>transfersByRecipient</Code> every ~10–15s and stop when{" "}
        <Code>status === &quot;FINALIZED&quot;</Code>.
      </Callout>
    </Section>
  );
}

function Contracts() {
  const rows: [string, string, string][] = [
    ["BitcoinTunnelManager", "Hemi Mainnet", "0xEAcA824F46c000fB89403846Bb57e6b913321081"],
    ["BitcoinTunnelManager", "Hemi Sepolia", "0x8221CFD3Eca3c5F9FA27b2AE774151642f1C449e"],
    ["L2StandardBridge", "Hemi (both)", "0x4200000000000000000000000000000000000010"],
    ["L1StandardBridgeProxy", "Ethereum Mainnet", "0x5eaa10F99e7e6D177eF9F74E519E319aa49f191e"],
    ["L1StandardBridgeProxy", "Ethereum Sepolia", "0xc94b1BEe63A3e101FE5F71C80F912b4F4b055925"],
    ["BitcoinKitV1", "Hemi Mainnet", "0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12"],
    ["BitcoinKit v0", "Hemi Sepolia", "0xeC9fa5daC1118963933e1A675a4EEA0009b7f215"],
    ["PoPPayoutsV2", "Hemi Sepolia", "0x4a3b61C586DB4CD219E85aC0697b66916c7457AB"],
  ];
  return (
    <Section id="contracts" title="Contracts indexed">
      <p>
        Strait watches these tunnel contracts. ETH/ERC-20 routes flow through the OP-Stack
        StandardBridge; BTC routes through the BitcoinTunnelManager, with Bitcoin state read
        via the BitcoinKit precompile on Hemi (no separate Bitcoin node required).
      </p>
      <div className="overflow-hidden rounded-xl border border-white/[0.07]">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-xs uppercase tracking-wide text-zinc-500 border-b border-white/[0.07]">
              <th className="font-medium px-4 py-3">Contract</th>
              <th className="font-medium px-4 py-3">Network</th>
              <th className="font-medium px-4 py-3">Address</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(([c, n, a]) => (
              <tr key={c + n} className="border-b border-white/[0.04] last:border-0">
                <td className="px-4 py-3 text-zinc-200 align-top whitespace-nowrap">{c}</td>
                <td className="px-4 py-3 text-zinc-400 align-top whitespace-nowrap">{n}</td>
                <td className="px-4 py-3 font-mono text-xs text-orange-300 align-top break-all">{a}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Section>
  );
}

function SelfHost() {
  return (
    <Section id="self-host" title="Run your own node">
      <p>
        Strait is open source and self-hostable. State lives in Postgres/Supabase, so the node
        is a stateless worker that resumes from its checkpoint on restart.
      </p>
      <Pre>{`git clone https://github.com/Godbrand0/strait
cd strait && cp .env.example .env

# Set HEMI_RPC_URL, ETH_RPC_URL, the contract addresses,
# DATABASE_URL, and start blocks in .env, then:
cargo run -p strait-node

# GraphQL:    http://localhost:8080/graphql
# Playground: open the same URL in a browser`}</Pre>
      <p className="text-zinc-400">
        For testnet, use <Code>.env.testnet.example</Code> (Hemi Sepolia + Ethereum Sepolia)
        and a separate database.
      </p>
    </Section>
  );
}

function Footer() {
  return (
    <footer className="border-t border-white/[0.06] pt-8 flex items-center justify-between text-sm text-zinc-500">
      <span>⊕ Strait</span>
      <div className="flex gap-5">
        <Link href="/dashboard/mainnet" className="hover:text-white transition-colors">
          Explorer
        </Link>
        <Link href="/" className="hover:text-white transition-colors">
          Home
        </Link>
      </div>
    </footer>
  );
}

/* ── Small helpers ───────────────────────────────────────────────────────── */

function Section({ id, title, children }: { id: string; title: string; children: React.ReactNode }) {
  return (
    <section id={id} className="scroll-mt-24 space-y-4">
      <h2 className="text-2xl md:text-3xl font-bold tracking-tight">{title}</h2>
      <div className="space-y-4 text-zinc-300 leading-relaxed">{children}</div>
    </section>
  );
}

function Code({ children }: { children: React.ReactNode }) {
  return <code className="font-mono text-sm text-orange-300 bg-white/[0.05] px-1.5 py-0.5 rounded">{children}</code>;
}

function Pre({ children }: { children: string }) {
  return (
    <pre className="overflow-x-auto rounded-xl border border-white/[0.07] bg-black/40 p-4 text-xs md:text-sm font-mono text-zinc-200 leading-relaxed">
      {children}
    </pre>
  );
}

function Callout({ children }: { children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-orange-500/20 bg-orange-500/[0.04] px-4 py-3 text-sm text-zinc-300">
      {children}
    </div>
  );
}
