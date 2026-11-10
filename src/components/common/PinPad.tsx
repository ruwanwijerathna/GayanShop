import { Delete } from "lucide-react";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { useTouchMode } from "@/db/hooks";
import { cn } from "@/lib/utils";

export interface PinPadProps {
  value: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
  maxLength?: number;
  disabled?: boolean;
  label: string;
  /** Show the on-screen digit keys. Defaults to this counter's touch screen mode. */
  keypad?: boolean;
  className?: string;
}

const KEYS = ["1", "2", "3", "4", "5", "6", "7", "8", "9"] as const;

/**
 * A masked, numeric PIN input that the physical keyboard / numpad always drives; Enter submits. In touch
 * screen mode a pad of 64 px keys shows underneath (doc §1.9 dual-mode input).
 */
export function PinPad({
  value,
  onChange,
  onSubmit,
  maxLength = 8,
  disabled = false,
  label,
  keypad,
  className,
}: PinPadProps) {
  const { t } = useTranslation("auth");
  const touchMode = useTouchMode();
  const showKeys = keypad ?? touchMode;
  const input = useRef<HTMLInputElement>(null);
  // Focus the PIN field on mount so a keyboard/numpad works immediately.
  useEffect(() => input.current?.focus(), []);
  const press = (d: string) => {
    if (value.length < maxLength) onChange(value + d);
  };
  return (
    <form
      className={cn("flex w-72 flex-col gap-3", className)}
      onSubmit={(e) => {
        e.preventDefault();
        if (value.length > 0 && !disabled) onSubmit();
      }}
    >
      <label className="flex flex-col gap-1 text-sm font-medium">
        {label}
        <input
          type="password"
          inputMode="numeric"
          autoComplete="off"
          ref={input}
          value={value}
          maxLength={maxLength}
          disabled={disabled}
          onChange={(e) => onChange(e.target.value.replace(/\D/g, "").slice(0, maxLength))}
          className="h-14 rounded-md border border-input bg-background px-4 text-center font-mono text-2xl tracking-[0.5em]"
        />
      </label>
      {showKeys && (
        <div className="grid grid-cols-3 gap-2">
          {KEYS.map((d) => (
            <Button
              key={d}
              type="button"
              size="pos-lg"
              variant="outline"
              disabled={disabled}
              aria-label={t("pinpad.digit", { digit: d })}
              onClick={() => press(d)}
            >
              {d}
            </Button>
          ))}
          <Button
            type="button"
            size="pos-lg"
            variant="secondary"
            disabled={disabled}
            onClick={() => onChange("")}
          >
            {t("pinpad.clear")}
          </Button>
          <Button
            type="button"
            size="pos-lg"
            variant="outline"
            disabled={disabled}
            aria-label={t("pinpad.digit", { digit: "0" })}
            onClick={() => press("0")}
          >
            0
          </Button>
          <Button
            type="button"
            size="pos-lg"
            variant="secondary"
            disabled={disabled}
            aria-label={t("pinpad.backspace")}
            onClick={() => onChange(value.slice(0, -1))}
          >
            <Delete aria-hidden />
          </Button>
        </div>
      )}
    </form>
  );
}
