import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface RadioCardOption<T extends string> {
  value: T;
  label: ReactNode;
  description?: ReactNode;
  /** A small picture above the label (for example a sticker layout). */
  visual?: ReactNode;
}

/**
 * A radio group drawn as big selectable cards (native radios underneath, so arrows, Space and screen
 * readers work). Each card is at least 48 px tall; the chosen one gets the primary border and tint.
 */
export function RadioCards<T extends string>({
  name,
  value,
  onValueChange,
  options,
  disabled,
  className,
  "aria-label": ariaLabel,
}: {
  name: string;
  value: T;
  onValueChange: (value: T) => void;
  options: readonly RadioCardOption<T>[];
  disabled?: boolean;
  className?: string;
  "aria-label": string;
}) {
  return (
    <div role="radiogroup" aria-label={ariaLabel} className={cn("grid gap-3 sm:grid-cols-2", className)}>
      {options.map((o) => (
        <label
          key={o.value}
          className={cn(
            "relative flex min-h-12 cursor-pointer flex-col gap-1 rounded-lg border-2 bg-background p-4 transition-colors",
            "has-[input:checked]:border-primary has-[input:checked]:bg-primary/10",
            "has-[input:focus-visible]:ring-[3px] has-[input:focus-visible]:ring-ring/50",
            disabled && "cursor-not-allowed opacity-50",
          )}
        >
          <input
            type="radio"
            name={name}
            value={o.value}
            checked={value === o.value}
            disabled={disabled}
            onChange={() => onValueChange(o.value)}
            className="sr-only top-0 left-0"
          />
          {o.visual}
          <span className="text-base font-semibold">{o.label}</span>
          {o.description && <span className="text-sm text-muted-foreground">{o.description}</span>}
        </label>
      ))}
    </div>
  );
}
