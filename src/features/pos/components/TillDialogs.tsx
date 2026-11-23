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

const FUNCTION_ACTIONS: Exclude<PosAction, "phone">[] = [
  "help",
  "quickSale",
  "search",
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
      <DialogContent className="max-w-xl max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("help.title")}</DialogTitle>
          <DialogDescription className="sr-only">{t("help.title")}</DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-5 text-sm sm:text-base">
          {/* Function keys */}
          <section className="flex flex-col gap-2">
            <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              {t("help.sectionFunction", { defaultValue: "Function Keys (press again to close)" })}
            </h3>
            <dl className="grid grid-cols-[auto_1fr] items-center gap-x-4 gap-y-2">
              {FUNCTION_ACTIONS.map((a) => (
                <div key={a} className="contents">
                  <dt>
                    <Kbd>{SHORTCUT_LABELS[a]}</Kbd>
                  </dt>
                  <dd className="text-sm sm:text-base">{t(`help.keys.${a}`)}</dd>
                </div>
              ))}
            </dl>
          </section>

          {/* Cart & Navigation */}
          <section className="flex flex-col gap-2">
            <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              {t("help.sectionCart", { defaultValue: "Cart Navigation & Actions" })}
            </h3>
            <dl className="grid grid-cols-[auto_1fr] items-center gap-x-4 gap-y-2">
              <dt>
                <div className="flex items-center gap-1">
                  <Kbd>↑</Kbd>
                  <Kbd>↓</Kbd>
                </div>
              </dt>
              <dd className="text-sm sm:text-base">
                {t("help.cartKeys.nav", {
                  defaultValue: "Navigate cart items (or search results)",
                })}
              </dd>

              <dt>
                <Kbd>Delete</Kbd>
              </dt>
              <dd className="text-sm sm:text-base">
                {t("help.cartKeys.delete", {
                  defaultValue: "Remove selected item from cart",
                })}
              </dd>

              <dt>
                <Kbd>Ctrl+→</Kbd>
              </dt>
              <dd className="text-sm sm:text-base">
                {t("help.cartKeys.qtyIncrease", {
                  defaultValue: "Increase selected item quantity (+1)",
                })}
              </dd>

              <dt>
                <Kbd>Ctrl+←</Kbd>
              </dt>
              <dd className="text-sm sm:text-base">
                {t("help.cartKeys.qtyDecrease", {
                  defaultValue: "Decrease selected item quantity (-1)",
                })}
              </dd>
            </dl>
          </section>

          {/* System */}
          <section className="flex flex-col gap-2">
            <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              {t("help.sectionSystem", { defaultValue: "System" })}
            </h3>
            <dl className="grid grid-cols-[auto_1fr] items-center gap-x-4 gap-y-2">
              <dt>
                <Kbd>Ctrl+L</Kbd>
              </dt>
              <dd className="text-sm sm:text-base">{t("help.lock")}</dd>

              <dt>
                <Kbd>Ctrl+Shift+L</Kbd>
              </dt>
              <dd className="text-sm sm:text-base">{t("help.language")}</dd>
            </dl>
          </section>
        </div>
      </DialogContent>
    </Dialog>
  );
}
