"use client";

import { useState } from "react";
import CopyButton from "../dashboard/CopyButton";
import { PUBLIC_API_URL } from "@/lib/client";
import { routeLabel, timeAgo } from "@/lib/strait";

const ROUTES = ["BTC_TO_HEMI", "HEMI_TO_BTC", "ETH_TO_HEMI", "HEMI_TO_ETH"];
const STATUSES = ["INITIATED", "ANCHORED", "PROVING", "FINALIZED", "FAILED", "REORGED"];

type Registered = {
  id: string;
  url: string;
  routes: string[] | null;
  assets: string[] | null;
  statuses: string[] | null;
  signing_secret: string;
  management_token: string;
};

type Subscription = {
  id: string;
  url: string;
  routes: string[] | null;
  assets: string[] | null;
  statuses: string[] | null;
  active: boolean;
  created_at: string;
};

type Delivery = {
  id: string;
  transfer_id: string;
  event_type: string;
  status: string;
  attempt_count: number;
  response_ms: number | null;
  last_error: string | null;
  delivered_at: string | null;
  created_at: string;
};

export default function WebhooksClient() {
  const [tab, setTab] = useState<"register" | "manage">("register");

  return (
    <div>
      <div className="mb-6 flex rounded-lg border border-white/10 p-0.5 text-sm w-fit">
        {(["register", "manage"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={`rounded-md px-4 py-1.5 capitalize transition-colors ${
              tab === t ? "bg-white/10 text-white" : "text-zinc-400 hover:text-white"
            }`}
          >
            {t}
          </button>
        ))}
      </div>
      {tab === "register" ? <RegisterTab /> : <ManageTab />}
    </div>
  );
}

/* ── Shared bits ─────────────────────────────────────────────────────────── */

const field =
  "w-full rounded-lg border border-white/10 bg-white/[0.03] px-3 py-2 text-sm text-white placeholder-zinc-500 focus:border-orange-400/50 focus:outline-none";

function ErrorNote({ children }: { children: React.ReactNode }) {
  return (
    <div className="rounded-lg border border-red-500/25 bg-red-500/[0.06] px-4 py-3 text-sm text-red-300">
      {children}
    </div>
  );
}

function CheckGroup({
  legend,
  items,
  selected,
  onToggle,
  label = (v: string) => v,
}: {
  legend: string;
  items: string[];
  selected: string[];
  onToggle: (v: string) => void;
  label?: (v: string) => string;
}) {
  return (
    <fieldset>
      <legend className="mb-2 text-sm text-zinc-400">{legend}</legend>
      <div className="flex flex-wrap gap-2">
        {items.map((v) => {
          const on = selected.includes(v);
          return (
            <button
              type="button"
              key={v}
              onClick={() => onToggle(v)}
              aria-pressed={on}
              className={`rounded-full border px-3 py-1 text-xs transition-colors ${
                on
                  ? "border-orange-400/50 bg-orange-500/10 text-orange-300"
                  : "border-white/10 text-zinc-400 hover:border-white/30 hover:text-white"
              }`}
            >
              {label(v)}
            </button>
          );
        })}
      </div>
    </fieldset>
  );
}

/* ── Register ────────────────────────────────────────────────────────────── */

function RegisterTab() {
  const [url, setUrl] = useState("");
  const [routes, setRoutes] = useState<string[]>([]);
  const [statuses, setStatuses] = useState<string[]>([]);
  const [assets, setAssets] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [created, setCreated] = useState<Registered | null>(null);

  const toggle = (list: string[], set: (v: string[]) => void) => (v: string) =>
    set(list.includes(v) ? list.filter((x) => x !== v) : [...list, v]);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const assetList = assets
        .split(",")
        .map((a) => a.trim())
        .filter(Boolean);
      const res = await fetch(`${PUBLIC_API_URL}/webhooks`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          url,
          routes: routes.length ? routes : undefined,
          statuses: statuses.length ? statuses : undefined,
          assets: assetList.length ? assetList : undefined,
        }),
      });
      const json = await res.json();
      if (!res.ok) {
        setError(json.error ?? `registration failed (HTTP ${res.status})`);
      } else {
        setCreated(json.webhook);
      }
    } catch {
      setError("Could not reach the Strait API — is the indexer up?");
    } finally {
      setBusy(false);
    }
  }

  if (created) return <SecretsPanel created={created} onReset={() => setCreated(null)} />;

  return (
    <form onSubmit={submit} className="space-y-5">
      <div>
        <label htmlFor="wh-url" className="mb-2 block text-sm text-zinc-400">
          Callback URL <span className="text-zinc-600">(public https endpoint on your backend)</span>
        </label>
        <input
          id="wh-url"
          type="url"
          required
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="https://example.com/strait-hook"
          className={field}
          spellCheck={false}
        />
      </div>

      <CheckGroup
        legend="Routes — leave empty for all"
        items={ROUTES}
        selected={routes}
        onToggle={toggle(routes, setRoutes)}
        label={routeLabel}
      />
      <CheckGroup
        legend="Statuses — leave empty for all"
        items={STATUSES}
        selected={statuses}
        onToggle={toggle(statuses, setStatuses)}
      />

      <div>
        <label htmlFor="wh-assets" className="mb-2 block text-sm text-zinc-400">
          Assets <span className="text-zinc-600">(comma-separated, e.g. BTC, ETH, HEMI — empty for all)</span>
        </label>
        <input
          id="wh-assets"
          value={assets}
          onChange={(e) => setAssets(e.target.value)}
          placeholder="BTC, ETH"
          className={field}
          spellCheck={false}
        />
      </div>

      {error && <ErrorNote>{error}</ErrorNote>}

      <button
        type="submit"
        disabled={busy}
        className="rounded-lg bg-orange-500 px-5 py-2 text-sm font-medium text-black transition-colors hover:bg-orange-400 disabled:opacity-50"
      >
        {busy ? "Registering…" : "Register webhook"}
      </button>
    </form>
  );
}

function SecretsPanel({ created, onReset }: { created: Registered; onReset: () => void }) {
  const rows: [string, string][] = [
    ["Subscription ID", created.id],
    ["Signing secret", created.signing_secret],
    ["Management token", created.management_token],
  ];
  return (
    <div className="space-y-5">
      <div className="rounded-xl border border-orange-500/30 bg-orange-500/[0.06] px-5 py-4 text-sm text-zinc-200">
        <p className="font-semibold text-orange-300">
          Store these now — they are shown once and can never be retrieved again.
        </p>
        <p className="mt-1 text-zinc-400">
          The signing secret verifies deliveries to your backend; the management token is
          required to inspect or delete this subscription. Losing them means registering a
          new webhook.
        </p>
      </div>

      <div className="overflow-hidden rounded-xl border border-white/[0.07]">
        <table className="w-full text-sm">
          <tbody>
            {rows.map(([label, value]) => (
              <tr key={label} className="border-b border-white/[0.04] last:border-0">
                <td className="px-4 py-3 text-zinc-400 whitespace-nowrap align-top">{label}</td>
                <td className="px-4 py-3">
                  <div className="flex items-start gap-2">
                    <code className="font-mono text-xs text-orange-300 break-all">{value}</code>
                    <CopyButton value={value} className="mt-0.5 shrink-0" />
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="text-sm text-zinc-400">
        Deliveries to <code className="font-mono text-zinc-300">{created.url}</code>
        {created.routes?.length ? ` · routes: ${created.routes.join(", ")}` : " · all routes"}
        {created.statuses?.length ? ` · statuses: ${created.statuses.join(", ")}` : " · all statuses"}
        {created.assets?.length ? ` · assets: ${created.assets.join(", ")}` : " · all assets"}
      </div>

      <button
        onClick={onReset}
        className="rounded-lg border border-white/10 px-4 py-2 text-sm text-zinc-300 transition-colors hover:border-white/30 hover:text-white"
      >
        Register another
      </button>
    </div>
  );
}

/* ── Manage ──────────────────────────────────────────────────────────────── */

function ManageTab() {
  const [id, setId] = useState("");
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sub, setSub] = useState<Subscription | null>(null);
  const [deliveries, setDeliveries] = useState<Delivery[]>([]);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleted, setDeleted] = useState(false);

  async function load(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    setSub(null);
    setDeleted(false);
    setConfirmDelete(false);
    try {
      const headers = { "X-Management-Token": token.trim() };
      const res = await fetch(`${PUBLIC_API_URL}/webhooks/${id.trim()}`, { headers });
      const json = await res.json();
      if (!res.ok) {
        setError(
          res.status === 404
            ? "No subscription with that id and token — check both values."
            : json.error ?? `lookup failed (HTTP ${res.status})`,
        );
        return;
      }
      setSub(json.webhook);
      const dres = await fetch(`${PUBLIC_API_URL}/webhooks/${id.trim()}/deliveries`, { headers });
      if (dres.ok) {
        const djson = await dres.json();
        setDeliveries(djson.deliveries ?? []);
      } else {
        setDeliveries([]);
      }
    } catch {
      setError("Could not reach the Strait API — is the indexer up?");
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!sub) return;
    setBusy(true);
    setError(null);
    try {
      const res = await fetch(`${PUBLIC_API_URL}/webhooks/${sub.id}`, {
        method: "DELETE",
        headers: { "X-Management-Token": token.trim() },
      });
      if (res.ok) {
        setSub(null);
        setDeliveries([]);
        setDeleted(true);
      } else {
        const json = await res.json();
        setError(json.error ?? `delete failed (HTTP ${res.status})`);
      }
    } catch {
      setError("Could not reach the Strait API — is the indexer up?");
    } finally {
      setBusy(false);
      setConfirmDelete(false);
    }
  }

  return (
    <div className="space-y-6">
      <form onSubmit={load} className="space-y-4">
        <div>
          <label htmlFor="m-id" className="mb-2 block text-sm text-zinc-400">Subscription ID</label>
          <input
            id="m-id"
            required
            value={id}
            onChange={(e) => setId(e.target.value)}
            placeholder="bd002e07-cdcd-4944-a03c-8791fe9b4ccf"
            className={field}
            spellCheck={false}
          />
        </div>
        <div>
          <label htmlFor="m-token" className="mb-2 block text-sm text-zinc-400">Management token</label>
          <input
            id="m-token"
            required
            type="password"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            placeholder="the 64-char token from registration"
            className={field}
            spellCheck={false}
          />
        </div>
        <button
          type="submit"
          disabled={busy}
          className="rounded-lg bg-orange-500 px-5 py-2 text-sm font-medium text-black transition-colors hover:bg-orange-400 disabled:opacity-50"
        >
          {busy ? "Loading…" : "Load subscription"}
        </button>
      </form>

      {error && <ErrorNote>{error}</ErrorNote>}
      {deleted && (
        <div className="rounded-lg border border-emerald-500/25 bg-emerald-500/[0.06] px-4 py-3 text-sm text-emerald-300">
          Subscription deleted — its pending deliveries were removed with it.
        </div>
      )}

      {sub && (
        <>
          <div className="overflow-hidden rounded-xl border border-white/[0.07]">
            <table className="w-full text-sm">
              <tbody>
                {(
                  [
                    ["URL", sub.url],
                    ["Routes", sub.routes?.length ? sub.routes.map(routeLabel).join(", ") : "all"],
                    ["Statuses", sub.statuses?.length ? sub.statuses.join(", ") : "all"],
                    ["Assets", sub.assets?.length ? sub.assets.join(", ") : "all"],
                    ["Active", sub.active ? "yes" : "no"],
                    ["Created", `${new Date(sub.created_at).toLocaleString()} (${timeAgo(sub.created_at)})`],
                  ] as [string, string][]
                ).map(([k, v]) => (
                  <tr key={k} className="border-b border-white/[0.04] last:border-0">
                    <td className="px-4 py-3 text-zinc-400 whitespace-nowrap align-top">{k}</td>
                    <td className="px-4 py-3 text-zinc-200 break-all">{v}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <div>
            <h3 className="mb-3 text-base font-semibold text-white">
              Recent deliveries <span className="text-xs font-normal text-zinc-500">(last 20)</span>
            </h3>
            {deliveries.length === 0 ? (
              <p className="text-sm text-zinc-500">
                None yet — deliveries appear here once a matching transfer changes.
              </p>
            ) : (
              <div className="overflow-x-auto rounded-xl border border-white/[0.07]">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="text-left text-xs uppercase tracking-wide text-zinc-500 border-b border-white/[0.07]">
                      <th className="font-medium px-4 py-3">When</th>
                      <th className="font-medium px-4 py-3">Event</th>
                      <th className="font-medium px-4 py-3">Status</th>
                      <th className="font-medium px-4 py-3">Attempts</th>
                      <th className="font-medium px-4 py-3">Response</th>
                      <th className="font-medium px-4 py-3">Error</th>
                    </tr>
                  </thead>
                  <tbody>
                    {deliveries.map((d) => (
                      <tr key={d.id} className="border-b border-white/[0.04] last:border-0">
                        <td className="px-4 py-3 text-zinc-400 whitespace-nowrap" title={d.created_at}>
                          {timeAgo(d.created_at)}
                        </td>
                        <td className="px-4 py-3 font-mono text-xs text-zinc-300 whitespace-nowrap">
                          {d.event_type}
                        </td>
                        <td className="px-4 py-3 whitespace-nowrap">
                          <DeliveryBadge status={d.status} />
                        </td>
                        <td className="px-4 py-3 text-zinc-400 tabular-nums">{d.attempt_count}</td>
                        <td className="px-4 py-3 text-zinc-400 tabular-nums whitespace-nowrap">
                          {d.response_ms != null ? `${d.response_ms} ms` : "—"}
                        </td>
                        <td className="px-4 py-3 text-zinc-500 max-w-[16rem] truncate" title={d.last_error ?? ""}>
                          {d.last_error ?? "—"}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>

          <div className="flex items-center gap-3 border-t border-white/[0.06] pt-5">
            {confirmDelete ? (
              <>
                <span className="text-sm text-zinc-400">Delete this subscription permanently?</span>
                <button
                  onClick={remove}
                  disabled={busy}
                  className="rounded-lg bg-red-500/90 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-red-500 disabled:opacity-50"
                >
                  Yes, delete
                </button>
                <button
                  onClick={() => setConfirmDelete(false)}
                  className="rounded-lg border border-white/10 px-4 py-2 text-sm text-zinc-300 hover:text-white"
                >
                  Cancel
                </button>
              </>
            ) : (
              <button
                onClick={() => setConfirmDelete(true)}
                className="rounded-lg border border-red-500/30 px-4 py-2 text-sm text-red-400 transition-colors hover:border-red-500/60 hover:text-red-300"
              >
                Delete subscription
              </button>
            )}
          </div>
        </>
      )}
    </div>
  );
}

function DeliveryBadge({ status }: { status: string }) {
  const style =
    status === "DELIVERED"
      ? { dot: "bg-emerald-400", text: "text-emerald-300" }
      : status === "FAILED"
        ? { dot: "bg-red-500", text: "text-red-400" }
        : { dot: "bg-zinc-400", text: "text-zinc-300" };
  return (
    <span className={`inline-flex items-center gap-1.5 ${style.text}`}>
      <span className={`h-2 w-2 rounded-full ${style.dot}`} />
      {status.toLowerCase()}
    </span>
  );
}
