"use client";

import { useRouter, useSearchParams } from "next/navigation";
import { useState } from "react";
import Dropdown from "../components/Dropdown";
import { routeLabel } from "@/lib/strait";

const STATUSES = ["INITIATED", "ANCHORED", "FINALIZED", "FAILED", "REORGED"];
const ROUTES = ["BTC_TO_HEMI", "HEMI_TO_BTC", "ETH_TO_HEMI", "HEMI_TO_ETH"];

const statusOptions = [
  { value: "", label: "All statuses" },
  ...STATUSES.map((s) => ({ value: s, label: s })),
];
const routeOptions = [
  { value: "", label: "All routes" },
  ...ROUTES.map((r) => ({ value: r, label: routeLabel(r) })),
];

const field =
  "rounded-lg border border-white/10 bg-white/[0.03] px-3 py-2 text-sm text-white focus:border-orange-400/50 focus:outline-none";

/** Search (address / tx hash / id) + status & route filters, driven through the
 *  URL search params so results are server-rendered and shareable. */
export default function SearchFilter({ network }: { network: string }) {
  const router = useRouter();
  const sp = useSearchParams();
  const [q, setQ] = useState(sp.get("q") ?? "");
  const status = sp.get("status") ?? "";
  const route = sp.get("route") ?? "";

  function apply(next: { q?: string; status?: string; route?: string }) {
    const params = new URLSearchParams();
    const nq = next.q ?? q;
    const ns = next.status ?? status;
    const nr = next.route ?? route;
    if (nq) params.set("q", nq);
    if (ns) params.set("status", ns);
    if (nr) params.set("route", nr);
    const qs = params.toString();
    router.push(`/dashboard/${network}${qs ? `?${qs}` : ""}`);
  }

  const active = q || status || route;

  return (
    <div className="flex flex-wrap items-center gap-2">
      <form
        onSubmit={(e) => {
          e.preventDefault();
          apply({});
        }}
        className="min-w-[240px] flex-1"
      >
        <input
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="Search by address, tx hash, or id…"
          className={`w-full ${field} placeholder-zinc-500`}
          spellCheck={false}
        />
      </form>
      <Dropdown
        value={status}
        options={statusOptions}
        onChange={(v) => apply({ status: v })}
        className="w-40"
      />
      <Dropdown
        value={route}
        options={routeOptions}
        onChange={(v) => apply({ route: v })}
        className="w-40"
      />
      {active ? (
        <button
          onClick={() => {
            setQ("");
            router.push(`/dashboard/${network}`);
          }}
          className="rounded-lg border border-white/10 px-3 py-2 text-sm text-white hover:text-zinc-300"
        >
          Clear
        </button>
      ) : null}
    </div>
  );
}
