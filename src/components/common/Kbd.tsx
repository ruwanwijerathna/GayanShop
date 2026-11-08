import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";

/** Visible keyboard-shortcut badge shown on till buttons, e.g. `PAY [F9]` (doc §1.9). */
export function Kbd({ className, ...props }: ComponentProps<"kbd">) {
  return (
    <kbd
      className={cn(
        "inline-flex h-6 min-w-6 items-center justify-center rounded border border-input bg-muted px-1.5 font-mono text-sm text-muted-foreground",
        className,
      )}
      {...props}
    />
  );
}
