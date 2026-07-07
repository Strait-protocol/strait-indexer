import Link from "next/link";

export default function TransferNotFound() {
  return (
    <div className="rounded-xl border border-dashed border-white/10 px-6 py-16 text-center">
      <h1 className="text-lg font-semibold text-zinc-200">Transfer not found</h1>
      <p className="mt-2 text-sm text-zinc-500">
        This transfer ID doesn&apos;t exist in the index yet. It may still be in flight or
        the node may not have indexed it.
      </p>
      <Link href="/dashboard/mainnet" className="mt-4 inline-block text-sm text-orange-400 hover:text-orange-300">
        ← Back to explorer
      </Link>
    </div>
  );
}
