"use client";

import { useEffect, useState } from "react";

const NAV = [
  { id: "overview", label: "Overview" },
  { id: "data", label: "The data" },
  { id: "lifecycle", label: "Lifecycle" },
  { id: "api", label: "Using the API" },
  { id: "contracts", label: "Contracts" },
];

const TOC = [
  { id: "overview", label: "Overview", level: 2 },
  { id: "data", label: "The data", level: 2 },
  { id: "lifecycle", label: "Lifecycle", level: 2 },
  { id: "lifecycle-eth-to-hemi", label: "ETH → Hemi (~2 min)", level: 3 },
  { id: "lifecycle-btc-to-hemi", label: "BTC → Hemi (~1–2 hr)", level: 3 },
  { id: "lifecycle-hemi-to-eth", label: "Hemi → ETH (~1 day)", level: 3 },
  { id: "lifecycle-hemi-to-btc", label: "Hemi → BTC (~2–14 hr)", level: 3 },
  { id: "api", label: "Using the API", level: 2 },
  { id: "api-endpoints", label: "Endpoints", level: 3 },
  { id: "api-graphql", label: "GraphQL queries", level: 3 },
  { id: "api-examples", label: "Examples", level: 3 },
  { id: "contracts", label: "Contracts", level: 2 },
];

const IDS = TOC.map((h) => h.id);

export default function DocsSidebars() {
  const [activeId, setActiveId] = useState(IDS[0]);

  useEffect(() => {
    function onScroll() {
      const scrollY = window.scrollY + 96;
      let current = IDS[0];
      for (const id of IDS) {
        const el = document.getElementById(id);
        if (el && el.offsetTop <= scrollY) current = id;
      }
      setActiveId(current);
    }
    window.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  // Walk back from activeId to find the enclosing h2 section
  const tocIdx = TOC.findIndex((h) => h.id === activeId);
  let activeSectionId = activeId;
  for (let i = tocIdx; i >= 0; i--) {
    if (TOC[i].level === 2) { activeSectionId = TOC[i].id; break; }
  }

  return (
    <>
      {/* Left sidebar — section navigation */}
      <aside className="fixed top-16 left-0 bottom-0 w-56 overflow-y-auto border-r border-white/[0.06] bg-[#0a0a0a] hidden xl:block z-40">
        <nav className="p-6">
          <p className="text-[11px] font-mono text-zinc-600 uppercase tracking-widest mb-5">Docs</p>
          <ul className="space-y-0.5">
            {NAV.map(({ id, label }) => (
              <li key={id}>
                <a
                  href={`#${id}`}
                  className={`flex items-center px-3 py-2 rounded-lg text-sm transition-colors ${
                    activeSectionId === id
                      ? "text-orange-400 bg-orange-500/10 font-medium"
                      : "text-zinc-400 hover:text-white hover:bg-white/[0.04]"
                  }`}
                >
                  {label}
                </a>
              </li>
            ))}
          </ul>
        </nav>
      </aside>

      {/* Right sidebar — on this page */}
      <aside className="fixed top-16 right-0 bottom-0 w-52 overflow-y-auto border-l border-white/[0.06] bg-[#0a0a0a] hidden xl:block z-40">
        <nav className="p-6">
          <p className="text-[11px] font-mono text-zinc-600 uppercase tracking-widest mb-5">On this page</p>
          <ul className="space-y-0.5">
            {TOC.map(({ id, label, level }) => (
              <li key={id}>
                <a
                  href={`#${id}`}
                  className={`flex items-center py-1.5 transition-colors ${
                    level === 3 ? "pl-3 text-xs" : "text-sm"
                  } ${
                    activeId === id
                      ? "text-orange-400"
                      : level === 3
                      ? "text-zinc-600 hover:text-zinc-300"
                      : "text-zinc-500 hover:text-zinc-300"
                  }`}
                >
                  {level === 3 && <span className="mr-1.5 text-zinc-700">›</span>}
                  {label}
                </a>
              </li>
            ))}
          </ul>
        </nav>
      </aside>
    </>
  );
}
