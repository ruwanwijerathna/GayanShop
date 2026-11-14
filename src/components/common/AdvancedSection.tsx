import { ChevronDown } from "lucide-react";
import type { ReactNode } from "react";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";

/** A closed-by-default shadcn Collapsible for low-level fields: a 48 px trigger with a turning chevron. */
export function AdvancedSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Collapsible className="rounded-lg border bg-muted/30">
      <CollapsibleTrigger className="group flex min-h-12 w-full items-center justify-between gap-3 rounded-lg px-4 text-left text-base font-medium outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50">
        {title}
        <ChevronDown aria-hidden className="size-5 transition-transform group-data-[state=open]:rotate-180" />
      </CollapsibleTrigger>
      <CollapsibleContent className="flex flex-col gap-4 border-t p-4">{children}</CollapsibleContent>
    </Collapsible>
  );
}
