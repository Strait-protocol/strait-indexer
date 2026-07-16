import Link from "next/link";
import DocsSidebars from "./DocsSidebars";
import NavBar from "../components/NavBar";

export const metadata = {
  title: "Strait — Docs",
  description:
    "Developer documentation for Strait: the project, the read API (GraphQL + REST), the data model, and the contract addresses it indexes.",
};

export default function DocsPage() {
  return (
    <div className="min-h-screen bg-[#0a0a0a] text-white font-sans">
      <NavBar
        extras={
          <a
            href="https://github.com/Godbrand0/strait"
            target="_blank"
            rel="noopener noreferrer"
            className="text-zinc-400 hover:text-white transition-colors"
          >
            GitHub
          </a>
        }
      />
      <DocsSidebars />
      <div className="xl:ml-56 xl:mr-52">
        <main className="max-w-3xl mx-auto px-6 pt-12 pb-24 space-y-16">
          <Hero />
          <Overview />
          <DataModel />
          <Lifecycle />
          <UsingTheApi />
          <Webhooks />
          <Contracts />
          <Footer />
        </main>
      </div>
    </div>
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
      <nav className="mt-6 flex flex-wrap gap-2 text-sm xl:hidden">
        {[
          ["Overview", "overview"],
          ["The data", "data"],
          ["Lifecycle", "lifecycle"],
          ["Using the API", "api"],
          ["Webhooks", "webhooks"],
          ["Contracts", "contracts"],
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
    ["status", "Enum", "INITIATED · PROVING · FINALIZED · FAILED · REORGED."],
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
        <Code>status</Code> is what you poll for. Each route has a different sequence of
        on-chain events before it reaches <Code>FINALIZED</Code>.
      </p>

      {/* ETH → Hemi */}
      <div id="lifecycle-eth-to-hemi" className="scroll-mt-24 space-y-3">
        <h3 className="text-base font-semibold text-white">
          ETH → Hemi <span className="ml-2 text-xs font-normal text-zinc-500">deposit · ~2 minutes</span>
        </h3>
        <Pre>{`INITIATED ──────────────────────────────────────────► FINALIZED`}</Pre>
        <ol className="space-y-3 text-sm text-zinc-300">
          <li className="flex gap-3">
            <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-zinc-800 text-xs text-zinc-400">1</span>
            <div>
              <span className="font-medium text-white">User calls <Code>depositETH</Code> on the L1StandardBridge</span>
              <p className="mt-0.5 text-zinc-400">Funds are locked on Ethereum. Strait sees <Code>ETHBridgeInitiated</Code> and records the transfer as <Code>INITIATED</Code>.</p>
            </div>
          </li>
          <li className="flex gap-3">
            <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-zinc-800 text-xs text-zinc-400">2</span>
            <div>
              <span className="font-medium text-white">OP Stack relays the deposit to Hemi <span className="font-normal text-zinc-500">(~2 min)</span></span>
              <p className="mt-0.5 text-zinc-400">The sequencer picks up the L1 deposit and finalizes it on L2. Strait sees <Code>ETHBridgeFinalized</Code> on Hemi and advances to <Code>FINALIZED</Code>.</p>
            </div>
          </li>
        </ol>
      </div>

      {/* BTC → Hemi */}
      <div id="lifecycle-btc-to-hemi" className="scroll-mt-24 space-y-3">
        <h3 className="text-base font-semibold text-white">
          BTC → Hemi <span className="ml-2 text-xs font-normal text-zinc-500">deposit · ~1–2 hours</span>
        </h3>
        <Pre>{`INITIATED ──────────────────────────────────────────► FINALIZED`}</Pre>
        <ol className="space-y-3 text-sm text-zinc-300">
          <li className="flex gap-3">
            <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-zinc-800 text-xs text-zinc-400">1</span>
            <div>
              <span className="font-medium text-white">User sends BTC to the vault custody address</span>
              <p className="mt-0.5 text-zinc-400">Strait observes the Bitcoin UTXO and records the transfer as <Code>INITIATED</Code>.</p>
            </div>
          </li>
          <li className="flex gap-3">
            <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-zinc-800 text-xs text-zinc-400">2</span>
            <div>
              <span className="font-medium text-white">6 Bitcoin confirmations accumulate, hBTC is minted <span className="font-normal text-zinc-500">(~1 hour)</span></span>
              <p className="mt-0.5 text-zinc-400">An operator calls <Code>confirmDeposit</Code> on BitcoinTunnelManager, minting hBTC to the recipient. Strait sees <Code>DepositConfirmed</Code> and advances to <Code>FINALIZED</Code> — the user has their funds.</p>
            </div>
          </li>
          <li className="flex gap-3">
            <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-zinc-800 text-xs text-zinc-400">3</span>
            <div>
              <span className="font-medium text-white">PoP keystone anchors the Hemi block <span className="font-normal text-zinc-500">(async, ~90 min — optional)</span></span>
              <p className="mt-0.5 text-zinc-400"><Code>PoPPayoutsV2.PayoutRoundExecuted</Code> fires for the keystone covering the mint block. <Code>status</Code> stays <Code>FINALIZED</Code>; Strait sets <Code>popAnchored=true</Code>. This upgrades the transfer to Bitcoin-grade finality independently of <Code>FINALIZED</Code>.</p>
              <p className="mt-1 text-zinc-500 text-xs">Note: PoP payouts are not yet activated on mainnet as of June 2026. <Code>popAnchored</Code> stays false, but transfers still reach <Code>FINALIZED</Code> at mint.</p>
            </div>
          </li>
        </ol>
      </div>

      {/* Hemi → ETH */}
      <div id="lifecycle-hemi-to-eth" className="scroll-mt-24 space-y-3">
        <h3 className="text-base font-semibold text-white">
          Hemi → ETH <span className="ml-2 text-xs font-normal text-zinc-500">withdrawal · ~1 day</span>
        </h3>
        <Pre>{`INITIATED ──► PROVING ─────────────────────────────► FINALIZED
              (1-day challenge window)`}</Pre>
        <ol className="space-y-3 text-sm text-zinc-300">
          <li className="flex gap-3">
            <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-zinc-800 text-xs text-zinc-400">1</span>
            <div>
              <span className="font-medium text-white">User calls <Code>withdraw</Code> on the L2StandardBridge on Hemi</span>
              <p className="mt-0.5 text-zinc-400">ETH is burned on Hemi. Strait sees <Code>ETHBridgeInitiated</Code> and records the transfer as <Code>INITIATED</Code>.</p>
            </div>
          </li>
          <li className="flex gap-3">
            <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-zinc-800 text-xs text-zinc-400">2</span>
            <div>
              <span className="font-medium text-white">User calls <Code>proveWithdrawalTransaction</Code> on OptimismPortal <span className="font-normal text-zinc-500">(after output root is published, ~1 hour)</span></span>
              <p className="mt-0.5 text-zinc-400">The withdrawal is proven against the L2 output root on Ethereum. Strait sees <Code>WithdrawalProven</Code> and advances to <Code>PROVING</Code>. The 1-day challenge window begins.</p>
            </div>
          </li>
          <li className="flex gap-3">
            <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-zinc-800 text-xs text-zinc-400">3</span>
            <div>
              <span className="font-medium text-white">Challenge window elapses <span className="font-normal text-zinc-500">(~1 day after proving)</span></span>
              <p className="mt-0.5 text-zinc-400">Anyone calls <Code>finalizeWithdrawalTransaction</Code> on OptimismPortal. ETH is released on Ethereum. Strait sees <Code>ETHBridgeFinalized</Code> on L1 and advances to <Code>FINALIZED</Code>.</p>
            </div>
          </li>
        </ol>
      </div>

      {/* Hemi → BTC */}
      <div id="lifecycle-hemi-to-btc" className="scroll-mt-24 space-y-3">
        <h3 className="text-base font-semibold text-white">
          Hemi → BTC <span className="ml-2 text-xs font-normal text-zinc-500">withdrawal · ~2–14 hours</span>
        </h3>
        <Pre>{`INITIATED ──────────────────────────────────────────► FINALIZED`}</Pre>
        <ol className="space-y-3 text-sm text-zinc-300">
          <li className="flex gap-3">
            <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-zinc-800 text-xs text-zinc-400">1</span>
            <div>
              <span className="font-medium text-white">User calls <Code>initiateWithdrawal</Code> on BitcoinTunnelManager</span>
              <p className="mt-0.5 text-zinc-400">hBTC is burned on Hemi. Strait sees <Code>WithdrawalInitiated</Code> and records the transfer as <Code>INITIATED</Code>. A uuid is embedded in the event for cross-chain matching.</p>
            </div>
          </li>
          <li className="flex gap-3">
            <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-zinc-800 text-xs text-zinc-400">2</span>
            <div>
              <span className="font-medium text-white">Vault operator pays out on Bitcoin <span className="font-normal text-zinc-500">(up to ~14 hours)</span></span>
              <p className="mt-0.5 text-zinc-400">The operator broadcasts a Bitcoin transaction with the uuid in an <Code>OP_RETURN</Code> output. Strait matches it by uuid and advances to <Code>FINALIZED</Code>.</p>
              <p className="mt-1 text-zinc-500 text-xs">If the operator misses the deadline, anyone can call <Code>challengeWithdrawal</Code> on <Code>BitcoinTunnelManager</Code>. On success, the contract re-mints hBTC to the original sender and Strait marks the transfer <Code>FAILED</Code>.</p>
            </div>
          </li>
        </ol>
      </div>

      <div className="rounded-xl border border-white/[0.06] bg-white/[0.02] px-5 py-4 text-sm text-zinc-400 space-y-1">
        <p><span className="text-white font-medium">FAILED</span> — a terminal error (e.g. challenge succeeded, vault defaulted). No further transitions.</p>
        <p><span className="text-white font-medium">REORGED</span> — a chain reorg retracted the source event. Strait rolls back the transfer. A duplicate may re-appear if the tx is re-included.</p>
        <p className="pt-1">A robust integration treats <Code>FINALIZED</Code> as &quot;done&quot; and polls on any other non-terminal status.</p>
      </div>
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

      <h3 id="api-endpoints" className="text-lg font-semibold text-white pt-2 scroll-mt-24">Endpoints</h3>
      <div className="overflow-hidden rounded-xl border border-white/[0.07]">
        <table className="w-full text-sm">
          <tbody>
            {[
              ["POST /graphql", "Execute GraphQL queries"],
              ["GET /graphql", "GraphiQL playground"],
              ["GET /transfers?limit=&offset=", "REST: list transfers"],
              ["GET /transfers/:id", "REST: one transfer by UUID"],
              ["POST /webhooks", "Register a webhook subscription"],
              ["GET · DELETE /webhooks/:id", "Inspect · remove a subscription (token-gated)"],
              ["GET /webhooks/:id/deliveries", "Last 20 delivery attempts (token-gated)"],
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

      <h3 id="api-graphql" className="text-lg font-semibold text-white pt-2 scroll-mt-24">GraphQL queries</h3>
      <Pre>{`# Recent transfers (newest first; limit clamped 1–500, default 50)
{ transfers(limit: 20) { id route asset amount status initiatedAt finalizedAt } }

# A single transfer by id
{ transfer(id: "a2ce3b2d-…") { id route status destChain destTxHash recipient } }

# All transfers for a recipient address (a wallet view)
{ transfersByRecipient(recipient: "0x64ea…951f", limit: 50) { id route amount status } }

# Search by address / tx hash / id, with optional status & route filters
{ searchTransfers(query: "bc1qwql2…", status: "FINALIZED", route: "HEMI_TO_BTC") {
    id amount status finalizedAt } }

# Aggregate stats — optionally scoped to a window
{ stats { totalTransfers finalized failed } }
{ stats(window: LAST_24H) { totalTransfers finalized } }

# Time-bucketed analytics: count + volume per route/asset.
# window: LAST_24H | LAST_7D | LAST_30D | ALL_TIME · granularity: DAY | WEEK | MONTH
# volume is atomic units (sats/wei) per asset — convert client-side.
{ analyticsSeries(window: LAST_30D, granularity: DAY) {
    bucketStart route asset transferCount volume } }

# Which route dominates a window (share is 0–1 of total transfers)
{ routeBreakdown(window: LAST_7D) { route transferCount share } }`}</Pre>

      <h3 id="api-examples" className="text-lg font-semibold text-white pt-2 scroll-mt-24">Examples</h3>
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
        <strong>Watching for finality:</strong> prefer a{" "}
        <a href="#webhooks" className="text-orange-300 underline decoration-orange-300/40 hover:decoration-orange-300">
          webhook
        </a>{" "}
        — Strait pushes the change to you the moment it lands. If you&apos;d rather poll:
        you know your source tx hash (you just submitted it), so use{" "}
        <Code>searchTransfers(query: &quot;0xyourtxhash&quot;)</Code> every ~10–15s rather
        than matching by amount/route/time — it finds your transfer directly instead of
        guessing. Fall back to <Code>transfersByRecipient</Code> only if you don&apos;t
        have a tx hash yet. Stop when <Code>status === &quot;FINALIZED&quot;</Code> (or{" "}
        <Code>FAILED</Code> / <Code>REORGED</Code>).
      </Callout>
    </Section>
  );
}

function Webhooks() {
  return (
    <Section id="webhooks" title="Webhooks">
      <p>
        Push notifications for transfer lifecycle events: register a URL and Strait POSTs an
        HMAC-signed JSON payload to it whenever a matching transfer changes. Deliveries are
        backed by a durable outbox — a node restart never drops one — and failed POSTs retry
        with exponential backoff (10s → 24h, 8 attempts). Delivery is{" "}
        <strong>at-least-once</strong>: dedupe on the <Code>X-Strait-Delivery</Code> header.
      </p>

      <h3 id="webhooks-register" className="text-lg font-semibold text-white pt-2 scroll-mt-24">Registering</h3>
      <Pre>{`curl -X POST http://localhost:8080/webhooks \\
  -H 'content-type: application/json' \\
  -d '{
    "url": "https://example.com/strait-hook",
    "routes":   ["HEMI_TO_BTC", "HEMI_TO_ETH"],
    "assets":   ["BTC", "ETH"],
    "statuses": ["FINALIZED", "FAILED"]
  }'`}</Pre>
      <p>
        Filters are optional — omit a dimension to match everything on it. The URL must be
        public <Code>http(s)</Code>; loopback and private-network hosts are rejected.
      </p>
      <Callout>
        <strong>The response contains two credentials, shown exactly once.</strong>{" "}
        <Code>signing_secret</Code> is the HMAC key every delivery to you is signed with;{" "}
        <Code>management_token</Code> is required to inspect or delete the subscription
        later. Store both immediately — the API never discloses them again. Lose them and
        you re-register.
      </Callout>

      <h3 id="webhooks-deliveries" className="text-lg font-semibold text-white pt-2 scroll-mt-24">Deliveries</h3>
      <div className="overflow-hidden rounded-xl border border-white/[0.07]">
        <table className="w-full text-sm">
          <tbody>
            {[
              ["X-Strait-Signature", "sha256=<hex HMAC-SHA256 of the raw body under your signing_secret>"],
              ["X-Strait-Event", "transfer.created · transfer.status_changed · transfer.pop_anchored · transfer.retracted"],
              ["X-Strait-Delivery", "Unique delivery id — your dedupe key"],
            ].map(([h, d]) => (
              <tr key={h} className="border-b border-white/[0.04] last:border-0">
                <td className="px-4 py-3 font-mono text-orange-300 whitespace-nowrap align-top">{h}</td>
                <td className="px-4 py-3 text-zinc-300">{d}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p>
        The body is{" "}
        <Code>{`{ "event", "timestamp", "transfer": { … } }`}</Code> where{" "}
        <Code>transfer</Code> has the same snake_case shape as <Code>GET /transfers</Code>{" "}
        rows. Respond with any <Code>2xx</Code> within 10 seconds to acknowledge — anything
        else (or a timeout) schedules a retry.
      </p>

      <h3 id="webhooks-verify" className="text-lg font-semibold text-white pt-2 scroll-mt-24">Verifying signatures</h3>
      <p>
        Always verify before trusting a payload — anyone who discovers your endpoint URL can
        POST fake events to it; only Strait knows your <Code>signing_secret</Code>. Verify
        over the <strong>raw request bytes</strong>: re-serializing parsed JSON can reorder
        keys and break the digest.
      </p>
      <Pre>{`import { createHmac, timingSafeEqual } from "node:crypto";

function verify(rawBody /* Buffer */, signatureHeader, secret) {
  const expected =
    "sha256=" + createHmac("sha256", secret).update(rawBody).digest("hex");
  return timingSafeEqual(Buffer.from(signatureHeader), Buffer.from(expected));
}`}</Pre>
      <p>
        What those two calls do (<Code>node:crypto</Code> ships with Node — no npm package):{" "}
        <Code>createHmac</Code> recomputes the HMAC-SHA256 signature Strait attached — only a
        holder of your <Code>signing_secret</Code> can produce it, and changing one byte of
        the body changes it completely, so a match proves the payload is genuinely from
        Strait and untampered. <Code>timingSafeEqual</Code> compares the signatures in{" "}
        <strong>constant time</strong>: a plain <Code>===</Code> returns faster the earlier
        the first mismatch is, and that timing difference — measured over many forged
        requests — can leak a valid signature byte by byte. Constant-time comparison closes
        that side channel.
      </p>

      <h3 id="webhooks-credentials" className="text-lg font-semibold text-white pt-2 scroll-mt-24">Credentials &amp; subscriptions</h3>
      <p>
        <strong className="text-white">One subscription per service, not per end-user.</strong>{" "}
        Strait doesn&apos;t know about your users — it notifies <em>you</em> about transfers.
        A wallet with 10,000 users runs <strong>one</strong> subscription (per environment)
        pointed at its backend; when a delivery arrives, match{" "}
        <Code>transfer.recipient</Code> (or <Code>sender</Code>) against your own users table
        to decide who to notify.
      </p>
      <div className="overflow-hidden rounded-xl border border-white/[0.07]">
        <table className="w-full text-sm">
          <tbody>
            {[
              ["id", "Not secret — config/env; needed for GET / DELETE /webhooks/:id"],
              ["signing_secret", "Secret — env var / secret manager; your receiver reads it to verify deliveries"],
              ["management_token", "Secret — secret manager; only needed to inspect or delete the subscription"],
            ].map(([v, d]) => (
              <tr key={v} className="border-b border-white/[0.04] last:border-0">
                <td className="px-4 py-3 font-mono text-orange-300 whitespace-nowrap align-top">{v}</td>
                <td className="px-4 py-3 text-zinc-300">{d}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p>
        Register separately for staging and production, each with its own URL and secrets. To
        rotate, register a new subscription, accept both secrets during the cutover, then
        delete the old one. The API never re-discloses secrets — if you lose the management
        token, keep returning <Code>2xx</Code> (and ignore the events) so retries don&apos;t
        pile up, and ask the operator to remove the row.
      </p>

      <h3 id="webhooks-integrate" className="text-lg font-semibold text-white pt-2 scroll-mt-24">Integrating with your backend</h3>
      <p>
        Two rules for every receiver: verify the signature over the <strong>raw request
        bytes</strong> (parse JSON only after the check — body-parsing middleware that
        re-serializes breaks the digest), and <strong>acknowledge fast</strong> — return{" "}
        <Code>2xx</Code>, then do your real work asynchronously. A handler slower than 10s
        looks like a failure and gets retried, which you&apos;ll then process twice.
      </p>
      <p className="text-sm text-zinc-400">Express:</p>
      <Pre>{`import express from "express";
import { createHmac, timingSafeEqual } from "node:crypto";

const app = express();
const SECRET = process.env.STRAIT_SIGNING_SECRET;

// express.raw (NOT express.json) so we verify the exact bytes.
app.post("/strait-hook", express.raw({ type: "application/json" }), (req, res) => {
  const sig = req.get("X-Strait-Signature") ?? "";
  const expected = "sha256=" + createHmac("sha256", SECRET).update(req.body).digest("hex");
  if (sig.length !== expected.length ||
      !timingSafeEqual(Buffer.from(sig), Buffer.from(expected))) {
    return res.status(401).send("bad signature");
  }

  res.status(200).send("ok"); // ack first — work after

  const deliveryId = req.get("X-Strait-Delivery"); // your dedupe key
  const { event, transfer } = JSON.parse(req.body);
  if (event === "transfer.status_changed" && transfer.status === "FINALIZED") {
    // mark the user's bridge complete, send a push notification…
  }
});`}</Pre>
      <p className="text-sm text-zinc-400">Next.js (App Router route handler):</p>
      <Pre>{`// app/api/strait-hook/route.ts
import { createHmac, timingSafeEqual } from "node:crypto";

export async function POST(req: Request) {
  const raw = Buffer.from(await req.arrayBuffer()); // raw bytes, not req.json()
  const sig = req.headers.get("x-strait-signature") ?? "";
  const expected = "sha256=" +
    createHmac("sha256", process.env.STRAIT_SIGNING_SECRET!).update(raw).digest("hex");
  if (sig.length !== expected.length ||
      !timingSafeEqual(Buffer.from(sig), Buffer.from(expected))) {
    return new Response("bad signature", { status: 401 });
  }

  const deliveryId = req.headers.get("x-strait-delivery"); // your dedupe key
  const { event, transfer } = JSON.parse(raw.toString());
  // handle the event (keep it quick, or hand off to a queue)…
  return new Response("ok");
}`}</Pre>
      <p>
        <strong className="text-white">Recommended pattern — webhook + poll reconciliation.</strong>{" "}
        On submit, store a <Code>pending</Code> row in your DB keyed by the source tx hash.
        On webhook, match <Code>transfer.source_tx_hash</Code> to that row, update it,
        notify the user. Then reconcile: webhooks are at-least-once, but if your endpoint is
        down longer than the retry window (~1.5 days) a delivery can permanently fail — so
        sweep your still-pending rows every ~10 minutes and resolve them by polling:
      </p>
      <Pre>{`async function fetchByTxHash(txHash) {
  const res = await fetch("http://localhost:8080/graphql", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      query: \`query($q: String) {
        searchTransfers(query: $q, limit: 1) {
          id status route asset amount destTxHash finalizedAt
        }
      }\`,
      variables: { q: txHash },
    }),
  });
  const { data } = await res.json();
  return data?.searchTransfers?.[0] ?? null;
}`}</Pre>
      <p>
        The webhook gives you low latency; the sweep guarantees you never miss a terminal
        state. Both read the same records, so they can share handling code.
      </p>

      <h3 id="webhooks-manage" className="text-lg font-semibold text-white pt-2 scroll-mt-24">Managing a subscription</h3>
      <p>
        The{" "}
        <Link href="/webhooks" className="text-orange-300 underline decoration-orange-300/40 hover:decoration-orange-300">
          Webhooks page
        </Link>{" "}
        does all of this in the browser — register, inspect delivery history, delete. The
        same operations over curl:
      </p>
      <Pre>{`# Inspect (metadata only — secrets are never returned)
curl http://localhost:8080/webhooks/<id> -H 'X-Management-Token: <token>'

# Last 20 delivery attempts: event, status, attempt count, response time, error
curl http://localhost:8080/webhooks/<id>/deliveries -H 'X-Management-Token: <token>'

# Unsubscribe (pending deliveries are removed with it)
curl -X DELETE http://localhost:8080/webhooks/<id> -H 'X-Management-Token: <token>'`}</Pre>
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
    ["PoPPayoutsV2", "Hemi Mainnet", "0x9a23ab7cb11cfb96e577da52a6ad5211ff24434b"],
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
      <Callout>
        <strong>PoP anchoring status (June 2026):</strong> The <Code>PoPPayoutsV2</Code>{" "}
        contracts are deployed on mainnet but <Code>mintPoPRewards()</Code> has not yet been
        called — <Code>lastBlockRewarded = 0</Code> on both deployments.{" "}
        <Code>BTC_TO_HEMI</Code> deposits already reach <Code>FINALIZED</Code> at the Hemi
        mint — PoP anchoring is tracked separately via <Code>popAnchored</Code>. Strait will
        set <Code>popAnchored=true</Code> automatically once{" "}
        <Code>PayoutRoundExecuted</Code> events start firing.
      </Callout>
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
