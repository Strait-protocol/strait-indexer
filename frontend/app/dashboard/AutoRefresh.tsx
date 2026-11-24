"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";

/** Periodically re-fetches the server-rendered page so the dashboard stays live
 *  without a manual refresh. Only while the tab is visible: hidden tabs don't
 *  poll, and refresh once when they become visible again if they've gone stale. */
export default function AutoRefresh({ seconds = 30 }: { seconds?: number }) {
  const router = useRouter();
  useEffect(() => {
    const intervalMs = seconds * 1000;
    let last = Date.now();
    let id: ReturnType<typeof setInterval> | undefined;

    const refresh = () => {
      last = Date.now();
      router.refresh();
    };
    const start = () => {
      if (id === undefined) id = setInterval(refresh, intervalMs);
    };
    const stop = () => {
      clearInterval(id);
      id = undefined;
    };
    const onVisibility = () => {
      if (document.hidden) {
        stop();
      } else {
        if (Date.now() - last >= intervalMs) refresh();
        start();
      }
    };

    if (!document.hidden) start();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      stop();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [router, seconds]);
  return null;
}
