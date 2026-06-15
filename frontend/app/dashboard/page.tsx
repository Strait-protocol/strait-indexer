import { redirect } from "next/navigation";

// /dashboard has no network of its own — default to mainnet.
export default function DashboardIndex() {
  redirect("/dashboard/mainnet");
}
