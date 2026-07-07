"use client";

import { useRouter } from "next/navigation";
import {
  type Transfer,
  type Network,
  formatAmount,
  routeLabel,
  statusStyle,
  transferKind,
  shortHash,
  timeAgo,
} from "@/lib/strait";

/** A clickable transfer row — the whole row navigates to the detail page. */
export default function TransferRow({ t, network }: { t: Transfer; network: Network }) {
  const router = useRouter();
  const s = statusStyle(t.status);
  const kind = transferKind(t.direction);
  return (
    <tr
      onClick={() => router.push(`/dashboard/${network}/transfers/${t.id}`)}
      className="cursor-pointer border-b border-white/[0.04] last:border-0 hover:bg-white/[0.04] transition-colors"
    >
      <td className="px-4 py-3">
        <div className="font-mono text-zinc-100">{routeLabel(t.route)}</div>
        <div className="mt-1 flex items-center gap-2 text-[11px]">
          <span className={`rounded border px-1.5 py-0.5 ${kind.cls}`}>{kind.label}</span>
          <span className="font-mono text-zinc-600">{t.id.slice(0, 8)}…</span>
        </div>
      </td>
      <td className="px-4 py-3 tabular-nums text-zinc-200">{formatAmount(t.asset, t.amount)}</td>
      <td className="px-4 py-3">
        <span className={`inline-flex items-center gap-1.5 ${s.text}`}>
          <span className={`h-2 w-2 rounded-full ${s.dot}`} />
          {s.label}
        </span>
      </td>
      <td className="px-4 py-3 font-mono text-zinc-400">{shortHash(t.sender)}</td>
      <td className="px-4 py-3 font-mono text-zinc-400">{shortHash(t.recipient)}</td>
      <td className="px-4 py-3 text-right">
        <span className="text-zinc-500" suppressHydrationWarning>{timeAgo(t.initiatedAt)}</span>
        <span className="ml-2 text-zinc-600 group-hover:text-orange-400">›</span>
      </td>
    </tr>
  );
}
