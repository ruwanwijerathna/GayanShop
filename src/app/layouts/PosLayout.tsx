import type { ReactNode } from "react";

/** Full-screen till shell for a 1024×768 baseline (doc §1.9): top bar, then the till regions. */
export function PosLayout({ topBar, children }: { topBar: ReactNode; children: ReactNode }) {
  return (
    <div className="flex h-screen min-h-[768px] min-w-[1024px] flex-col bg-background">
      {topBar}
      <main className="flex min-h-0 flex-1">{children}</main>
    </div>
  );
}
