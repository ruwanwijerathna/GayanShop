import type { ComponentProps } from "react";
import { TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";

/** shadcn TabsList laid out for the counter: wrapping row of 48 px tabs with 8 px gaps (doc §1.9). */
export function PosTabsList({ className, ...props }: ComponentProps<typeof TabsList>) {
  return (
    <TabsList
      className={cn("h-auto flex-wrap justify-start gap-2 bg-transparent p-0", className)}
      {...props}
    />
  );
}

/** A 48 px tab; the selected one uses the primary (emerald) fill, the rest an outline. */
export function PosTabsTrigger({ className, ...props }: ComponentProps<typeof TabsTrigger>) {
  return (
    <TabsTrigger
      className={cn(
        "h-12 flex-none border-input px-4 text-base text-foreground [&_svg:not([class*='size-'])]:size-5",
        "data-[state=active]:border-primary data-[state=active]:bg-primary data-[state=active]:text-primary-foreground",
        "dark:data-[state=active]:border-primary dark:data-[state=active]:bg-primary dark:data-[state=active]:text-primary-foreground",
        className,
      )}
      {...props}
    />
  );
}
