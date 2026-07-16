export default function Home() {
  return (
    <div className="min-h-screen bg-[#0a0a0a] text-white font-sans">
      <Nav />
      <Hero />
      <StatsBar />
      <Problem />
      <HowItWorks />
      <WhyBetter />
      <Features />
      <EcosystemImpact />
      <BuiltOnStrait />
      <Roadmap />
      <CTA />
      <Footer />
    </div>
  );
}

/* ─── Navigation ──────────────────────────────────────────────────────────── */

function Nav() {
  return (
    <nav className="fixed top-0 inset-x-0 z-50 border-b border-white/[0.06] bg-[#0a0a0a]/80 backdrop-blur-md">
      <div className="max-w-6xl mx-auto px-6 h-16 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-orange-400 font-mono text-xl font-bold tracking-tight">
            ⊕ Strait
          </span>
        </div>
        <div className="hidden md:flex items-center gap-8 text-sm text-zinc-400">
          <a href="/dashboard" className="hover:text-white transition-colors">Explorer</a>
          <a href="/docs" className="hover:text-white transition-colors">Docs</a>
          <a href="/webhooks" className="hover:text-white transition-colors">Webhooks</a>
        </div>
        <div className="flex items-center gap-3">
          <a
            href="https://github.com/strait-data/strait"
            className="flex items-center gap-2 text-sm text-zinc-400 hover:text-white transition-colors"
            target="_blank"
            rel="noopener noreferrer"
          >
            <GithubIcon />
            GitHub
          </a>
          <a
            href="/docs"
            className="text-sm bg-orange-500 hover:bg-orange-400 text-black font-semibold px-4 py-2 rounded-full transition-colors"
          >
            Get API Access
          </a>
        </div>
      </div>
    </nav>
  );
}

/* ─── Hero ────────────────────────────────────────────────────────────────── */

function Hero() {
  return (
    <section className="pt-40 pb-24 px-6 text-center relative overflow-hidden">
      {/* Background glow */}
      <div className="absolute top-0 left-1/2 -translate-x-1/2 w-[800px] h-[400px] bg-orange-500/10 rounded-full blur-3xl pointer-events-none" />

      <div className="relative max-w-4xl mx-auto">
        <div className="inline-flex items-center gap-2 text-xs font-mono text-orange-400 border border-orange-500/30 bg-orange-500/10 px-3 py-1.5 rounded-full mb-8">
          <span className="w-1.5 h-1.5 rounded-full bg-orange-400 animate-pulse" />
          Live on Hemi Testnet
        </div>

        <h1 className="text-5xl md:text-7xl font-bold tracking-tight leading-[1.08] mb-6">
          The missing data layer
          <br />
          <span className="text-orange-400">for Hemi tunnels.</span>
        </h1>

        <p className="text-lg md:text-xl text-zinc-400 max-w-2xl mx-auto leading-relaxed mb-10">
          Strait indexes every asset crossing Hemi&rsquo;s Bitcoin and Ethereum bridges.
          Three chains ingested. One unified record per transfer. Real-time, reorg-safe,
          queryable from day one.
        </p>

        <div className="flex flex-col sm:flex-row items-center justify-center gap-4 mb-16">
          <a
            href="/docs"
            className="w-full sm:w-auto px-8 py-3.5 bg-orange-500 hover:bg-orange-400 text-black font-semibold rounded-full transition-colors text-sm"
          >
            Start querying →
          </a>
          <a
            href="#how-it-works"
            className="w-full sm:w-auto px-8 py-3.5 border border-white/10 hover:border-white/30 text-zinc-300 rounded-full transition-colors text-sm"
          >
            See how it works
          </a>
        </div>

        {/* Hero code block */}
        <div className="text-left bg-zinc-900/80 border border-white/[0.08] rounded-2xl overflow-hidden shadow-2xl max-w-2xl mx-auto">
          <div className="flex items-center gap-2 px-4 py-3 border-b border-white/[0.06]">
            <span className="w-3 h-3 rounded-full bg-red-500/70" />
            <span className="w-3 h-3 rounded-full bg-yellow-500/70" />
            <span className="w-3 h-3 rounded-full bg-green-500/70" />
            <span className="ml-2 text-xs text-zinc-500 font-mono">GraphQL · api.strait.xyz/graphql</span>
          </div>
          <pre className="p-5 text-sm font-mono overflow-x-auto leading-relaxed">
            <code>
              <span className="text-zinc-500">{"# Query any tunnel transfer — Bitcoin to Hemi\n"}</span>
              <span className="text-orange-300">{"tunnelTransfer"}</span>
              <span className="text-zinc-300">{"(id: "}</span>
              <span className="text-green-400">{"\"a3f7...c91d\""}</span>
              <span className="text-zinc-300">{")"}</span>
              <span className="text-zinc-300">{" {\n"}</span>
              <span className="text-blue-300">{"  asset       "}</span>
              <span className="text-zinc-500">{"# BTC | ETH | ERC20\n"}</span>
              <span className="text-blue-300">{"  route       "}</span>
              <span className="text-zinc-500">{"# BTC_TO_HEMI\n"}</span>
              <span className="text-blue-300">{"  amount\n"}</span>
              <span className="text-blue-300">{"  status      "}</span>
              <span className="text-zinc-500">{"# FINALIZED\n"}</span>
              <span className="text-blue-300">{"  sourceTx    "}</span>
              <span className="text-zinc-300">{"{ hash blockNumber chain }\n"}</span>
              <span className="text-blue-300">{"  destinationTx "}</span>
              <span className="text-zinc-300">{"{ hash blockNumber chain }\n"}</span>
              <span className="text-blue-300">{"  popProofs   "}</span>
              <span className="text-zinc-300">{"{ bitcoinTxid bitcoinBlock }\n"}</span>
              <span className="text-zinc-300">{"}"}</span>
            </code>
          </pre>
        </div>
      </div>
    </section>
  );
}

/* ─── Stats Bar ───────────────────────────────────────────────────────────── */

function StatsBar() {
  const stats = [
    { value: "3", label: "Chains indexed" },
    { value: "4", label: "Tunnel routes" },
    { value: "<1s", label: "Webhook latency" },
    { value: "100%", label: "Reorg-safe" },
    { value: "$1.2B+", label: "Hemi TVL tracked" },
  ];

  return (
    <div className="border-y border-white/[0.06] bg-white/[0.02] py-8 px-6">
      <div className="max-w-5xl mx-auto grid grid-cols-2 md:grid-cols-5 gap-8 text-center">
        {stats.map((s) => (
          <div key={s.label}>
            <div className="text-2xl font-bold text-orange-400">{s.value}</div>
            <div className="text-xs text-zinc-500 mt-1">{s.label}</div>
          </div>
        ))}
      </div>
    </div>
  );
}

/* ─── Problem ─────────────────────────────────────────────────────────────── */

function Problem() {
  const failures = [
    {
      tool: "The Graph / Goldsky",
      icon: "◈",
      problem:
        "Index Hemi&rsquo;s EVM events just fine — but are blind to Bitcoin entirely. They show you the destination half of every tunnel transfer with no link to the Bitcoin deposit that triggered it.",
    },
    {
      tool: "Block Explorers",
      icon: "◈",
      problem:
        "Show individual transactions on individual chains. No aggregation, no streaming, no cross-chain joins, no analytics. Three tabs open is not an indexer.",
    },
    {
      tool: "Bridge dashboards",
      icon: "◈",
      problem:
        "Treat bridges as opaque entities. They don&rsquo;t understand Hemi&rsquo;s tunneling semantics, PoP anchoring, or the dual nature of BTC tunnels versus ETH tunnels.",
    },
    {
      tool: "Custom in-house solutions",
      icon: "◈",
      problem:
        "Every team builds a partial version of the same thing in isolation. Most rely on polling and miss reorg edge cases. None expose a usable API.",
    },
  ];

  return (
    <section className="py-28 px-6" id="problem">
      <div className="max-w-5xl mx-auto">
        <div className="text-center mb-16">
          <div className="text-xs font-mono text-orange-400 uppercase tracking-widest mb-4">The problem</div>
          <h2 className="text-4xl md:text-5xl font-bold tracking-tight mb-6">
            Tunnel data is everywhere.
            <br />
            <span className="text-zinc-500">Joined, it exists nowhere.</span>
          </h2>
          <p className="text-zinc-400 max-w-2xl mx-auto text-lg leading-relaxed">
            A complete tunnel flow involves at least three blockchain states — Bitcoin initiation,
            Hemi PoP anchoring, and destination minting — with different finality semantics and no
            shared identifier in raw form. Reconstructing a single transfer requires stateful,
            ordered joining across three asynchronous data sources. No existing tool does it.
          </p>
        </div>

        <div className="grid md:grid-cols-2 gap-4">
          {failures.map((f) => (
            <div
              key={f.tool}
              className="bg-white/[0.03] border border-white/[0.07] rounded-2xl p-6 hover:border-white/[0.12] transition-colors"
            >
              <div className="flex items-center gap-3 mb-3">
                <span className="text-red-400 text-lg">{f.icon}</span>
                <span className="font-semibold text-white">{f.tool}</span>
              </div>
              <p className="text-zinc-400 text-sm leading-relaxed" dangerouslySetInnerHTML={{ __html: f.problem }} />
            </div>
          ))}
        </div>

        <div className="mt-10 bg-orange-500/10 border border-orange-500/20 rounded-2xl p-8 text-center">
          <p className="text-orange-200 text-lg font-medium">
            The result: every team that needs canonical tunnel data is building partial versions
            of the same thing in isolation — and getting reorgs wrong.
          </p>
        </div>
      </div>
    </section>
  );
}

/* ─── How It Works ────────────────────────────────────────────────────────── */

function HowItWorks() {
  const layers = [
    {
      step: "01",
      title: "Three-chain ingestion",
      description:
        "Independent ingesters poll Bitcoin (via RPC), Hemi, and Ethereum (via alloy). Each runs its own reorg-detection window. Bitcoin deposits watch custody addresses and OP_RETURN data encoding the Hemi destination. EVM ingesters decode tunnel contract events.",
      tags: ["bitcoincore-rpc", "alloy", "tokio"],
    },
    {
      step: "02",
      title: "Join engine",
      description:
        "A stateful engine consumes all three event streams over mpsc channels. It matches related cross-chain events using the Bitcoin txid or Ethereum tx hash as the join key, and drives a per-transfer state machine from INITIATED through to FINALIZED.",
      tags: ["state machine", "cross-chain join", "reorg-aware"],
    },
    {
      step: "03",
      title: "Reorg handling",
      description:
        "Every ingester keeps a rolling block-hash window beyond its confirmation depth. On each new block, the parent hash is verified. A mismatch triggers backward walking to the fork point, a BlockReorg event, and retraction of any in-flight transfers that referenced reorged transactions.",
      tags: ["6-block BTC window", "3-block Hemi", "12-block ETH"],
    },
    {
      step: "04",
      title: "Serve",
      description:
        "The unified TunnelTransfer dataset is written to Postgres and served via a GraphQL API (async-graphql + axum) with cursor pagination, filters, and time-windowed aggregations. Webhooks fan out status updates in real time, signed with HMAC-SHA256.",
      tags: ["GraphQL", "webhooks", "Postgres"],
    },
  ];

  return (
    <section className="py-28 px-6 bg-white/[0.015]" id="how-it-works">
      <div className="max-w-5xl mx-auto">
        <div className="text-center mb-16">
          <div className="text-xs font-mono text-orange-400 uppercase tracking-widest mb-4">Architecture</div>
          <h2 className="text-4xl md:text-5xl font-bold tracking-tight mb-6">
            How Strait works
          </h2>
          <p className="text-zinc-400 max-w-xl mx-auto text-lg leading-relaxed">
            Three ingesters. One join engine. A single, coherent record per transfer.
          </p>
        </div>

        {/* Architecture diagram */}
        <div className="bg-zinc-900/60 border border-white/[0.08] rounded-2xl p-6 mb-16 overflow-x-auto">
          <pre className="text-xs font-mono text-zinc-400 leading-5 text-center whitespace-pre">{`
  Bitcoin Node          Hemi RPC              Ethereum RPC
       |                    |                      |
  +----+----+          +----+----+           +-----+-----+
  | Bitcoin |          |   EVM   |           |    EVM    |
  |Ingester |          |Ingester |           | Ingester  |
  |  (BTC)  |          | (Hemi)  |           |   (ETH)   |
  +----+----+          +----+----+           +-----+-----+
       |   RawEvent          |   RawEvent           |  RawEvent
       +--------------+------+-----------------------+
                      |  mpsc channel
                 +----+--------+
                 | Join Engine |  <- state machine per transfer
                 | (strait-   |     INITIATED -> ANCHORED -> FINALIZED
                 |   join)    |                         \\-> REORGED
                 +----+--------+
                      |  TunnelTransferUpdate
              +-------+----------+
              |  strait-store    |  <- Postgres (sqlx)
              +-------+----------+
                      |
           +----------+-----------+
           |  strait-api (axum)   |
           |  GraphQL  | Webhooks |
           +----------------------+
`}</pre>
        </div>

        <div className="grid md:grid-cols-2 gap-6">
          {layers.map((l) => (
            <div
              key={l.step}
              className="bg-white/[0.03] border border-white/[0.07] rounded-2xl p-6 hover:border-orange-500/30 transition-colors"
            >
              <div className="flex items-start justify-between mb-4">
                <span className="text-4xl font-bold text-orange-500/20 font-mono">{l.step}</span>
              </div>
              <h3 className="text-lg font-semibold text-white mb-3">{l.title}</h3>
              <p className="text-zinc-400 text-sm leading-relaxed mb-4">{l.description}</p>
              <div className="flex flex-wrap gap-2">
                {l.tags.map((t) => (
                  <span
                    key={t}
                    className="text-xs font-mono text-orange-400 bg-orange-500/10 border border-orange-500/20 px-2.5 py-1 rounded-full"
                  >
                    {t}
                  </span>
                ))}
              </div>
            </div>
          ))}
        </div>

        {/* Transfer lifecycle */}
        <div className="mt-12 bg-zinc-900/60 border border-white/[0.08] rounded-2xl p-8">
          <h3 className="text-sm font-mono text-zinc-400 uppercase tracking-widest mb-6">Transfer lifecycle</h3>
          <div className="flex flex-wrap items-center gap-3 text-sm font-mono">
            {[
              { label: "INITIATED", color: "text-yellow-400 bg-yellow-500/10 border-yellow-500/20" },
              { label: "→", color: "text-zinc-600", arrow: true },
              { label: "ANCHORED", color: "text-blue-400 bg-blue-500/10 border-blue-500/20" },
              { label: "→", color: "text-zinc-600", arrow: true },
              { label: "FINALIZED", color: "text-green-400 bg-green-500/10 border-green-500/20" },
            ].map((s, i) =>
              s.arrow ? (
                <span key={i} className={s.color + " text-lg"}>{s.label}</span>
              ) : (
                <span key={i} className={`px-3 py-1.5 rounded-full border text-xs ${s.color}`}>
                  {s.label}
                </span>
              )
            )}
            <div className="w-full mt-4 flex flex-wrap gap-3 items-center">
              {[
                { label: "REORGED", color: "text-red-400 bg-red-500/10 border-red-500/20" },
                { label: "FAILED", color: "text-zinc-400 bg-zinc-500/10 border-zinc-500/20" },
              ].map((s) => (
                <span key={s.label} className={`px-3 py-1.5 rounded-full border text-xs ${s.color}`}>
                  {s.label}
                </span>
              ))}
              <span className="text-zinc-500 text-xs">— emitted as retraction; record preserved for audit</span>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

/* ─── Why Better ──────────────────────────────────────────────────────────── */

function WhyBetter() {
  const features = [
    "Indexes Bitcoin tunnel side",
    "Cross-chain join",
    "Real-time webhooks",
    "Reorg-safe by design",
    "Hemi-specific schema",
    "PoP proof tracking",
    "Full lifecycle status",
    "Available as a product",
  ];

  const tools = [
    {
      name: "The Graph",
      note: "(on Hemi)",
      values: [false, false, false, "partial", false, false, false, true],
    },
    {
      name: "Goldsky",
      note: "(not on Hemi)",
      values: [false, false, true, "EVM only", false, false, false, false],
    },
    {
      name: "Custom in-house",
      note: "",
      values: ["sometimes", "sometimes", "varies", false, "varies", false, false, false],
    },
    {
      name: "Strait",
      note: "",
      values: [true, true, true, true, true, true, true, true],
      highlight: true,
    },
  ];

  return (
    <section className="py-28 px-6" id="why-better">
      <div className="max-w-5xl mx-auto">
        <div className="text-center mb-16">
          <div className="text-xs font-mono text-orange-400 uppercase tracking-widest mb-4">Comparison</div>
          <h2 className="text-4xl md:text-5xl font-bold tracking-tight mb-6">
            What makes Strait different
          </h2>
          <p className="text-zinc-400 max-w-xl mx-auto text-lg leading-relaxed">
            The Graph is general-purpose EVM infrastructure that happens to support Hemi.
            Strait is Hemi-specific infrastructure that understands the chain&rsquo;s defining feature — tunnels — natively.
          </p>
        </div>

        <div className="overflow-x-auto">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="border-b border-white/[0.08]">
                <th className="text-left py-4 pr-6 text-zinc-400 font-normal w-52">Capability</th>
                {tools.map((t) => (
                  <th
                    key={t.name}
                    className={`text-center py-4 px-4 font-semibold ${t.highlight ? "text-orange-400" : "text-zinc-300"}`}
                  >
                    {t.name}
                    {t.note && <span className="block text-xs font-normal text-zinc-500">{t.note}</span>}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {features.map((f, fi) => (
                <tr key={f} className="border-b border-white/[0.04] hover:bg-white/[0.02] transition-colors">
                  <td className="py-3.5 pr-6 text-zinc-300">{f}</td>
                  {tools.map((t) => {
                    const v = t.values[fi];
                    return (
                      <td key={t.name} className="text-center py-3.5 px-4">
                        {v === true ? (
                          <span className="text-green-400 text-base">✓</span>
                        ) : v === false ? (
                          <span className="text-zinc-600 text-base">✗</span>
                        ) : (
                          <span className="text-xs text-zinc-500">{v}</span>
                        )}
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div className="mt-12 grid md:grid-cols-3 gap-6">
          {[
            {
              title: "The data doesn't exist elsewhere",
              body: "Strait is not a better indexer in a crowded market. It is the only indexer for a category of data that every serious Hemi participant needs — and that no other system produces.",
            },
            {
              title: "Depth over breadth",
              body: "The Graph covers hundreds of chains generically. Strait covers one chain with surgical depth. That means PoP proof tracking, OP_RETURN parsing, and dual-finality reconciliation built in.",
            },
            {
              title: "Built in Rust for correctness",
              body: "The ingestion, join, and webhook layers are written in Rust. The type system makes the state machine tractable to verify. Tokio handles I/O concurrency. No GC pauses, no missed reorgs.",
            },
          ].map((c) => (
            <div key={c.title} className="bg-white/[0.03] border border-white/[0.07] rounded-2xl p-6">
              <h3 className="font-semibold text-white mb-3">{c.title}</h3>
              <p className="text-zinc-400 text-sm leading-relaxed">{c.body}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

/* ─── Features ────────────────────────────────────────────────────────────── */

function Features() {
  const cards = [
    {
      icon: "⬡",
      title: "GraphQL API",
      description:
        "Full cursor-paginated query interface. Filter by asset, route, status, amount, sender, recipient, and time window. Aggregate stats (volume, count, median finality) per time period. GraphiQL playground in development.",
    },
    {
      icon: "⚡",
      title: "Real-Time Webhooks",
      description:
        "Sub-second delivery from chain event to your endpoint. HMAC-SHA256 signed payloads. Configurable filters on amount, address, asset, and status transition. Exponential-backoff retry with up to 5 attempts.",
    },
    {
      icon: "↺",
      title: "Reorg Safety",
      description:
        "Every ingester maintains a rolling block-hash window. Reorgs are detected by verifying parent hashes on every new block. Affected transfers are retracted — not deleted — with a reorg_at timestamp for full auditability.",
    },
    {
      icon: "⊕",
      title: "Bitcoin-Native",
      description:
        "Watches tunnel custody addresses directly via Bitcoin RPC. Parses OP_RETURN data encoding the Hemi destination address. Tracks every sat from Bitcoin deposit through Hemi mint in a single joined record.",
    },
    {
      icon: "◎",
      title: "PoP Proof Tracking",
      description:
        "Hemi's Proof-of-Publication miners anchor Hemi finality to Bitcoin. Strait indexes every PoP submission, links it to the Hemi block range it covers, and attaches it to the relevant TunnelTransfer records.",
    },
    {
      icon: "▦",
      title: "Replay Log",
      description:
        "Every raw event from all three chains is written to an append-only log before processing. If the join engine needs to restart or the schema migrates, events can be replayed from any block height.",
    },
  ];

  return (
    <section className="py-28 px-6 bg-white/[0.015]" id="features">
      <div className="max-w-5xl mx-auto">
        <div className="text-center mb-16">
          <div className="text-xs font-mono text-orange-400 uppercase tracking-widest mb-4">Features</div>
          <h2 className="text-4xl md:text-5xl font-bold tracking-tight">
            Everything you need. Nothing you don&rsquo;t.
          </h2>
        </div>
        <div className="grid md:grid-cols-2 lg:grid-cols-3 gap-5">
          {cards.map((c) => (
            <div
              key={c.title}
              className="bg-white/[0.03] border border-white/[0.07] rounded-2xl p-6 hover:border-orange-500/30 transition-colors group"
            >
              <div className="text-2xl text-orange-400 mb-4 group-hover:scale-110 transition-transform inline-block">
                {c.icon}
              </div>
              <h3 className="font-semibold text-white mb-2">{c.title}</h3>
              <p className="text-zinc-400 text-sm leading-relaxed">{c.description}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

/* ─── Ecosystem Impact ────────────────────────────────────────────────────── */

function EcosystemImpact() {
  const segments = [
    {
      who: "DeFi Protocols",
      icon: "◈",
      value:
        "Know the origin of every deposited asset. Risk-stratify lending positions by capital provenance. Detect large tunnel inflows before they hit your liquidity pool.",
    },
    {
      who: "Analytics Platforms",
      icon: "◎",
      value:
        "Power Dune-equivalent dashboards on Hemi. Track TVL composition, capital flows, and route utilization across all tunnel directions from a single API.",
    },
    {
      who: "Wallets & Aggregators",
      icon: "⬡",
      value:
        "Surface real-time tunnel status to end users. Show estimated time-to-finality for in-flight transfers. Alert on deposit confirmation without polling three chains.",
    },
    {
      who: "Compliance & AML",
      icon: "▦",
      value:
        "Institutional BTC flow tracking with full Bitcoin-to-Hemi chain of custody. OFAC screening of tunnel participants. Reorg-aware audit logs that never delete history.",
    },
    {
      who: "MEV & Market Makers",
      icon: "⚡",
      value:
        "Detect large tunnel events in real time and predict downstream liquidity movements. Cross-chain arbitrage detection between tunneled and native assets.",
    },
    {
      who: "Hemi Foundation",
      icon: "◑",
      value:
        "Ecosystem reporting, TVL composition, capital flow analysis, and partner integration tracking — all from a canonical data source the whole ecosystem trusts.",
    },
  ];

  return (
    <section className="py-28 px-6" id="ecosystem">
      <div className="max-w-5xl mx-auto">
        <div className="text-center mb-16">
          <div className="text-xs font-mono text-orange-400 uppercase tracking-widest mb-4">Ecosystem</div>
          <h2 className="text-4xl md:text-5xl font-bold tracking-tight mb-6">
            Who benefits
          </h2>
          <p className="text-zinc-400 max-w-xl mx-auto text-lg leading-relaxed">
            Tunnel data is high-leverage. A small number of well-defined customer segments
            need it, and all of them require the same underlying joined dataset.
          </p>
        </div>

        <div className="grid md:grid-cols-2 lg:grid-cols-3 gap-5">
          {segments.map((s) => (
            <div
              key={s.who}
              className="bg-white/[0.03] border border-white/[0.07] rounded-2xl p-6 hover:border-orange-500/20 transition-colors"
            >
              <div className="flex items-center gap-3 mb-3">
                <span className="text-orange-400">{s.icon}</span>
                <span className="font-semibold text-white">{s.who}</span>
              </div>
              <p className="text-zinc-400 text-sm leading-relaxed">{s.value}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

/* ─── How to Use ──────────────────────────────────────────────────────────── */

function BuiltOnStrait() {
  const apps = [
    {
      name: 'Strait APIs',
      status: 'Live',
      desc: 'The GraphQL and REST API that powers everything here — full Transfer schema, queries, contract addresses, and a self-host guide.',
      cta: 'Read the docs',
      href: '/docs',
    },
    {
      name: 'Strait Analytics',
      status: 'Live',
      desc: 'Volume, TVL, route utilization, average finality time, and flow charts — Dune-style dashboards built straight off the indexed tunnel data.',
      cta: 'Open Analytics',
      href: 'https://strait-analytics.vercel.app/',
      external: true,
    },
    {
      name: 'Strait Webhooks',
      status: 'In progress',
      desc: 'HMAC-signed push notifications for transfer lifecycle events — register a URL, get notified the moment a matching transfer finalizes or fails.',
      cta: 'Register a webhook',
      href: '/webhooks',
    },
    {
      name: 'Strait BTC-Collateralized Cross-Chain Lending',
      status: 'Idea',
      desc: 'Use Bitcoin-anchored tunnel transfers as a settlement and collateral signal — lend against BTC bridged into Hemi, settled across chains.',
      cta: 'Explore the idea',
      href: '/docs',
    },
  ];
  const badge = (s: string) => {
    const cls =
      s === 'Live'
        ? 'border-emerald-500/30 bg-emerald-500/10 text-emerald-300'
        : s === 'In progress'
          ? 'border-orange-500/30 bg-orange-500/10 text-orange-300'
          : 'border-white/10 bg-white/[0.04] text-zinc-400';
    return (
      <span className={`text-[11px] font-mono rounded-full border px-2 py-0.5 ${cls}`}>{s}</span>
    );
  };
  return (
    <section className="py-28 px-6 bg-white/[0.015]" id="how-to-use">
      <div className="max-w-6xl mx-auto">
        <div className="text-center mb-16">
          <div className="text-xs font-mono text-orange-400 uppercase tracking-widest mb-4">Built on Strait</div>
          <h2 className="text-4xl md:text-5xl font-bold tracking-tight mb-6">One API, many apps</h2>
          <p className="text-zinc-400 max-w-2xl mx-auto">
            Strait is infrastructure. Every app below reads the same indexed tunnel data — start with the developer docs.
          </p>
        </div>
        <div className="grid md:grid-cols-2 gap-5">
          {apps.map((a) => (
            <a
              key={a.name}
              href={a.href}
              {...(a.external ? { target: "_blank", rel: "noopener noreferrer" } : {})}
              className="group rounded-2xl border border-white/[0.07] bg-white/[0.03] p-6 flex flex-col hover:border-orange-500/40 transition-colors"
            >
              <div className="mb-3">{badge(a.status)}</div>
              <div className="font-bold text-white text-lg mb-2">{a.name}</div>
              <p className="text-sm text-zinc-400 leading-relaxed flex-1">{a.desc}</p>
              <span className="mt-5 text-sm text-orange-400 group-hover:text-orange-300">{a.cta} →</span>
            </a>
          ))}
        </div>
      </div>
    </section>
  );
}

/* ─── Roadmap ─────────────────────────────────────────────────────────────── */

function Roadmap() {
  const phases = [
    {
      phase: "Phase 1",
      period: "Months 1–2",
      title: "Foundation",
      status: "In progress",
      statusColor: "text-yellow-400 bg-yellow-500/10 border-yellow-500/20",
      dot: "border-orange-400 bg-orange-400/30",
      items: [
        "Bitcoin + Hemi ingesters operational on testnet",
        "Join engine producing TunnelTransfer records",
        "Architecture finalized, infrastructure provisioned",
        "Grant proposal submitted to Hemispheres Foundation",
      ],
    },
    {
      phase: "Phase 2",
      period: "Months 3–5",
      title: "Build",
      status: "Upcoming",
      statusColor: "text-blue-400 bg-blue-500/10 border-blue-500/20",
      dot: "border-zinc-600 bg-zinc-900",
      items: [
        "All three ingesters in production",
        "GraphQL API in private beta with 2–3 pilot users",
        "Reorg handling validated against historical Bitcoin reorgs",
        "Webhook delivery live",
      ],
    },
    {
      phase: "Phase 3",
      period: "Month 6",
      title: "Public Launch",
      status: "Planned",
      statusColor: "text-zinc-400 bg-zinc-500/10 border-zinc-500/20",
      dot: "border-zinc-600 bg-zinc-900",
      items: [
        "Public launch at Hemi ecosystem event",
        "Reference dashboard published",
        "Self-serve onboarding open",
        "Free tier available",
      ],
    },
    {
      phase: "Phase 4",
      period: "Months 7–12",
      title: "Platform expansion",
      status: "Roadmap",
      statusColor: "text-zinc-500 bg-zinc-600/10 border-zinc-600/20",
      dot: "border-zinc-700 bg-zinc-900",
      items: [
        "Mirror pipelines (Postgres, Kafka, ClickHouse)",
        "hVM-aware indexing — Bitcoin reads inside Hemi contracts",
        "PoP anchoring + tunneled asset flow datasets",
        "First 50 paying customers",
      ],
    },
    {
      phase: "Phase 5",
      period: "Year 2–3",
      title: "Multi-chain",
      status: "Vision",
      statusColor: "text-purple-400 bg-purple-500/10 border-purple-500/20",
      dot: "border-zinc-700 bg-zinc-900",
      items: [
        "Botanix, Citrea, BOB integration",
        "Cross-chain Bitcoin index across all Bitcoin-aware chains",
        "General-purpose hVM subgraph DSL",
        "Category-defining data platform for Bitcoin-aware execution",
      ],
    },
  ];

  return (
    <section className="py-28 px-6" id="roadmap">
      <div className="max-w-5xl mx-auto">
        <div className="text-center mb-16">
          <div className="text-xs font-mono text-orange-400 uppercase tracking-widest mb-4">Roadmap</div>
          <h2 className="text-4xl md:text-5xl font-bold tracking-tight mb-6">
            Ship the wedge. Earn the platform.
          </h2>
          <p className="text-zinc-400 max-w-xl mx-auto text-lg leading-relaxed">
            Tunnel indexing is the beachhead. Every component built for the wedge has a role
            in the platform — the architecture is a strict subset, not a throwaway.
          </p>
        </div>

        <div className="relative">
          <div className="absolute left-6 top-0 bottom-0 w-px bg-white/[0.06] hidden md:block" />

          <div className="space-y-6">
            {phases.map((p) => (
              <div key={p.phase} className="md:pl-16 relative">
                <div className={`absolute left-4 top-6 w-4 h-4 rounded-full border-2 hidden md:block ${p.dot}`} />
                <div className="bg-white/[0.03] border border-white/[0.07] rounded-2xl p-6">
                  <div className="flex flex-wrap items-start justify-between gap-4 mb-4">
                    <div>
                      <div className="text-xs font-mono text-zinc-500 mb-1">{p.phase} · {p.period}</div>
                      <h3 className="text-xl font-bold text-white">{p.title}</h3>
                    </div>
                    <span className={`text-xs font-mono px-3 py-1 rounded-full border ${p.statusColor}`}>
                      {p.status}
                    </span>
                  </div>
                  <ul className="space-y-2">
                    {p.items.map((item) => (
                      <li key={item} className="flex items-start gap-2 text-sm text-zinc-400">
                        <span className="text-orange-400 mt-0.5 flex-shrink-0">›</span>
                        {item}
                      </li>
                    ))}
                  </ul>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

/* ─── CTA ─────────────────────────────────────────────────────────────────── */

function CTA() {
  return (
    <section className="py-28 px-6 bg-white/[0.015]">
      <div className="max-w-3xl mx-auto text-center">
        <div className="bg-gradient-to-b from-orange-500/20 to-transparent rounded-3xl p-16 border border-orange-500/20">
          <div className="text-5xl mb-6">⊕</div>
          <h2 className="text-4xl md:text-5xl font-bold tracking-tight mb-6">
            Every tunnel. One record.
          </h2>
          <p className="text-zinc-400 text-lg leading-relaxed mb-10 max-w-xl mx-auto">
            Stop stitching together three chain explorers. Strait gives you a complete,
            real-time, reorg-safe view of every asset moving through Hemi — queryable in seconds.
          </p>
          <div className="flex flex-col sm:flex-row gap-4 justify-center">
            <a
              href="/docs"
              className="px-8 py-3.5 bg-orange-500 hover:bg-orange-400 text-black font-semibold rounded-full transition-colors text-sm"
            >
              Get API access →
            </a>
            <a
              href="https://github.com/strait-data/strait"
              className="px-8 py-3.5 border border-white/10 hover:border-white/30 text-zinc-300 rounded-full transition-colors text-sm flex items-center justify-center gap-2"
              target="_blank"
              rel="noopener noreferrer"
            >
              <GithubIcon />
              View on GitHub
            </a>
          </div>
        </div>
      </div>
    </section>
  );
}

/* ─── Footer ──────────────────────────────────────────────────────────────── */

function Footer() {
  return (
    <footer className="border-t border-white/[0.06] py-12 px-6">
      <div className="max-w-5xl mx-auto flex flex-col md:flex-row items-center justify-between gap-6">
        <div className="flex items-center gap-2">
          <span className="text-orange-400 font-mono font-bold">⊕ Strait</span>
          <span className="text-zinc-600 text-sm">Real-time tunnel indexer for Hemi</span>
        </div>
        <div className="flex items-center gap-6 text-sm text-zinc-500">
          <a href="/dashboard" className="hover:text-white transition-colors">Explorer</a>
          <a href="/docs" className="hover:text-white transition-colors">Docs</a>
          <a href="/webhooks" className="hover:text-white transition-colors">Webhooks</a>
          <a
            href="https://github.com/strait-data/strait"
            className="hover:text-white transition-colors"
            target="_blank"
            rel="noopener noreferrer"
          >
            GitHub
          </a>
          <span>MIT License</span>
        </div>
      </div>
    </footer>
  );
}

/* ─── Shared components ───────────────────────────────────────────────────── */

function GithubIcon() {
  return (
    <svg width="16" height="16" fill="currentColor" viewBox="0 0 24 24">
      <path d="M12 0C5.374 0 0 5.373 0 12c0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23A11.509 11.509 0 0112 5.803c1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576C20.566 21.797 24 17.3 24 12c0-6.627-5.373-12-12-12z" />
    </svg>
  );
}
