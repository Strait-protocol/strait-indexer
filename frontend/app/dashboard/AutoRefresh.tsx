"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";

/** Periodically re-fetches the server-rendered page so the dashboard stays live
 *  without a manual refresh. */
export default function AutoRefresh({ seconds = 10 }: { seconds?: number }) {
  const router = useRouter();
  useEffect(() => {
    const id = setInterval(() => router.refresh(), seconds * 1000);
    return () => clearInterval(id);
  }, [router, seconds]);
  return null;
}
