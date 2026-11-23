// "Print labels" (doc §7.4), from the product list or a received batch: choose a template, adjust copies,
// see exactly what will print (the sticker drawn by the same HTML/CSS template that becomes the printer's
// picture, with warnings), then queue it for the label printer. `LABEL_PRINT` is approvable, so a cashier
// can print with a supervisor's PIN.
import { Minus, Plus, Printer } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ApprovalDialog } from "@/components/auth/ApprovalDialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import type { LabelJob } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useLabelPreview, useLabelTemplates } from "@/db/hooks";
import { translateError } from "@/i18n";
import { cn } from "@/lib/utils";
import { LabelStickerPreview } from "./LabelSticker";
import { printLabelJob } from "./stickerPrint";
import { lastTemplate, rememberTemplate } from "./templatePreference";

export interface LabelItem {
  product_id: string;
  name: string;
  copies: number;
}

/** Must match `commands::labels`'s approval target. */
export const LABELS_TARGET = "labels";

const MAX_COPIES = 999;

/** − [ n ] +: big touch targets; the number can also be typed. */
function CopiesStepper({
  name,
  value,
  invalid,
  large,
  onChange,
  onFocus,
}: {
  name: string;
  value: string;
  invalid: boolean;
  large?: boolean;
  onChange: (v: string) => void;
  onFocus?: () => void;
}) {
  const { t } = useTranslation("labels");
  const n = /^\d{1,3}$/.test(value.trim()) ? Number.parseInt(value.trim(), 10) : 0;
  return (
    <div className="flex items-center gap-2">
      <Button
        type="button"
        variant="outline"
        size="icon-pos"
        aria-label={t("print.fewer", { name })}
        disabled={n <= 1}
        onClick={() => onChange(String(Math.max(1, n - 1)))}
      >
        <Minus aria-hidden />
      </Button>
      <Input
        inputMode="numeric"
        aria-label={t("print.copiesFor", { name })}
        aria-invalid={invalid}
        className={cn("w-20 text-center font-mono", large ? "h-14 text-2xl" : "h-12 text-lg")}
        value={value}
        onFocus={onFocus}
        onChange={(e) => onChange(e.target.value)}
      />
      <Button
        type="button"
        variant="outline"
        size="icon-pos"
        aria-label={t("print.more", { name })}
        disabled={n >= MAX_COPIES}
        onClick={() => onChange(String(Math.min(MAX_COPIES, n + 1)))}
      >
        <Plus aria-hidden />
      </Button>
    </div>
  );
}

export function PrintLabelsDialog({ items, onClose }: { items: LabelItem[]; onClose: () => void }) {
  const { t, i18n } = useTranslation("labels");
  const templates = useLabelTemplates();
  const [printing, setPrinting] = useState(false);
  const [printError, setPrintError] = useState<CommandError | null>(null);
  const [templateId, setTemplateId] = useState(lastTemplate);
  const [copies, setCopies] = useState<Record<string, string>>(() =>
    Object.fromEntries(items.map((i) => [i.product_id, String(i.copies)])),
  );
  const [fill, setFill] = useState(false);
  const [approving, setApproving] = useState(false);
  const [previewId, setPreviewId] = useState(items[0]?.product_id ?? "");

  const parsed = items.map((i) => {
    const s = (copies[i.product_id] ?? "").trim();
    const n = /^\d{1,3}$/.test(s) ? Number.parseInt(s, 10) : 0;
    return { product_id: i.product_id, copies: n };
  });
  const valid = parsed.every((p) => p.copies >= 1);
  const job: LabelJob | null = valid ? { template_id: templateId, items: parsed, fill_last_row: fill } : null;
  const total = parsed.reduce((a, p) => a + p.copies, 0);
  const names = new Map(items.map((i) => [i.product_id, i.name]));
  const many = items.length > 1;
  const singleItem = items[0];

  // The preview is one label: every copy prints the same, so showing twenty adds nothing.
  const oneLabel: LabelJob = {
    template_id: templateId,
    items: [{ product_id: previewId, copies: 1 }],
    fill_last_row: false,
  };
  const one = useLabelPreview(previewId ? oneLabel : null);
  // With several products, the warnings must cover all of them, not just the one on show.
  const whole = useLabelPreview(many ? job : null);
  const warnings = (many ? whole.data : one.data)?.warnings ?? [];
  const template = templates.data?.find((x) => x.id === templateId);
  const columns = Math.max(1, template?.columns ?? 1);
  const rows = Math.ceil(total / columns);

  const send = (approvalId: string | null) => {
    if (!job) return;
    rememberTemplate(templateId);
    setPrinting(true);
    setPrintError(null);
    printLabelJob(job, approvalId).then(
      (count) => {
        setPrinting(false);
        toast.success(t("print.queued", { count }));
        onClose();
      },
      (e: unknown) => {
        setPrinting(false);
        if (e instanceof CommandError && e.needsApproval) setApproving(true);
        else if (e instanceof CommandError) setPrintError(e);
        else toast.error(t("print.failed"));
      },
    );
  };

  const error =
    (printError && translateError(i18n.t, printError.error)) ||
    (one.error instanceof CommandError && translateError(i18n.t, one.error.error)) ||
    null;
  const shown = one.data?.stickers[0];

  const setCount = (id: string, v: string) => setCopies((c) => ({ ...c, [id]: v }));
  const invalid = (id: string) => (parsed.find((p) => p.product_id === id)?.copies ?? 0) < 1;
  const previewName = names.get(previewId) ?? "";

  return (
    <>
      <Sheet open={!approving} onOpenChange={(o) => !o && onClose()}>
        <SheetContent
          showCloseButton={false}
          className="flex h-full w-full flex-col gap-0 overflow-y-auto p-0 sm:max-w-md"
        >
          <SheetHeader className="border-b bg-muted/30 px-6 py-5">
            <div className="flex items-center gap-3">
              <span
                aria-hidden
                className="flex size-11 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary ring-1 ring-primary/15"
              >
                <Printer className="size-5" />
              </span>
              <div className="min-w-0 flex-1">
                <SheetTitle className="text-xl tracking-tight">{t("print.title")}</SheetTitle>
                <SheetDescription className="truncate text-base font-medium text-foreground">
                  {!many && singleItem
                    ? singleItem.name
                    : t("print.itemsCount", {
                        count: items.length,
                        defaultValue: `${items.length} items selected`,
                      })}
                </SheetDescription>
              </div>
            </div>
          </SheetHeader>

          <div className="flex flex-1 flex-col gap-5 overflow-y-auto px-6 py-5">
            {/* Label preview placed cleanly at the top */}
            <div className="flex flex-col items-center gap-3 rounded-xl border bg-muted/30 p-4">
              {one.data && shown ? (
                <LabelStickerPreview
                  layout={one.data}
                  sticker={shown}
                  width={240}
                  title={t("print.preview")}
                  className="rounded-md bg-white shadow-md"
                />
              ) : (
                <div className="h-28 w-full animate-pulse rounded-md bg-background/60 motion-reduce:animate-none" />
              )}
              <div className="text-center text-sm text-muted-foreground">
                <p>
                  {one.data
                    ? t("print.previewNote", {
                        w: one.data.label_w_mm,
                        h: one.data.label_h_mm,
                      })
                    : t("print.previewLoading")}
                </p>
                {many && previewName && (
                  <span className="mt-1 block font-medium text-foreground">{previewName}</span>
                )}
              </div>
            </div>

            {warnings.length > 0 && (
              <ul className="flex flex-col gap-1.5 text-sm" aria-label={t("print.warnings")}>
                {warnings.map((w) => (
                  <li
                    key={`${w.code}:${w.product_id}`}
                    role="status"
                    className="rounded-md border border-warning/30 bg-warning/15 px-3 py-2 text-warning-foreground font-medium"
                  >
                    {t(`warnings.${w.code}`, { name: names.get(w.product_id) ?? "" })}
                  </li>
                ))}
              </ul>
            )}

            {/* Copies */}
            {many ? (
              <section className="flex flex-col gap-2" aria-label={t("print.copies")}>
                <h3 className="text-sm font-medium text-muted-foreground">{t("print.copies")}</h3>
                <ul className="flex flex-col gap-2">
                  {items.map((i) => (
                    <li
                      key={i.product_id}
                      className={cn(
                        "flex items-center gap-3 rounded-lg border p-2 pl-3 transition-colors",
                        previewId === i.product_id ? "border-primary bg-primary/5" : "border-border",
                      )}
                    >
                      <button
                        type="button"
                        aria-pressed={previewId === i.product_id}
                        className="min-h-12 flex-1 truncate text-left text-base font-medium"
                        onClick={() => setPreviewId(i.product_id)}
                      >
                        {i.name}
                      </button>
                      <CopiesStepper
                        name={i.name}
                        value={copies[i.product_id] ?? ""}
                        invalid={invalid(i.product_id)}
                        onChange={(v) => setCount(i.product_id, v)}
                        onFocus={() => setPreviewId(i.product_id)}
                      />
                    </li>
                  ))}
                </ul>
              </section>
            ) : (
              singleItem && (
                <section className="flex flex-col gap-2" aria-label={t("print.copies")}>
                  <div className="flex items-center justify-between rounded-lg border p-3">
                    <span className="text-base font-medium">{t("print.copies")}</span>
                    <CopiesStepper
                      large
                      name={singleItem.name}
                      value={copies[singleItem.product_id] ?? ""}
                      invalid={invalid(singleItem.product_id)}
                      onChange={(v) => setCount(singleItem.product_id, v)}
                    />
                  </div>
                </section>
              )
            )}

            {/* Label roll: 3 options vertically stacked one below the other */}
            <section className="flex flex-col gap-2.5">
              <h3 id="label-roll" className="text-sm font-medium text-muted-foreground">
                {t("print.template")}
              </h3>
              <div role="radiogroup" aria-labelledby="label-roll" className="flex flex-col gap-2">
                {(templates.data ?? []).map((tpl) => {
                  const on = tpl.id === templateId;
                  return (
                    <button
                      key={tpl.id}
                      type="button"
                      role="radio"
                      aria-checked={on}
                      className={cn(
                        "flex min-h-12 items-center justify-between rounded-lg border px-4 py-3 text-left text-base transition-colors",
                        on
                          ? "border-primary bg-primary/10 font-semibold text-foreground ring-1 ring-primary/30"
                          : "border-border hover:bg-accent text-muted-foreground hover:text-foreground",
                      )}
                      onClick={() => setTemplateId(tpl.id)}
                    >
                      <span>{t(`templates.${tpl.id}`, { defaultValue: tpl.name })}</span>
                      <span
                        className={cn(
                          "flex size-4 items-center justify-center rounded-full border border-primary",
                          on ? "bg-primary" : "bg-transparent",
                        )}
                      >
                        {on && <span className="size-1.5 rounded-full bg-primary-foreground" />}
                      </span>
                    </button>
                  );
                })}
              </div>
              {columns > 1 && (
                <label className="flex items-start gap-3 pt-1 text-base">
                  <Checkbox
                    className="mt-0.5 size-6"
                    checked={fill}
                    onCheckedChange={(v) => setFill(v === true)}
                  />
                  <span className="flex flex-col">
                    {t("print.fill")}
                    <span className="text-sm text-muted-foreground">{t("print.fillHint")}</span>
                  </span>
                </label>
              )}
            </section>

            {error && (
              <p role="alert" className="rounded-md bg-destructive/15 p-3 text-base text-destructive">
                {error}
              </p>
            )}
          </div>

          <SheetFooter className="mt-auto flex flex-col gap-3 border-t bg-muted/20 px-6 py-4">
            <div className="flex items-center justify-between rounded-lg border bg-background/80 px-4 py-2.5 text-sm shadow-xs">
              <span className="font-medium text-muted-foreground">{t("print.summaryLabel")}</span>
              <span className="font-semibold text-foreground">
                {columns > 1
                  ? t("print.summary", { count: total, rows })
                  : t("print.count", { count: total })}
              </span>
            </div>
            <div className="grid grid-cols-2 gap-3">
              <Button type="button" size="pos" variant="outline" onClick={onClose}>
                {t("common:actions.cancel")}
              </Button>
              <Button type="button" size="pos" disabled={!job || printing} onClick={() => send(null)}>
                <Printer aria-hidden />
                {t("print.submit", { count: total })}
              </Button>
            </div>
          </SheetFooter>
        </SheetContent>
      </Sheet>
      <ApprovalDialog
        open={approving}
        perm="LABEL_PRINT"
        targetRef={LABELS_TARGET}
        onApproved={(id) => {
          setApproving(false);
          send(id);
        }}
        onCancel={() => setApproving(false)}
      />
    </>
  );
}
