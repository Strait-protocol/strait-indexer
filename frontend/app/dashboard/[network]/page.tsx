import { notFound } from "next/navigation";
import {
  getOverview,
  searchTransfers,
  statusStyle,
  STATUSES,
  isNetwork,
  type Network,
  type Transfer,
} from "@/lib/strait";
import TransferRow from "../TransferRow";
import SearchFilter from "../SearchFilter";

export const dynamic = "force-dynamic";

export default async function DashboardPage({
  params,
  searchParams,
}: {
  params: Promise<{ network: string }>;
  searchParams: Promise<{ q?: string; status?: string; route?: string }>;
}) {
  const { network: seg } = await params;
  if (!isNetwork(seg)) notFound();
  const network: Network = seg;

  const sp = await searchParams;
  const filtering = Boolean(sp.q || sp.status || sp.route);

  const data = await getOverview(network);

  if (!data) return <OfflineState network={network} />;

  const { stats } = data;
  const transfers = filtering
    ? await searchTransfers(network, { query: sp.q, status: sp.status, route: sp.route })
    : data.transfers;
  const counts = countByStatus(transfers);
  const anchored = transfers.filter((t) => t.popAnchored).length;

  return (
    <div className="space-y-10">
      {network === "testnet" && (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-4 py-2.5 text-sm text-amber-300">
          🧪 <span className="font-medium">Testnet</span> — Hemi Sepolia + Ethereum Sepolia.
          Test data only; not real value.
        </div>
      )}
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">
          Tunnel Explorer
          <span className="ml-2 align-middle text-xs font-normal text-zinc-500 capitalize">
            {network}
          </span>
        </h1>
        <p className="mt-1 text-sm text-zinc-400">
          Every asset moving through Hemi&apos;s Bitcoin and Ethereum tunnels — tracked
          from initiation to Bitcoin-anchored finality.
        </p>
      </div>

      {/* Stat cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
        <StatCard label="Total transfers" value={stats.totalTransfers.toLocaleString()} />
        <StatCard label="PoP-anchored (recent)" value={anchored.toString()} accent />
        <StatCard label="Finalized (recent)" value={(counts.FINALIZED ?? 0).toString()} />
        <StatCard label="In flight (recent)" value={((counts.INITIATED ?? 0) + (counts.ANCHORED ?? 0)).toString()} />
      </div>

      {/* Status funnel */}
      <StatusFunnel counts={counts} total={transfers.length} />

      {/* Search + transfers */}
      <section className="space-y-4">
        <SearchFilter network={network} />
        <div className="flex items-baseline justify-between">
          <h2 className="text-sm font-medium text-zinc-300">
            {filtering ? "Search results" : "Recent transfers"}
          </h2>
          <span className="text-xs text-zinc-500">{transfers.length} shown</span>
        </div>

        {transfers.length === 0 ? (
          <EmptyTransfers filtering={filtering} />
        ) : (
          <div className="overflow-hidden rounded-xl border border-white/[0.06]">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left text-xs uppercase tracking-wide text-zinc-500 border-b border-white/[0.06]">
                  <th className="font-medium px-4 py-3">Route</th>
                  <th className="font-medium px-4 py-3">Amount</th>
                  <th className="font-medium px-4 py-3">Status</th>
                  <th className="font-medium px-4 py-3">Bitcoin-final</th>
                  <th className="font-medium px-4 py-3">Recipient</th>
                  <th className="font-medium px-4 py-3 text-right">Age →</th>
                </tr>
              </thead>
              <tbody>
                {transfers.map((t) => (
                  <TransferRow key={t.id} t={t} network={network} />
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  );
}

function countByStatus(transfers: Transfer[]): Record<string, number> {
  const c: Record<string, number> = {};
  for (const t of transfers) c[t.status] = (c[t.status] ?? 0) + 1;
  return c;
}

function StatCard({ label, value, accent }: { label: string; value: string; accent?: boolean }) {
  return (
    <div className="rounded-xl border border-white/[0.06] bg-white/[0.02] px-5 py-4">
      <div className="text-xs text-zinc-500">{label}</div>
      <div className={`mt-1 text-2xl font-semibold tabular-nums ${accent ? "text-orange-400" : "text-white"}`}>
        {value}
      </div>
    </div>
  );
}

function StatusFunnel({ counts, total }: { counts: Record<string, number>; total: number }) {
  const colors: Record<string, string> = {
    INITIATED: "bg-zinc-500",
    ANCHORED: "bg-orange-400",
    FINALIZED: "bg-emerald-400",
    FAILED: "bg-red-500",
    REORGED: "bg-red-700",
  };
  return (
    <div>
      <div className="flex h-2.5 w-full overflow-hidden rounded-full bg-white/[0.04]">
        {STATUSES.map((s) => {
          const n = counts[s] ?? 0;
          if (n === 0) return null;
          return (
            <div
              key={s}
              className={colors[s]}
              style={{ width: `${total ? (n / total) * 100 : 0}%` }}
              title={`${s}: ${n}`}
            />
          );
        })}
      </div>
      <div className="mt-2 flex flex-wrap gap-x-5 gap-y-1 text-xs text-zinc-400">
        {STATUSES.map((s) => (
          <span key={s} className="inline-flex items-center gap-1.5">
            <span className={`h-2 w-2 rounded-full ${colors[s]}`} />
            {statusStyle(s).label} <span className="tabular-nums text-zinc-500">{counts[s] ?? 0}</span>
          </span>
        ))}
      </div>
    </div>
  );
}

function EmptyTransfers({ filtering }: { filtering: boolean }) {
  return (
    <div className="rounded-xl border border-dashed border-white/10 px-6 py-14 text-center">
      <p className="text-zinc-300">{filtering ? "No matching transfers." : "No transfers indexed yet."}</p>
      <p className="mt-1 text-sm text-zinc-500">
        {filtering
          ? "Try a different address, transaction hash, or id — or clear the filters."
          : "Once the node ingests a tunnel transfer on Hemi, it will appear here in real time."}
      </p>
    </div>
  );
}

function OfflineState({ network }: { network: Network }) {
  const envVar = network === "testnet" ? "STRAIT_TESTNET_API_URL" : "STRAIT_API_URL";
  return (
    <div className="rounded-xl border border-dashed border-white/10 px-6 py-16 text-center">
      <h1 className="text-lg font-semibold text-zinc-200">
        {network === "testnet" ? "Testnet" : "Mainnet"} indexer offline
      </h1>
      <p className="mt-2 text-sm text-zinc-500">
        Couldn&apos;t reach the {network} Strait API. Start the {network} node and ensure{" "}
        <code className="font-mono text-zinc-300">{envVar}</code> points at its{" "}
        <code className="font-mono text-zinc-300">/graphql</code> endpoint.
      </p>
    </div>
  );
}
