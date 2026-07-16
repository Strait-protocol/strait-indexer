import AutoRefresh from "./AutoRefresh";
import NetworkSwitcher from "./NetworkSwitcher";
import NavBar from "../components/NavBar";

export default function DashboardLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <div className="min-h-screen bg-[#0a0a0a] text-white font-sans">
      <AutoRefresh seconds={10} />
      <NavBar
        extras={
          <>
            <NetworkSwitcher />
            <span
              className="inline-flex items-center gap-2 text-zinc-400"
              title="Auto-refreshes every 10s"
            >
              <span className="h-2 w-2 rounded-full bg-emerald-400 animate-pulse" />
              live
            </span>
          </>
        }
      />
      <main className="max-w-6xl mx-auto px-6 py-10">{children}</main>
    </div>
  );
}
