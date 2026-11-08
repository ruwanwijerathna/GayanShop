import type { ReactNode } from "react";

/** Back-office shell: top bar, collapsible sidebar navigation, content (doc §1.4 responsive table). */
export function BackOfficeLayout({
  topBar,
  nav,
  children,
}: {
  topBar: ReactNode;
  nav: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="flex h-screen min-h-[768px] min-w-[1024px] flex-col bg-background">
      {topBar}
      <div className="flex min-h-0 flex-1">
        <nav className="flex w-16 shrink-0 flex-col gap-2 border-r border-border bg-card p-2 xl:w-56">
          {nav}
        </nav>
        <main className="min-w-0 flex-1 overflow-auto p-4">{children}</main>
      </div>
    </div>
  );
}
