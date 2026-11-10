import { Phone, X } from "lucide-react";
import type { KeyboardEvent, Ref } from "react";
import { useTranslation } from "react-i18next";
import { Kbd } from "@/components/common/Kbd";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useCustomerLookup } from "@/db/hooks";
import { normalizeLk } from "@/lib/phone";
import { cartStore, useCart } from "../store/cart.store";
import { WithTooltip } from "@/components/common/WithTooltip";

/** `0771234567` → `077 123 4567` while typing (digits only, at most 10). */
export function formatTyped(raw: string): string {
  const d = raw.replace(/\D/g, "").slice(0, 10);
  if (d.length <= 3) return d;
  if (d.length <= 6) return `${d.slice(0, 3)} ${d.slice(3)}`;
  return `${d.slice(0, 3)} ${d.slice(3, 6)} ${d.slice(6)}`;
}

/**
 * F4 / Numpad `/` (doc §3.8). Optional and never a dialog: consent is verbal at the counter. After 9–10 digits
 * Rust looks the number up offline: a known customer shows their name and visit number, a new one gets an
 * optional name. Enter goes back to the scan field; Esc clears.
 */
export function CustomerPhoneField({
  inputRef,
  onDone,
}: {
  inputRef?: Ref<HTMLInputElement>;
  onDone: () => void;
}) {
  const { t } = useTranslation("pos");
  const phone = useCart((s) => s.customerPhone);
  const name = useCart((s) => s.customerName);
  const e164 = normalizeLk(phone);
  const lookup = useCustomerLookup(e164);
  const set = (p: string, n: string) => cartStore.getState().setCustomer(p, n);

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      onDone();
    } else if (e.key === "Escape") {
      e.preventDefault();
      set("", "");
      onDone();
    }
  };

  const found = lookup.data;
  const digits = phone.replace(/\D/g, "").length;
  let status: string | null = null;
  if (e164 && found?.customer_id) {
    status = found.display_name
      ? t("customer.known", { name: found.display_name, visit: found.visit_count + 1 })
      : t("customer.knownNoName", { visit: found.visit_count + 1 });
  } else if (e164 && found) {
    status = t("customer.new");
  } else if (digits >= 9 && !e164) {
    status = t("customer.invalid");
  }

  return (
    <div className="flex w-72 shrink-0 flex-col gap-1">
      <label htmlFor="customer-phone" className="flex items-center gap-2 text-sm font-medium">
        <Phone aria-hidden className="size-4" />
        {t("customer.label")}
        <Kbd>F4</Kbd>
      </label>
      <div className="flex gap-2">
        <Input
          id="customer-phone"
          ref={inputRef}
          className="h-12 font-mono text-base"
          inputMode="numeric"
          autoComplete="off"
          placeholder={t("customer.placeholder")}
          value={formatTyped(phone)}
          onChange={(e) => set(e.target.value.replace(/\D/g, "").slice(0, 10), name)}
          onKeyDown={onKey}
          aria-describedby="customer-status"
        />
        {phone !== "" && (
          <WithTooltip label={t("customer.clear")}>
            <Button
              type="button"
              variant="ghost"
              size="icon-pos"
              aria-label={t("customer.clear")}
              onClick={() => set("", "")}
            >
              <X aria-hidden />
            </Button>
          </WithTooltip>
        )}
      </div>
      <p id="customer-status" role="status" className="min-h-5 truncate text-sm text-muted-foreground">
        {status}
      </p>
      {e164 && found && !found.customer_id && (
        <Input
          aria-label={t("customer.name")}
          placeholder={t("customer.name")}
          className="h-12 text-base"
          maxLength={60}
          value={name}
          onChange={(e) => set(phone, e.target.value)}
          onKeyDown={onKey}
        />
      )}
    </div>
  );
}
