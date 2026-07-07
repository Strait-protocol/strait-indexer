"use client";

import { useState } from "react";

/** Collapses its children behind a hamburger toggle below `md`. Desktop
 *  renders `children` inline instead (see layout.tsx) — this component only
 *  mounts the mobile variant, so the two never show at once. */
export default function MobileMenu({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = useState(false);

  return (
    <div className="md:hidden">
      <button
        onClick={() => setOpen((o) => !o)}
        aria-label={open ? "Close menu" : "Open menu"}
        aria-expanded={open}
        className="-mr-2 p-2 text-zinc-300 transition-colors hover:text-white"
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

      {open && (
        <div className="absolute left-0 right-0 top-16 flex flex-col gap-4 border-b border-white/[0.06] bg-[#0a0a0a]/95 px-6 py-5 text-sm backdrop-blur-md">
          {children}
        </div>
      )}
    </div>
  );
}
