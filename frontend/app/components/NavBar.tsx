"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useState } from "react";

const LINKS = [
  { href: "/dashboard", label: "Explorer" },
  { href: "/docs", label: "Docs" },
  { href: "/webhooks", label: "Webhooks" },
];

/** Shared site navbar: brand, the three product links, and a page-specific
 *  `extras` slot (network switcher, live badge, …). Desktop shows everything
 *  inline; below `md` the links + extras collapse behind a hamburger. */
export default function NavBar({ extras }: { extras?: React.ReactNode }) {
  const path = usePathname();
  const [open, setOpen] = useState(false);

  const links = LINKS.map(({ href, label }) => {
    const active = path === href || path.startsWith(href + "/");
    return (
      <Link
        key={href}
        href={href}
        onClick={() => setOpen(false)}
        className={`transition-colors ${
          active ? "text-orange-400 font-medium" : "text-zinc-400 hover:text-white"
        }`}
      >
        {label}
      </Link>
    );
  });

  return (
    <header className="sticky top-0 z-50 border-b border-white/[0.06] bg-[#0a0a0a]/80 backdrop-blur-md">
      <div className="max-w-6xl mx-auto px-6 h-16 flex items-center justify-between">
        <div className="flex items-center gap-8">
          <Link href="/" className="text-orange-400 font-mono text-xl font-bold tracking-tight">
            ⊕ Strait
          </Link>
          <nav className="hidden md:flex items-center gap-6 text-sm">{links}</nav>
        </div>

        <div className="hidden md:flex items-center gap-3 text-sm">{extras}</div>

        {/* Mobile hamburger */}
        <button
          onClick={() => setOpen((o) => !o)}
          aria-label={open ? "Close menu" : "Open menu"}
          aria-expanded={open}
          className="md:hidden -mr-2 p-2 text-zinc-300 transition-colors hover:text-white"
        >
          {open ? (
            <svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M5 5l10 10M15 5L5 15" strokeLinecap="round" />
            </svg>
          ) : (
            <svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M3 5h14M3 10h14M3 15h14" strokeLinecap="round" />
            </svg>
          )}
        </button>
      </div>

      {open && (
        <div className="md:hidden absolute left-0 right-0 top-16 flex flex-col gap-4 border-b border-white/[0.06] bg-[#0a0a0a]/95 px-6 py-5 text-sm backdrop-blur-md">
          {links}
          {extras && <div className="flex flex-wrap items-center gap-3 pt-1">{extras}</div>}
        </div>
      )}
    </header>
  );
}
