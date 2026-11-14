import type { FormEventHandler, ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * One settings group, the same everywhere: a tinted icon, a title with a one-line description, an optional
 * status badge, the controls, and an optional footer bar that holds the actions (Save, Test). With
 * `onSubmit` the card is a `<form>`, so Enter saves and the footer's submit button works.
 */
export function SettingsCard({
  id,
  sectionId,
  icon,
  title,
  description,
  badge,
  footer,
  onSubmit,
  className,
  children,
}: {
  /** Id of the title; the card is labelled by it. */
  id: string;
  sectionId?: string;
  icon: ReactNode;
  title: string;
  description?: string;
  badge?: ReactNode;
  footer?: ReactNode;
  onSubmit?: FormEventHandler<HTMLFormElement>;
  className?: string;
  children: ReactNode;
}) {
  const inner = (
    <>
      <header className="flex items-start gap-3 border-b px-5 py-4">
        <span
          aria-hidden
          className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary [&_svg]:size-5"
        >
          {icon}
        </span>
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <h2 id={id} className="text-lg font-semibold tracking-tight">
            {title}
          </h2>
          {description && <p className="text-sm text-muted-foreground">{description}</p>}
        </div>
        {badge}
      </header>
      <div className="flex flex-col gap-5 p-5">{children}</div>
      {footer && (
        <footer className="flex flex-wrap items-center gap-3 border-t bg-muted/30 px-5 py-4">{footer}</footer>
      )}
    </>
  );
  const classes = cn("flex flex-col overflow-hidden rounded-xl border bg-card shadow-xs", className);
  return onSubmit ? (
    <form id={sectionId} onSubmit={onSubmit} aria-labelledby={id} className={classes}>
      {inner}
    </form>
  ) : (
    <section id={sectionId} aria-labelledby={id} className={classes}>
      {inner}
    </section>
  );
}

/** A status pill beside a card title. */
export function StatusPill({
  tone,
  children,
}: {
  tone: "ok" | "warn" | "bad" | "idle";
  children: ReactNode;
}) {
  return (
    <span
      className={cn(
        "inline-flex h-7 shrink-0 items-center gap-1.5 rounded-full px-3 text-xs font-medium",
        tone === "ok" && "bg-success-surface text-success-foreground",
        tone === "warn" && "bg-warning text-warning-foreground",
        tone === "bad" && "bg-destructive/10 text-destructive",
        tone === "idle" && "bg-muted text-muted-foreground",
      )}
    >
      <span aria-hidden className="size-1.5 rounded-full bg-current" />
      {children}
    </span>
  );
}
