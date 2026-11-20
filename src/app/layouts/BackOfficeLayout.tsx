import type { CSSProperties, ReactNode } from "react";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";

/** Wide screens start with the sidebar open; the 1024 px baseline starts collapsed to icons (doc §1.4). */
const OPEN_FROM_PX = 1280;

/** 64 px icon rail so collapsed items keep 48 px touch targets (doc §1.9); 15 rem open. */
const SIDEBAR_SIZE = { "--sidebar-width": "15rem", "--sidebar-width-icon": "4rem" } as CSSProperties;

/** Back-office shell: the shadcn Sidebar (collapsible to icons) and content. */
export function BackOfficeLayout({ sidebar, children }: { sidebar: ReactNode; children: ReactNode }) {
  const defaultOpen = typeof window === "undefined" || window.innerWidth >= OPEN_FROM_PX;
  return (
    <SidebarProvider
      defaultOpen={defaultOpen}
      style={SIDEBAR_SIZE}
      className="h-screen w-full overflow-hidden bg-background"
    >
      {sidebar}
      <SidebarInset className="flex min-h-0 min-w-0 flex-col overflow-hidden">
        <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-auto p-4">{children}</div>
      </SidebarInset>
    </SidebarProvider>
  );
}
