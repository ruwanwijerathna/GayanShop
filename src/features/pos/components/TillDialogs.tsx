import { useTranslation } from "react-i18next";
import { Kbd } from "@/components/common/Kbd";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import type { HeldCart_Serialize as HeldCart } from "@/db/bindings";
import { useHeldCarts } from "@/db/hooks";
import { formatMoney } from "@/lib/money";
import { SHORTCUT_LABELS, type PosAction } from "../lib/shortcuts";
import { parsePayload, previewTotals } from "../store/cart.store";

/** F11: the held sales on this counter, newest first. */
export function HeldSalesDialog(props: {
  onRecall: (cart: HeldCart) => void;
  onClose: () => void;
  /** The cart in progress will be held first. */
  swapping: boolean;
}) {
  const { t } = useTranslation("pos");
  const held = useHeldCarts(true);
  const carts = held.data ?? [];
  return (
    <Dialog open onOpenChange={(o) => !o && props.onClose()}>
      <DialogContent className="max-w-xl">
        <DialogHeader>
          <DialogTitle>{t("recall.title")}</DialogTitle>
          <DialogDescription>{props.swapping ? t("recall.swap") : t("recall.title")}</DialogDescription>
        </DialogHeader>
        {carts.length === 0 ? (
          <p className="text-base text-muted-foreground">{t("recall.empty")}</p>
        ) : (
          <ul className="flex max-h-96 flex-col gap-2 overflow-y-auto">
            {carts.map((c) => {
              const p = parsePayload(c.payload);
              const total = p ? (previewTotals(p)?.grand_total ?? 0) : 0;
              return (
                <li key={c.id} className="flex min-h-12 items-center gap-3 rounded-md border px-3 py-2">
                  <div className="flex flex-1 flex-col">
                    <span className="text-base font-medium">
                      {c.label ?? t("hold.label", { time: new Date(c.created_at).toLocaleTimeString() })}
                    </span>
                    <span className="text-sm text-muted-foreground">
                      {t("recall.lines", { count: p?.lines.length ?? 0, total: formatMoney(total) })}
                    </span>
                  </div>
                  <Button type="button" size="pos" disabled={!p} onClick={() => props.onRecall(c)}>
                    {t("recall.take")}
                  </Button>
                </li>
              );
            })}
          </ul>
        )}
      </DialogContent>
    </Dialog>
  );
}

const HELP_ORDER: PosAction[] = [
  "help",
  "quickSale",
  "search",
  "phone",
  "lineDiscount",
  "billDiscount",
  "hold",
  "pay",
  "void",
  "recall",
  "exchange",
  "cancel",
];

/** F1: every till shortcut. */
export function HelpDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation("pos");
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("help.title")}</DialogTitle>
          <DialogDescription className="sr-only">{t("help.title")}</DialogDescription>
        </DialogHeader>
        <dl className="grid grid-cols-[auto_1fr] items-center gap-x-4 gap-y-2 text-base">
          {HELP_ORDER.map((a) => (
            <div key={a} className="contents">
              <dt>
                <Kbd>{SHORTCUT_LABELS[a]}</Kbd>
              </dt>
              <dd>{t(`help.keys.${a}`)}</dd>
            </div>
          ))}
          <dt>
            <Kbd>Ctrl+L</Kbd>
          </dt>
          <dd>{t("help.lock")}</dd>
          <dt>
            <Kbd>Ctrl+Shift+L</Kbd>
          </dt>
          <dd>{t("help.language")}</dd>
        </dl>
      </DialogContent>
    </Dialog>
  );
}
