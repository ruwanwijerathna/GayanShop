import type { ReactElement } from "react";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";

/**
 * A tooltip for an icon-only button (hover or keyboard focus). The button keeps its own `aria-label`; the
 * tooltip repeats it visually for sighted mouse users and never replaces it. Carries its own provider, so it
 * works anywhere (the app root also has one).
 */
export function WithTooltip({ label, children }: { label: string; children: ReactElement }) {
  return (
    <TooltipProvider delayDuration={300}>
      <Tooltip>
        <TooltipTrigger asChild>{children}</TooltipTrigger>
        <TooltipContent>{label}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
