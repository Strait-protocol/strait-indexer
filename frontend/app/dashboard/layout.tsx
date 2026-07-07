import Link from "next/link";
import AutoRefresh from "./AutoRefresh";
import NetworkSwitcher from "./NetworkSwitcher";
import MobileMenu from "./MobileMenu";

export default function DashboardLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <div className="min-h-screen bg-[#0a0a0a] text-white font-sans">
      <AutoRefresh seconds={10} />
      <header className="sticky top-0 z-50 border-b border-white/[0.06] bg-[#0a0a0a]/80 backdrop-blur-md">
        <div className="max-w-6xl mx-auto px-6 h-16 flex items-center justify-between">
          <div className="flex items-center gap-6">
            <Link href="/" className="text-orange-400 font-mono text-xl font-bold tracking-tight">
              ⊕ Strait
            </Link>
            <span className="text-zinc-600">/</span>
            <Link href="/dashboard" className="text-sm text-zinc-300 hover:text-white transition-colors">
              Explorer
            </Link>
          </div>

          {/* Desktop nav — hidden on mobile, where MobileMenu takes over below */}
          <div className="hidden md:flex items-center gap-3 text-sm">
            <NetworkSwitcher />
            <span className="inline-flex items-center gap-2 text-zinc-400" title="Auto-refreshes every 10s">
              <span className="h-2 w-2 rounded-full bg-emerald-400 animate-pulse" />
              live
            </span>
            <Link
              href="/"
              className="text-zinc-400 hover:text-white transition-colors"
            >
              ← Home
            </Link>
          </div>

          <MobileMenu>
            <NetworkSwitcher />
            <span className="inline-flex items-center gap-2 text-zinc-400" title="Auto-refreshes every 10s">
              <span className="h-2 w-2 rounded-full bg-emerald-400 animate-pulse" />
              live
            </span>
            <Link href="/" className="text-zinc-400 hover:text-white transition-colors">
              ← Home
            </Link>
          </MobileMenu>
        </div>
      </header>
      <main className="max-w-6xl mx-auto px-6 py-10">{children}</main>
    </div>
  );
}
