import { Link } from "@tanstack/react-router";
import {
  ArrowLeftRight,
  CircleHelp,
  Ellipsis,
  Hand,
  History,
  Percent,
  Phone,
  Printer,
  Search,
  SquarePen,
  Undo2,
  X,
  Zap,
} from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useSession } from "@/app/session";
import { ApprovalDialog } from "@/components/auth/ApprovalDialog";
import { Kbd } from "@/components/common/Kbd";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import type {
  HeldCart_Serialize as HeldCart,
  PaymentLine_Deserialize as PaymentLine,
  Perm,
  ProductSummary_Serialize as ProductSummary,
} from "@/db/bindings";
import { CommandError } from "@/db/client";
import { commands } from "@/db/bindings";
import {
  discardCurrentCart,
  lookupProductMatches,
  saveCurrentCart,
  useCheckoutOrder,
  useCurrentCart,
  useCurrentCashSession,
  useHoldCart,
  useProductSearch,
  useRecallCart,
} from "@/db/hooks";
import { usePrintReceiptStore } from "@/components/receipt/PrintableReceiptPortal";
import { notFoundToast, routeScan } from "@/hardware/scanner/feedback";
import { useScanHandler, useScanner } from "@/hardware/scanner/ScannerProvider";
import { translateError } from "@/i18n";
import { playCue } from "@/lib/audio";
import { formatMoney } from "@/lib/money";
import { cn } from "@/lib/utils";
import { Cart } from "./components/Cart";
import { CustomerPhoneField, formatTyped } from "./components/CustomerPhoneField";
import { ExchangeDialog } from "./components/ExchangeDialog";
import { BillDiscountDialog, LineEditDialog } from "./components/DiscountDialogs";
import { PaymentDialog } from "./components/PaymentDialog";
import { QuickSaleBar } from "./components/QuickSaleBar";
import { HeldSalesDialog, HelpDialog } from "./components/TillDialogs";
import { VoidDialog } from "./components/VoidDialog";
import { BarcodeDisambiguationDialog } from "./dialogs/BarcodeDisambiguationDialog";
import { amountDue, toExchangeInput, totalCredit } from "./lib/exchange";
import { withinLimit } from "./lib/pricing";
import { routeShortcut, SHORTCUT_LABELS, type PosAction } from "./lib/shortcuts";
import { cartStore, parsePayload, previewTotals, toCheckoutLines, useCart } from "./store/cart.store";

/** The crash-recovery cart is written at most this often (doc §1.4). */
export const SAVE_DEBOUNCE_MS = 500;
const DONE_BANNER_MS = 2_000;

type Dialog = "pay" | "line" | "bill" | "void" | "recall" | "help" | "exchange";

/** The till (doc §1.9). Needs an open cash session on this counter. */
export function PosScreen() {
  const { t } = useTranslation("pos");
  const cash = useCurrentCashSession();
  if (cash.isPending) return null;
  if (!cash.data) {
    return (
      <div className="flex flex-col items-center gap-4 p-10 text-center">
        <p className="text-lg">{t("noSession")}</p>
        <Button asChild size="pos">
          <Link to="/cash">{t("openCash")}</Link>
        </Button>
      </div>
    );
  }
  return <Till />;
}

function isTextField(el: Element | null): boolean {
  return (
    el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement
  );
}

function Till() {
  const { t, i18n } = useTranslation("pos");
  const { session } = useSession();
  const scanner = useScanner();
  const lines = useCart((s) => s.lines);
  const billDiscount = useCart((s) => s.billDiscount);
  const selectedKey = useCart((s) => s.selectedKey);
  const exchange = useCart((s) => s.exchange);
  const customerPhone = useCart((s) => s.customerPhone);
  const priced = previewTotals({ lines, billDiscount });
  // An exchange: the cart is the replacement; the customer pays only what exceeds the returned goods' value.
  const credit = exchange ? totalCredit(exchange.source, exchange.picks) : 0;
  const owed = amountDue(priced?.grand_total ?? 0, credit);

  const scanRef = useRef<HTMLInputElement>(null);
  const phoneRef = useRef<HTMLInputElement>(null);
  const quickRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [quickText, setQuickText] = useState("");
  // The optional phone / quick-sale fields live in one inline panel that is closed until asked for (F4 / F2).
  const [extra, setExtra] = useState<{ kind: "phone" | "quick"; n: number } | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [voidPreselect, setVoidPreselect] = useState<string | null>(null);
  const [approval, setApproval] = useState<{ perm: Perm; payments: PaymentLine[]; soldByNo: number } | null>(
    null,
  );
  const [shared, setShared] = useState<{ code: string; products: ProductSummary[] } | null>(null);
  const [done, setDone] = useState<{
    receipt: string;
    change: number;
    exchange: boolean;
    orderId?: string;
  } | null>(null);

  const search = useProductSearch(query);
  const results = query.trim() === "" ? [] : (search.data ?? []);
  const checkout = useCheckoutOrder();
  const hold = useHoldCart();
  const recall = useRecallCart();
  const current = useCurrentCart();

  const focusScan = () => window.setTimeout(() => scanRef.current?.focus(), 0);
  const closeDialog = () => {
    setDialog(null);
    focusScan();
  };

  // Focus starts (and always returns) on the scan field.
  useEffect(() => {
    scanRef.current?.focus();
  }, []);

  const openExtra = (kind: "phone" | "quick") => setExtra((e) => ({ kind, n: (e?.n ?? 0) + 1 }));
  const closeExtra = () => {
    setExtra(null);
    focusScan();
  };
  // Opening the panel (or pressing its key again) puts the cursor in its field.
  useEffect(() => {
    if (!extra) return;
    (extra.kind === "phone" ? phoneRef : quickRef).current?.focus();
  }, [extra]);

  // "Recover cart?" while the till is still empty, if the app closed with a sale in progress.
  const [recoverDismissed, setRecoverDismissed] = useState(false);
  const recoverable = current.data ? parsePayload(current.data.payload) : null;
  const recover =
    !recoverDismissed && lines.length === 0 && recoverable && recoverable.lines.length > 0
      ? recoverable
      : null;

  // Debounced crash-recovery write of the cart in progress (local only; nothing is sold until checkout).
  useEffect(() => {
    let timer: number | null = null;
    const unsubscribe = cartStore.subscribe((s, prev) => {
      if (
        s.lines === prev.lines &&
        s.billDiscount === prev.billDiscount &&
        s.customerPhone === prev.customerPhone &&
        s.customerName === prev.customerName &&
        s.token === prev.token &&
        s.exchange === prev.exchange
      )
        return;
      // An exchange is never saved for recovery: a restored cart without it would be a plain sale.
      if (s.exchange) {
        if (timer !== null) window.clearTimeout(timer);
        timer = null;
        return;
      }
      if (timer !== null) window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        const snap = cartStore.getState().snapshot();
        const write =
          snap.lines.length > 0
            ? saveCurrentCart({ payload: JSON.stringify(snap), checkout_token: snap.token, label: null })
            : discardCurrentCart();
        write.catch(() => undefined); // best effort: a failed save must never interrupt selling
      }, SAVE_DEBOUNCE_MS);
    });
    return () => {
      unsubscribe();
      if (timer !== null) window.clearTimeout(timer);
    };
  }, []);

  // The emerald "Sale complete" banner never blocks the next scan, and goes after 2 s.
  useEffect(() => {
    if (!done) return;
    const timer = window.setTimeout(() => setDone(null), DONE_BANNER_MS);
    return () => window.clearTimeout(timer);
  }, [done]);

  const addProduct = (p: ProductSummary) => {
    cartStore.getState().addProduct(p);
    setDone(null);
  };

  const notFound = (code: string) => {
    notFoundToast(t("scanner.notFound", { code }));
    setQuickText(`${code} `);
    openExtra("quick");
  };

  useScanHandler(async (code) => {
    setQuery("");
    await routeScan(code, {
      onProduct: addProduct,
      onMultipleProducts: (products, shownCode) => setShared({ code: shownCode, products }),
      onOrder: (id) => {
        setVoidPreselect(id);
        setDialog("void");
      },
      onUnknown: () => undefined,
      notFound,
    });
  });

  /** Enter in the scan field: an exact barcode/SKU first, then the highlighted search result. */
  const enterQuery = async () => {
    const q = query.trim();
    if (q === "") return;
    const exact = await lookupProductMatches(q).catch(() => []);
    setQuery("");
    setActive(0);
    if (exact.length > 1) {
      playCue("approval");
      setShared({ code: q, products: exact });
      return;
    }
    const pick = exact[0] ?? results[active] ?? null;
    if (pick) {
      playCue("tick");
      addProduct(pick);
    } else {
      playCue("error");
      notFound(q);
    }
  };

  const onScanKey = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      void enterQuery();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(a + 1, Math.max(0, results.length - 1)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(0, a - 1));
    }
  };

  const pay = (payments: PaymentLine[], soldByNo: number, approvalId: string | null) => {
    const s = cartStore.getState();
    checkout.mutate(
      {
        checkout_token: s.token,
        lines: toCheckoutLines(s.lines),
        bill_discount: s.billDiscount,
        payments,
        customer_phone: s.customerPhone.trim() || null,
        customer_name: s.customerName.trim() || null,
        note: null,
        sold_by_no: soldByNo,
        approval_id: approvalId,
        exchange: s.exchange ? toExchangeInput(s.exchange.source, s.exchange.picks) : null,
      },
      {
        onSuccess: (r) => {
          playCue("complete");
          const wasExchange = cartStore.getState().exchange !== null;
          cartStore.getState().clear();
          setApproval(null);
          setDone({
            receipt: r.receipt_no,
            change: r.change_due,
            exchange: wasExchange,
            orderId: r.order_id,
          });
          closeDialog();
        },
        onError: (e) => {
          if (e instanceof CommandError && e.error.kind === "APPROVAL_REQUIRED") {
            playCue("approval");
            setApproval({ perm: e.error.perm, payments, soldByNo });
          } else {
            playCue("error");
          }
        },
      },
    );
  };

  const holdSale = (after?: () => void) => {
    if (cartStore.getState().exchange) {
      playCue("error");
      toast.error(t("exchange.finishFirst"));
      return;
    }
    const snap = cartStore.getState().snapshot();
    if (snap.lines.length === 0) {
      after?.();
      return;
    }
    hold.mutate(
      { payload: JSON.stringify(snap), checkout_token: snap.token, label: snap.customerName.trim() || null },
      {
        onSuccess: () => {
          cartStore.getState().clear();
          toast.success(t("hold.held"));
          if (after) after();
          else focusScan();
        },
        onError: (e) => {
          playCue("error");
          if (e instanceof CommandError) toast.error(translateError(i18n.t, e.error));
        },
      },
    );
  };

  const recallSale = (cart: HeldCart) => {
    holdSale(() =>
      recall.mutate(cart.id, {
        onSuccess: (c) => {
          const p = parsePayload(c.payload);
          if (p) cartStore.getState().restore(p);
          closeDialog();
        },
      }),
    );
  };

  const run = (action: PosAction) => {
    const has = cartStore.getState().lines.length > 0;
    switch (action) {
      case "help":
        setDialog("help");
        break;
      case "quickSale":
        openExtra("quick");
        break;
      case "search":
        scanRef.current?.focus();
        break;
      case "phone":
        openExtra("phone");
        break;
      case "lineDiscount":
        if (cartStore.getState().selectedKey) setDialog("line");
        break;
      case "billDiscount":
        if (has) setDialog("bill");
        break;
      case "hold":
        holdSale();
        break;
      case "exchange":
        checkout.reset();
        setDialog("exchange");
        break;
      case "pay":
        if (has && priced && !owed.short) {
          checkout.reset();
          setDialog("pay");
        }
        break;
      case "void":
        setVoidPreselect(null);
        setDialog("void");
        break;
      case "recall":
        if (cartStore.getState().exchange) {
          playCue("error");
          toast.error(t("exchange.finishFirst"));
        } else setDialog("recall");
        break;
      case "cancel":
        setQuery("");
        setQuickText("");
        setExtra(null);
        focusScan();
        break;
    }
  };

  // Till shortcuts, after the scanner's capture-phase listener has had its say (doc §1.4, §7.1).
  const runRef = useRef(run);
  useEffect(() => {
    runRef.current = run;
  });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = document.activeElement;
      const action = routeShortcut(e, {
        capturing: scanner.isCapturing(),
        dialogOpen: document.querySelector('[role="dialog"]') !== null,
        inTextField: isTextField(el) && el !== scanRef.current,
      });
      if (!action) return;
      e.preventDefault();
      runRef.current(action);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [scanner]);

  const selected = lines.find((l) => l.key === selectedKey) ?? null;
  const overLimit =
    priced !== null &&
    session !== null &&
    priced.lines.some(
      (l) => !withinLimit(l.line_discount + l.order_discount_share, l.line_gross, session.max_discount_bp),
    );
  const itemCount = lines.reduce((a, l) => a + l.qty, 0);
  const payError =
    checkout.error instanceof CommandError && checkout.error.error.kind !== "APPROVAL_REQUIRED"
      ? translateError(i18n.t, checkout.error.error)
      : null;

  const menuItem = (a: PosAction, label: string, icon: React.ReactNode, disabled = false) => (
    <DropdownMenuItem
      key={a}
      disabled={disabled}
      className="min-h-12 gap-3 text-base"
      onSelect={() => {
        // Let the menu close and give focus back before the action opens its own field or dialog.
        window.setTimeout(() => run(a), 0);
      }}
    >
      {icon}
      <span className="flex-1">{label}</span>
      <Kbd>{SHORTCUT_LABELS[a]}</Kbd>
    </DropdownMenuItem>
  );

  return (
    <div className="-m-4 flex min-h-0 flex-1 flex-col overflow-hidden">
      {recover && (
        <div
          role="status"
          className="flex flex-wrap items-center gap-3 border-b bg-warning/15 px-4 py-2 text-base"
        >
          <span className="flex-1">{t("recover.message")}</span>
          <Button
            size="pos"
            onClick={() => {
              cartStore.getState().restore(recover);
              setRecoverDismissed(true);
              focusScan();
            }}
          >
            {t("recover.restore")}
          </Button>
          <Button
            size="pos"
            variant="outline"
            onClick={() => {
              discardCurrentCart().catch(() => undefined);
              setRecoverDismissed(true);
              focusScan();
            }}
          >
            {t("recover.discard")}
          </Button>
        </div>
      )}
      {done && (
        <div
          role="status"
          className="flex flex-wrap items-center justify-between gap-3 border-b bg-success-surface px-4 py-2 text-lg font-semibold text-success-foreground"
        >
          <div>
            {done.exchange ? t("exchange.done") : t("done.title")} ·{" "}
            {t("done.receipt", { receipt: done.receipt })}
            {done.change > 0 && ` · ${t("done.change", { change: formatMoney(done.change) })}`}
          </div>
          {done.orderId && (
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="border-success-foreground/40 bg-white/70 text-success-foreground hover:bg-white"
              onClick={async () => {
                const orderId = done.orderId;
                if (orderId) {
                  const res = await commands.orderReceipt(orderId);
                  if (res.status === "ok") {
                    usePrintReceiptStore.getState().print(res.data);
                  }
                }
              }}
            >
              <Printer className="mr-1.5 size-4" />
              {t("done.printReceipt", { defaultValue: "බිල්පත මුද්‍රණය (Print)" })}
            </Button>
          )}
        </div>
      )}

      <div className="grid min-h-0 flex-1 grid-cols-[3fr_2fr]">
        <section className="flex min-h-0 flex-col border-r" aria-label={t("cart.title")}>
          <header className="flex h-14 shrink-0 items-center justify-between border-b px-4">
            <h1 className="text-xl font-semibold">{t("cart.title")}</h1>
            <span className="text-base text-muted-foreground tabular-nums">
              {t("totals.items")}: {itemCount}
            </span>
          </header>
          <Cart priced={priced} />
          <p className="sr-only" aria-live="polite">
            {t("cart.announce", { count: itemCount, total: formatMoney(priced?.grand_total ?? 0) })}
          </p>
        </section>

        <section className="flex min-h-0 flex-col gap-3 bg-muted/40 p-4">
          <div className="flex flex-col gap-1">
            <label htmlFor="scan-field" className="flex items-center gap-2 text-base font-medium">
              <Search aria-hidden className="size-4" />
              {t("scan.label")}
              <Kbd>F3</Kbd>
            </label>
            <Input
              id="scan-field"
              ref={scanRef}
              className="h-14 bg-background text-lg"
              autoComplete="off"
              placeholder={t("scan.placeholder")}
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                setActive(0);
              }}
              onKeyDown={onScanKey}
              aria-describedby="scan-hint"
            />
            <p id="scan-hint" className="text-sm text-muted-foreground">
              {t("scan.hint")}
            </p>
          </div>

          {extra && (
            <div className="relative rounded-lg border bg-background p-3">
              <Button
                type="button"
                variant="ghost"
                size="icon-pos"
                className="absolute top-1 right-1"
                aria-label={t("common:close")}
                onClick={closeExtra}
              >
                <X aria-hidden />
              </Button>
              {extra.kind === "phone" ? (
                <CustomerPhoneField inputRef={phoneRef} onDone={closeExtra} />
              ) : (
                <QuickSaleBar
                  inputRef={quickRef}
                  value={quickText}
                  onChange={setQuickText}
                  onAdd={(q) => {
                    cartStore.getState().addQuickSale(q);
                    playCue("tick");
                  }}
                  onAddProduct={(p) => {
                    addProduct(p);
                    playCue("tick");
                  }}
                  onDone={closeExtra}
                />
              )}
            </div>
          )}
          {!extra && customerPhone !== "" && (
            <Button
              type="button"
              variant="outline"
              size="pos"
              className="justify-start font-mono"
              onClick={() => openExtra("phone")}
            >
              <Phone aria-hidden />
              {formatTyped(customerPhone)}
            </Button>
          )}

          <div className="min-h-0 flex-1 overflow-y-auto" aria-label={t("scan.results")}>
            {query.trim() !== "" &&
              (results.length === 0 && !search.isPending ? (
                <p className="p-2 text-base text-muted-foreground">
                  {t("scan.noResults", { query: query.trim() })}
                </p>
              ) : (
                <ul className="flex flex-col gap-2" role="listbox" aria-label={t("scan.results")}>
                  {results.map((p, i) => (
                    <li key={p.id} role="option" aria-selected={i === active}>
                      <button
                        type="button"
                        className={cn(
                          "flex min-h-12 w-full items-center gap-3 rounded-md border bg-background px-3 text-left text-base",
                          i === active && "border-primary ring-2 ring-primary/40",
                        )}
                        onClick={() => {
                          addProduct(p);
                          playCue("tick");
                          setQuery("");
                          focusScan();
                        }}
                      >
                        <span className="flex-1 truncate">{p.name}</span>
                        <span className="font-mono text-sm text-muted-foreground">{p.barcode ?? p.sku}</span>
                        <span className="tabular-nums">{formatMoney(p.price)}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              ))}
          </div>

          <div className="flex flex-col gap-2 rounded-lg border bg-background p-4">
            {(priced?.discount_total ?? 0) > 0 && (
              <dl className="grid grid-cols-2 gap-x-4 text-base">
                <dt className="text-muted-foreground">{t("totals.subtotal")}</dt>
                <dd className="text-right tabular-nums">{formatMoney(priced?.gross_sales ?? 0)}</dd>
                <dt className="text-muted-foreground">{t("totals.discounts")}</dt>
                <dd className="text-right tabular-nums">− {formatMoney(priced?.discount_total ?? 0)}</dd>
              </dl>
            )}
            <div className="flex items-baseline justify-between">
              <span className="text-xl font-semibold">{t("totals.total")}</span>
              <span className="text-[40px] leading-tight font-bold tabular-nums" data-testid="grand-total">
                {formatMoney(priced?.grand_total ?? 0)}
              </span>
            </div>
            {exchange && (
              <div className="flex flex-col gap-1 rounded-md border bg-accent/40 p-2" role="status">
                <p className="flex flex-wrap items-center gap-2 text-base font-semibold">
                  <ArrowLeftRight aria-hidden className="size-4" />
                  {t("exchange.banner", { receipt: exchange.source.receipt_no })}
                  <span className="font-normal text-muted-foreground">
                    {t("exchange.returned", {
                      units: Object.values(exchange.picks).reduce((a, q) => a + q, 0),
                    })}
                  </span>
                </p>
                <p className="flex justify-between text-base">
                  <span>{t("exchange.creditLine")}</span>
                  <span className="tabular-nums" data-testid="exchange-credit-line">
                    − {formatMoney(credit)}
                  </span>
                </p>
                <p className="flex items-baseline justify-between text-xl font-bold">
                  <span>{t("exchange.toPay")}</span>
                  <span className="tabular-nums" data-testid="exchange-to-pay">
                    {formatMoney(owed.due)}
                  </span>
                </p>
                {lines.length === 0 && (
                  <p className="text-sm text-muted-foreground">{t("exchange.needReplacement")}</p>
                )}
                {owed.short && lines.length > 0 && (
                  <p role="alert" className="text-sm text-destructive">
                    {t("exchange.tooLow", { credit: formatMoney(credit) })}
                  </p>
                )}
                <div className="flex gap-2">
                  <Button type="button" variant="outline" size="pos" onClick={() => run("exchange")}>
                    {t("exchange.change")}
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="pos"
                    onClick={() => {
                      cartStore.getState().setExchange(null);
                      focusScan();
                    }}
                  >
                    {t("exchange.cancel")}
                  </Button>
                </div>
              </div>
            )}
            {overLimit && <p className="text-sm text-muted-foreground">{t("totals.needsApproval")}</p>}
          </div>

          <Button
            type="button"
            size="pos-lg"
            className="w-full text-2xl"
            disabled={lines.length === 0 || priced === null || owed.short}
            onClick={() => run("pay")}
          >
            {t("actions.pay")}
            <Kbd className="border-primary-foreground/40 bg-transparent text-primary-foreground">F9</Kbd>
          </Button>
          <div className="grid grid-cols-2 gap-2">
            <Button type="button" variant="outline" size="pos" onClick={() => run("exchange")}>
              <ArrowLeftRight aria-hidden />
              {t("actions.exchange")}
              <Kbd>{SHORTCUT_LABELS.exchange}</Kbd>
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button type="button" variant="outline" size="pos">
                  <Ellipsis aria-hidden />
                  {t("actions.more")}
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" side="top" className="w-72">
                {menuItem("phone", t("customer.label"), <Phone aria-hidden />)}
                {menuItem("quickSale", t("quick.label"), <Zap aria-hidden />)}
                <DropdownMenuSeparator />
                {menuItem(
                  "hold",
                  t("actions.hold"),
                  <Hand aria-hidden />,
                  lines.length === 0 || hold.isPending || exchange !== null,
                )}
                {menuItem("recall", t("actions.recall"), <History aria-hidden />)}
                <DropdownMenuSeparator />
                {menuItem("lineDiscount", t("actions.lineDiscount"), <SquarePen aria-hidden />, !selected)}
                {menuItem(
                  "billDiscount",
                  t("actions.billDiscount"),
                  <Percent aria-hidden />,
                  lines.length === 0,
                )}
                <DropdownMenuSeparator />
                {menuItem("void", t("actions.void"), <Undo2 aria-hidden />)}
                {menuItem("help", t("actions.help"), <CircleHelp aria-hidden />)}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </section>
      </div>

      {dialog === "pay" && priced && (
        <PaymentDialog
          open={approval === null}
          due={exchange ? owed.due : priced.grand_total}
          pending={checkout.isPending}
          error={payError}
          onPay={(payments, soldByNo) => pay(payments, soldByNo, null)}
          onCancel={closeDialog}
        />
      )}
      {approval && (
        <ApprovalDialog
          open
          perm={approval.perm}
          targetRef={cartStore.getState().token}
          onApproved={(id) => pay(approval.payments, approval.soldByNo, id)}
          onCancel={() => setApproval(null)}
        />
      )}
      {dialog === "line" && selected && <LineEditDialog line={selected} onClose={closeDialog} />}
      {dialog === "bill" && priced && (
        <BillDiscountDialog
          current={billDiscount}
          base={priced.gross_sales - priced.lines.reduce((a, l) => a + l.line_discount, 0)}
          onClose={closeDialog}
        />
      )}
      {dialog === "exchange" && (
        <ExchangeDialog
          draft={exchange}
          onConfirm={(draft) => {
            cartStore.getState().setExchange(draft);
            playCue("tick");
            closeDialog();
          }}
          onClose={closeDialog}
        />
      )}
      {dialog === "void" && <VoidDialog preselect={voidPreselect} onClose={closeDialog} />}
      {dialog === "recall" && (
        <HeldSalesDialog swapping={lines.length > 0} onRecall={recallSale} onClose={closeDialog} />
      )}
      {dialog === "help" && <HelpDialog onClose={closeDialog} />}
      {shared && (
        <BarcodeDisambiguationDialog
          code={shared.code}
          products={shared.products}
          onSelect={(p) => {
            playCue("tick");
            addProduct(p);
            setShared(null);
            focusScan();
          }}
          onClose={() => {
            setShared(null);
            focusScan();
          }}
        />
      )}
    </div>
  );
}
