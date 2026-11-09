import { Printer, TriangleAlert } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import type { PrinterStatusView } from "@/db/bindings";
import { usePrintDismissFailed, usePrinterStatus, usePrintRecoveryResolve, usePrintRetry } from "@/db/hooks";

const KNOWN_FAULTS = new Set([
  "PAPER_OUT",
  "COVER_OPEN",
  "PRINTER_ERROR",
  "TIMEOUT",
  "CONNECT_FAILED",
  "WRITE_FAILED",
  "PRINTER_NOT_FOUND",
  "STATUS_FAILED",
  "UNSUPPORTED",
  "RENDER_FAILED",
]);

// Amber, not red (doc §1.9 "printer or system problem"): the sale is saved, this is a hardware nag.
const BAR =
  "flex flex-wrap items-center gap-3 border-b border-warning bg-warning/15 px-4 py-2 text-base text-foreground";

/**
 * The persistent printer banner (doc §7.5): unprinted receipts from before a restart ("Print now / Skip",
 * never a silent reprint) and stuck receipts ("Retry / Reprint later"). It never blocks selling: it is a
 * bar, not a modal, and the sale is already saved.
 */
export function PrinterBanner() {
  const { t } = useTranslation("printing");
  const status = usePrinterStatus();
  const resolve = usePrintRecoveryResolve();
  const retry = usePrintRetry();
  const dismiss = usePrintDismissFailed();
  const s: PrinterStatusView | undefined = status.data;
  if (!s) return null;

  const fault = s.fault && KNOWN_FAULTS.has(s.fault) ? t(`faults.${s.fault}`) : t("faults.unknown");
  const waiting = s.failed + (s.health === "ready" ? 0 : s.queued);

  return (
    <div aria-live="polite">
      {s.recovered > 0 && (
        <div role="status" className={BAR}>
          <Printer aria-hidden className="size-5 shrink-0" />
          <span className="flex-1">{t("banner.recovered", { count: s.recovered })}</span>
          <Button size="pos" onClick={() => resolve.mutate(true)} disabled={resolve.isPending}>
            {t("banner.printNow")}
          </Button>
          <Button
            size="pos"
            variant="outline"
            onClick={() => resolve.mutate(false)}
            disabled={resolve.isPending}
          >
            {t("banner.skip")}
          </Button>
        </div>
      )}
      {s.health === "not_configured" && s.queued > 0 && (
        <div role="status" className={BAR}>
          <Printer aria-hidden className="size-5 shrink-0" />
          <span className="flex-1">{t("banner.notConfigured", { count: s.queued })}</span>
        </div>
      )}
      {s.health === "unhealthy" && s.failed === 0 && (
        <div role="status" className={BAR}>
          <TriangleAlert aria-hidden className="size-5 shrink-0" />
          <span className="flex-1">{t("banner.notResponding")}</span>
        </div>
      )}
      {s.health !== "not_configured" && s.failed > 0 && (
        <div role="status" className={BAR}>
          <TriangleAlert aria-hidden className="size-5 shrink-0" />
          <span className="flex-1">{t("banner.waiting", { fault, count: waiting })}</span>
          <Button size="pos" onClick={() => retry.mutate()} disabled={retry.isPending}>
            {t("banner.retry")}
          </Button>
          <Button size="pos" variant="outline" onClick={() => dismiss.mutate()} disabled={dismiss.isPending}>
            {t("banner.later")}
          </Button>
        </div>
      )}
    </div>
  );
}
