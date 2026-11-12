import type { ReactNode } from "react";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { cn } from "@/lib/utils";

export interface SelectOption {
  value: string;
  label: ReactNode;
}

export interface SelectFieldProps {
  /** Pairs with a `<Label htmlFor>`; the trigger takes it, so the label names the control. */
  id?: string;
  value: string;
  onValueChange: (value: string) => void;
  options: readonly SelectOption[];
  disabled?: boolean;
  className?: string;
  "aria-label"?: string;
  "aria-describedby"?: string;
}

/** Radix keeps "" for "no value", so an option whose value is "" (e.g. "All categories") travels as this. */
const EMPTY = "__empty__";
const toRadix = (v: string) => (v === "" ? EMPTY : v);
const fromRadix = (v: string) => (v === EMPTY ? "" : v);

/**
 * The app's dropdown: shadcn Select with a 48 px trigger and 48 px options (touch, doc §1.9), keyboard
 * operable (Enter/Space/arrows/typeahead). Values are strings, as with a native `<select>`, and "" is a
 * normal option value.
 */
export function SelectField({
  id,
  value,
  onValueChange,
  options,
  disabled,
  className,
  "aria-label": ariaLabel,
  "aria-describedby": describedBy,
}: SelectFieldProps) {
  return (
    <Select value={toRadix(value)} onValueChange={(v) => onValueChange(fromRadix(v))} disabled={disabled}>
      <SelectTrigger
        id={id}
        size="pos"
        aria-label={ariaLabel}
        aria-describedby={describedBy}
        className={cn("w-full bg-background", className)}
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent position="popper">
        {options.map((o) => (
          <SelectItem key={o.value} value={toRadix(o.value)} className="min-h-12 text-base">
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
