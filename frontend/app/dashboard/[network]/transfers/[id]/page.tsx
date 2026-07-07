import Link from "next/link";
import { notFound } from "next/navigation";
import CopyButton from "@/app/dashboard/CopyButton";
import {
  getTransfer,
  formatAmount,
  routeLabel,
  statusStyle,
  transferKind,
  shortHash,
  timeAgo,
  txExplorerUrl,
  sourceObserved,
  formatFee,
  isNetwork,
  type Network,
  type Transfer,
} from "@/lib/strait";

export const dynamic = "force-dynamic";

type Step = {
  title: string;
  chain: string | null;
  hash: string | null;
  block: number | null;
  done: boolean;
  detail?: string;
  isFailed?: boolean;
};

export default async function TransferDetailPage({
  params,
}: {
  params: Promise<{ network: string; id: string }>;
}) {
  const { network: seg, id } = await params;
  if (!isNetwork(seg)) notFound();
  const network: Network = seg;

  const t = await getTransfer(id, network);

  if (!t) notFound();

  const s = statusStyle(t.status);
  const kind = transferKind(t.direction);
  const steps = buildTimeline(t);

  return (
    <div className="space-y-8">
      <Link href={`/dashboard/${network}`} className="text-sm text-zinc-400 hover:text-white transition-colors">
        ← All transfers
      </Link>

      {/* Header */}
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <div className="flex items-center gap-3">
            <span className="font-mono text-2xl font-semibold tracking-tight">
              {routeLabel(t.route)}
            </span>
            <span className={`rounded border px-2 py-0.5 text-xs ${kind.cls}`}>{kind.label}</span>
          </div>
          <div className="mt-1 text-3xl font-semibold tabular-nums text-white">
            {formatAmount(t.asset, t.amount)}
          </div>
          <div className="mt-2 flex items-center gap-1.5 font-mono text-xs text-zinc-500">
            {t.id}
            <CopyButton value={t.id} />
          </div>
        </div>
        <span className={`inline-flex items-center gap-2 rounded-full border border-white/10 px-3 py-1.5 text-sm ${s.text}`}>
          <span className={`h-2 w-2 rounded-full ${s.dot}`} />
          {s.label}
        </span>
      </div>

      <div className="grid gap-8 md:grid-cols-5">
        {/* Finality timeline */}
        <section className="md:col-span-3">
          <h2 className="mb-5 text-sm font-medium text-zinc-300">Finality lifecycle</h2>
          <ol className="relative">
            {steps.map((step, i) => (
              <TimelineStep key={i} step={step} last={i === steps.length - 1} network={network} />
            ))}
          </ol>
          {t.popAnchored && (
            <div className="mt-6 rounded-xl border border-orange-500/20 bg-orange-500/[0.04] px-4 py-3 text-sm">
              <span className="text-orange-300">⛓ Anchored to Bitcoin.</span>{" "}
              <span className="text-zinc-400">
                Covered by Hemi keystone{" "}
                <span className="font-mono text-zinc-200">#{t.popKeystoneBlock?.toLocaleString()}</span>
                {t.popScore != null && <> · PoP score {t.popScore.toLocaleString()}</>}. This transfer carries
                Bitcoin-grade finality, not just Hemi consensus.
              </span>
            </div>
          )}
        </section>

        {/* Metadata */}
        <section className="md:col-span-2">
          <h2 className="mb-5 text-sm font-medium text-zinc-300">Details</h2>
          <dl className="space-y-3 rounded-xl border border-white/[0.06] bg-white/[0.02] px-5 py-4 text-sm">
            <Field label="Asset" value={t.asset} />
            <Field label="Direction" value={`${kind.label} (${kind.hint})`} />
            <Field label="Amount" value={formatAmount(t.asset, t.amount)} mono />
            <Field label="Sender" value={shortHash(t.sender)} mono copyValue={t.sender} />
            <Field label="Recipient" value={shortHash(t.recipient)} mono copyValue={t.recipient} />
            <div className="h-px bg-white/[0.06]" />
            <TxField
              label="Source Tx"
              chain={t.sourceChain}
              hash={sourceObserved(t) ? t.sourceTxHash : null}
              block={sourceObserved(t) ? t.sourceBlock : null}
              fee={t.sourceFee}
              network={network}
            />
            <TxField
              label="Destination Tx"
              chain={t.destChain}
              hash={t.destTxHash}
              block={t.destBlock}
              fee={t.destFee}
              network={network}
            />
            <div className="h-px bg-white/[0.06]" />
            <Field label="Initiated" value={timeAgo(t.initiatedAt)} />
            <Field label="Finalized" value={t.finalizedAt ? timeAgo(t.finalizedAt) : "—"} />
          </dl>
        </section>
      </div>
    </div>
  );
}

function buildTimeline(t: Transfer): Step[] {
  const inbound = t.direction === "IN";
  const failed = t.status === "FAILED" || t.status === "REORGED";
  const failedDetail = t.status === "REORGED"
    ? "rolled back by a chain reorg"
    : t.route === "HEMI_TO_BTC"
      ? "operator did not pay — hBTC refunded to sender"
      : undefined;
  const finalStep: Step = {
    title: failed ? "Failed" : "Finalized",
    chain: null,
    hash: null,
    block: null,
    done: t.status === "FINALIZED" || failed,
    detail: failed ? failedDetail : t.finalizedAt ? timeAgo(t.finalizedAt) : undefined,
    isFailed: failed,
  };

  if (inbound) {
    const srcObs = sourceObserved(t);
    const srcLabel = t.sourceChain === "BITCOIN"
      ? "Bitcoin deposit confirmed"
      : srcObs
        ? "Locked on source chain"
        : "Locked on Ethereum (L1)";
    return [
      {
        title: srcLabel,
        chain: srcObs ? t.sourceChain : null,
        hash: srcObs ? t.sourceTxHash : null,
        block: srcObs ? t.sourceBlock || null : null,
        done: srcObs,
        detail: srcObs ? undefined : "L1 deposit not yet matched",
      },
      { title: "Minted on Hemi", chain: t.destChain, hash: t.destTxHash, block: t.destBlock, done: t.destTxHash != null },
      finalStep,
    ];
  }

  // Outbound (withdrawal from Hemi)
  const burned: Step = { title: "Burned on Hemi", chain: t.sourceChain, hash: t.sourceTxHash, block: t.sourceBlock || null, done: true };

  // HEMI_TO_ETH: 3-phase OP Stack withdrawal (burn → prove → release)
  if (t.route === "HEMI_TO_ETH") {
    const proven = t.status !== "INITIATED";
    return [
      burned,
      {
        title: "Proven on Ethereum L1",
        chain: "ETHEREUM",
        hash: null,
        block: null,
        done: proven,
        detail: proven ? "1-day challenge window started" : "Submit proof on the Hemi bridge to begin",
      },
      { title: "Released on Ethereum", chain: t.destChain, hash: t.destTxHash, block: t.destBlock, done: t.destTxHash != null },
      finalStep,
    ];
  }

  return [
    burned,
    {
      title: "Paid out on Bitcoin",
      chain: t.destChain,
      hash: t.destTxHash,
      block: t.destBlock,
      done: t.destTxHash != null,
    },
    finalStep,
  ];
}

function TimelineStep({ step, last, network }: { step: Step; last: boolean; network: Network }) {
  const url = txExplorerUrl(step.chain, step.hash, network);
  const dotColor = step.isFailed
    ? "border-red-500 bg-red-500"
    : step.done
      ? "border-orange-400 bg-orange-400"
      : "border-white/20 bg-[#0a0a0a]";
  const lineColor = step.isFailed ? "bg-red-500/30" : step.done ? "bg-orange-400/40" : "bg-white/10";
  return (
    <li className="relative flex gap-4 pb-7 last:pb-0">
      {!last && (
        <span className={`absolute left-1.5 top-4 h-full w-px ${lineColor}`} aria-hidden />
      )}
      <span className={`relative z-10 mt-1 h-3.5 w-3.5 shrink-0 rounded-full border-2 ${dotColor}`} />
      <div className="-mt-0.5">
        <div className={step.done ? "text-zinc-100" : "text-zinc-500"}>{step.title}</div>
        <div className="mt-0.5 flex flex-wrap items-center gap-x-3 text-xs text-zinc-500">
          {step.block ? <span>block {step.block.toLocaleString()}</span> : null}
          {step.detail ? <span>{step.detail}</span> : null}
          {url ? (
            <a href={url} target="_blank" rel="noopener noreferrer" className="font-mono text-zinc-400 hover:text-orange-400">
              {shortHash(step.hash)} ↗
            </a>
          ) : step.hash ? (
            <span className="font-mono">{shortHash(step.hash)}</span>
          ) : null}
        </div>
      </div>
    </li>
  );
}

function Field({ label, value, mono, copyValue }: { label: string; value: string; mono?: boolean; copyValue?: string }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <dt className="text-zinc-500">{label}</dt>
      <dd className={`flex items-center gap-1.5 text-zinc-200 ${mono ? "font-mono" : ""}`}>
        {value}
        {copyValue && <CopyButton value={copyValue} />}
      </dd>
    </div>
  );
}

function TxField({ label, chain, hash, block, fee, network }: { label: string; chain: string | null; hash: string | null; block: number | null; fee?: string | null; network: Network }) {
  const url = txExplorerUrl(chain, hash, network);
  const feeText = formatFee(chain, fee ?? null);
  return (
    <div className="flex items-center justify-between gap-4">
      <dt className="text-zinc-500">{label}</dt>
      <dd className="text-right">
        {hash ? (
          <>
            <div className="flex items-center justify-end gap-1.5">
              {url ? (
                <a href={url} target="_blank" rel="noopener noreferrer" className="font-mono text-zinc-200 hover:text-orange-400">
                  {shortHash(hash)} ↗
                </a>
              ) : (
                <span className="font-mono text-zinc-200">{shortHash(hash)}</span>
              )}
              <CopyButton value={hash} />
            </div>
            <div className="text-xs text-zinc-500">
              {chain}{block ? ` · block ${block.toLocaleString()}` : ""}
            </div>
            {feeText && <div className="text-xs text-zinc-500">fee {feeText}</div>}
          </>
        ) : (
          <span className="text-zinc-600">pending</span>
        )}
      </dd>
    </div>
  );
}

