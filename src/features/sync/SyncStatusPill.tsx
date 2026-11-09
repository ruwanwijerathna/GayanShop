import { Link } from "@tanstack/react-router";
import { Cloud, CloudOff, LoaderCircle, TriangleAlert } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { SyncState } from "@/db/bindings";
import { useSyncStatus } from "@/db/hooks";
import { cn } from "@/lib/utils";

/** Colour and icon per state: green synced, amber pending, red offline or a problem (doc §1.3). */
const LOOK: Record<SyncState, { tone: string; Icon: typeof Cloud }> = {
  synced: { tone: "bg-success-surface text-success-foreground", Icon: Cloud },
  syncing: { tone: "bg-info text-info-foreground", Icon: LoaderCircle },
  pending: { tone: "bg-warning text-warning-foreground", Icon: Cloud },
  bootstrap_pending: { tone: "bg-warning text-warning-foreground", Icon: LoaderCircle },
  restore_pending: { tone: "bg-warning text-warning-foreground", Icon: LoaderCircle },
  offline: { tone: "bg-destructive text-destructive-foreground", Icon: CloudOff },
  unauthorized: { tone: "bg-destructive text-destructive-foreground", Icon: TriangleAlert },
  error: { tone: "bg-destructive text-destructive-foreground", Icon: TriangleAlert },
  unregistered: { tone: "bg-muted text-muted-foreground", Icon: CloudOff },
};

/** The top bar's sync pill: 🟢 synced / 🟡 pending N / 🔴 offline. Opens Settings → Sync. */
export function SyncStatusPill() {
  const { t } = useTranslation("sync");
  const status = useSyncStatus();
  const s = status.data;
  if (!s) return null;
  const { tone, Icon } = LOOK[s.state];
  const text = t(`pill.${s.state}`, { count: s.pending });
  return (
    <Link
      to="/settings"
      hash="sync"
      aria-label={t("pill.label", { state: text })}
      className={cn("flex h-12 items-center gap-2 rounded-full px-4 text-sm font-medium", tone)}
    >
      <Icon aria-hidden className={cn("size-4", s.state === "syncing" && "motion-safe:animate-spin")} />
      <span>{text}</span>
    </Link>
  );
}
