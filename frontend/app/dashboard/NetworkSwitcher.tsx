"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

const NETWORKS = ["mainnet", "testnet"] as const;

/** Mainnet / Testnet toggle. Derives the current network from the path so it
 *  highlights correctly on both the overview and transfer-detail routes. */
export default function NetworkSwitcher() {
  const path = usePathname();
  const current = path.startsWith("/dashboard/testnet") ? "testnet" : "mainnet";
  return (
    <div className="flex items-center rounded-lg border border-white/10 p-0.5 text-xs">
      {NETWORKS.map((n) => (
        <Link
          key={n}
          href={`/dashboard/${n}`}
          className={`rounded-md px-2.5 py-1 capitalize transition-colors ${
            current === n ? "bg-white/10 text-white" : "text-zinc-400 hover:text-white"
          }`}
        >
          {n}
        </Link>
      ))}
    </div>
  );
}
