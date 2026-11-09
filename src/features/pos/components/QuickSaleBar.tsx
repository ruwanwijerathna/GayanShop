import { Zap } from "lucide-react";
import { useState, type KeyboardEvent, type Ref } from "react";
import { useTranslation } from "react-i18next";
import { Kbd } from "@/components/common/Kbd";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { ProductSummary_Serialize as ProductSummary } from "@/db/bindings";
import { useQuickSaleHint } from "@/db/hooks";
import { formatMoney, parseMajor } from "@/lib/money";
import { cn } from "@/lib/utils";
import { parseQuickSale } from "../lib/parseQuickSale";
import type { QuickSaleLine } from "../store/cart.store";

export interface QuickSaleBarProps {
  value: string;
  onChange: (value: string) => void;
  onAdd: (line: QuickSaleLine) => void;
  /** ↓ then Enter on the "now a product" hint adds the real product instead. */
  onAddProduct?: (product: ProductSummary) => void;
  /** Esc, or after adding: focus goes back to the scan field. */
  onDone: () => void;
  inputRef?: Ref<HTMLInputElement>;
}

/**
 * F2 → type → Enter (doc §3.2). Plain Enter always commits what was typed; Tab opens an optional cost.
 * The live preview says what will be added, or why Enter is disabled ("price missing"). When the typed name
 * has been promoted to a product, a hint says so; picking it takes ↓ then Enter, so speed is never hijacked.
 */
export function QuickSaleBar({ value, onChange, onAdd, onAddProduct, onDone, inputRef }: QuickSaleBarProps) {
  const { t } = useTranslation("pos");
  const [withCost, setWithCost] = useState(false);
  const [cost, setCost] = useState("");
  const [pickHint, setPickHint] = useState(false);
  const parsed = parseQuickSale(value);
  const hint = useQuickSaleHint(onAddProduct ? (parsed.ok ? parsed.name : value.trim()) || null : null);
  const promoted = hint.data?.product ?? null;
  const costCents = cost.trim() === "" ? null : parseMajor(cost);
  const costValid = cost.trim() === "" || costCents !== null;

  const add = () => {
    if (!parsed.ok || !costValid) return;
    onAdd({ name: parsed.name, qty: parsed.qty, unitPrice: parsed.unitPrice, unitCost: costCents });
    onChange("");
    setCost("");
    setWithCost(false);
    onDone();
  };

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" && promoted) {
      e.preventDefault();
      setPickHint(true);
      return;
    }
    if (e.key === "Enter" && pickHint && promoted && onAddProduct) {
      e.preventDefault();
      onAddProduct(promoted);
      onChange("");
      setPickHint(false);
      onDone();
      return;
    }
    if (e.key !== "Enter") setPickHint(false);
    if (e.key === "Enter") {
      e.preventDefault();
      add();
    } else if (e.key === "Tab" && !e.shiftKey && !withCost && value.trim() !== "") {
      e.preventDefault();
      setWithCost(true);
    } else if (e.key === "Escape") {
      onChange("");
      setWithCost(false);
      onDone();
    }
  };

  const preview = parsed.ok
    ? t("quick.preview", { name: parsed.name, price: formatMoney(parsed.unitPrice), qty: parsed.qty })
    : value.trim() === ""
      ? t("quick.more")
      : t(`quick.reasons.${parsed.reason}`);

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-1">
      <label htmlFor="quick-sale" className="flex items-center gap-2 text-sm font-medium">
        <Zap aria-hidden className="size-4" />
        {t("quick.label")}
        <Kbd>F2</Kbd>
      </label>
      <div className="flex gap-2">
        <Input
          id="quick-sale"
          ref={inputRef}
          className="h-12 text-base"
          autoComplete="off"
          placeholder={t("quick.placeholder")}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={onKey}
          aria-describedby="quick-sale-preview"
        />
        {withCost && (
          <Input
            aria-label={t("quick.cost")}
            placeholder={t("quick.cost")}
            className="h-12 w-40 text-base"
            inputMode="decimal"
            value={cost}
            onChange={(e) => setCost(e.target.value)}
            onKeyDown={onKey}
            aria-invalid={!costValid}
          />
        )}
        <Button type="button" size="pos" onClick={add} disabled={!parsed.ok || !costValid}>
          {t("quick.add")}
        </Button>
      </div>
      <p
        id="quick-sale-preview"
        className={cn("min-h-5 text-sm", parsed.ok ? "text-foreground" : "text-muted-foreground")}
      >
        {preview}
      </p>
      {promoted && (
        <p
          role="status"
          className={cn(
            "rounded-md px-2 py-1 text-sm",
            pickHint ? "bg-primary text-primary-foreground" : "bg-muted",
          )}
        >
          {t("quick.promoted", { name: promoted.name, price: formatMoney(promoted.price) })}
        </p>
      )}
    </div>
  );
}
