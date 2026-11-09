import { Can } from "@/components/auth/Can";
import { Dashboard } from "@/features/dashboard/Dashboard";
import { CounterStatus } from "@/features/home/CounterStatus";

/** Home: this counter's health, and the dashboard for staff who may see reports. Composed here, because
 * features never import each other. */
export function HomePage() {
  return (
    <div className="flex flex-col gap-6">
      <CounterStatus />
      <Can perm="REPORT_SALES">
        <Dashboard />
      </Can>
    </div>
  );
}
