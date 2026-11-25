import type { ComponentProps } from "react";
import { TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";

/**
 * shadcn TabsList as a segmented control: a muted pill that scrolls sideways when the window is narrow.
 * The base list pins its height (`group-data-[orientation=horizontal]/tabs:h-9`, which outranks a plain
 * `h-auto`); around 48 px triggers that overflowed, and `overflow-x-auto` then grew a vertical scrollbar.
 */
export function PosTabsList({ className, ...props }: ComponentProps<typeof TabsList>) {
  return (
    <TabsList
      className={cn(
        "h-auto max-w-full justify-start gap-1 overflow-x-auto overflow-y-hidden rounded-xl p-1 group-data-[orientation=horizontal]/tabs:h-auto",
        className,
      )}
      {...props}
    />
  );
}

/** A 48 px segment; the selected one lifts onto the background with a soft shadow. */
export function PosTabsTrigger({ className, ...props }: ComponentProps<typeof TabsTrigger>) {
  return (
    <TabsTrigger
      className={cn(
        "h-12 flex-none rounded-lg px-4 text-sm font-medium text-muted-foreground [&_svg:not([class*='size-'])]:size-4.5",
        "data-[state=active]:bg-background data-[state=active]:text-foreground data-[state=active]:shadow-sm",
        className,
      )}
      {...props}
    />
  );
}
