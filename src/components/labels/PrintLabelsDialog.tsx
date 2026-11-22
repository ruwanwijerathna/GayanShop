// "Print labels" (doc §7.4), from the product list or a received batch: choose a template, adjust copies,
// see exactly what will print (the Rust layout as SVG, with warnings), then queue it for the label printer.
// `LABEL_PRINT` is approvable, so a cashier can print with a supervisor's PIN.
import { Minus, Plus, Printer } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ApprovalDialog } from "@/components/auth/ApprovalDialog";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import type { LabelJob } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useLabelPreview, useLabelTemplates, usePrintLabels } from "@/db/hooks";
import { translateError } from "@/i18n";
import { cn } from "@/lib/utils";
import { LabelPreviewSvg } from "./LabelPreviewSvg";
import { lastTemplate, rememberTemplate } from "./templatePreference";
import { Checkbox } from "@/components/ui/checkbox";

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
  const print = usePrintLabels();
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
    print.mutate(
      { job, approvalId },
      {
        onSuccess: () => {
          toast.success(t("print.queued", { count: total }));
          onClose();
        },
        onError: (e) => {
          if (e instanceof CommandError && e.needsApproval) setApproving(true);
        },
      },
    );
  };

  const error =
    (print.error instanceof CommandError &&
      !print.error.needsApproval &&
      translateError(i18n.t, print.error.error)) ||
    (one.error instanceof CommandError && translateError(i18n.t, one.error.error)) ||
    null;

  const setCount = (id: string, v: string) => setCopies((c) => ({ ...c, [id]: v }));
  const invalid = (id: string) => (parsed.find((p) => p.product_id === id)?.copies ?? 0) < 1;
  const previewName = names.get(previewId) ?? "";

  return (
    <>
      <Dialog open={!approving} onOpenChange={(o) => !o && onClose()}>
        <DialogContent className="max-h-[92vh] max-w-3xl gap-5 overflow-y-auto">
          <DialogHeader>
            <DialogTitle className="text-xl">{t("print.title")}</DialogTitle>
          </DialogHeader>

          <div className="grid gap-6 md:grid-cols-[1fr_minmax(0,18rem)]">
            <div className="flex flex-col gap-5">
              <section className="flex flex-col gap-3" aria-label={t("print.copies")}>
                <h3 className="text-sm font-medium text-muted-foreground">{t("print.copies")}</h3>
                {many ? (
                  <ul className="flex flex-col gap-2">
                    {items.map((i) => (
                      <li
                        key={i.product_id}
                        className={cn(
                          "flex items-center gap-3 rounded-lg border p-2 pl-3",
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
                ) : (
                  items.map((i) => (
                    <div key={i.product_id} className="flex flex-col gap-3">
                      <p className="text-lg font-semibold">{i.name}</p>
                      <CopiesStepper
                        large
                        name={i.name}
                        value={copies[i.product_id] ?? ""}
                        invalid={invalid(i.product_id)}
                        onChange={(v) => setCount(i.product_id, v)}
                      />
                    </div>
                  ))
                )}
              </section>

              <section className="flex flex-col gap-3">
                <h3 id="label-roll" className="text-sm font-medium text-muted-foreground">
                  {t("print.template")}
                </h3>
                <div role="radiogroup" aria-labelledby="label-roll" className="grid grid-cols-2 gap-2">
                  {(templates.data ?? []).map((tpl) => {
                    const on = tpl.id === templateId;
                    return (
                      <button
                        key={tpl.id}
                        type="button"
                        role="radio"
                        aria-checked={on}
                        className={cn(
                          "min-h-14 rounded-lg border px-3 py-2 text-left text-base transition-colors",
                          on ? "border-primary bg-primary/10 font-semibold" : "border-border hover:bg-accent",
                        )}
                        onClick={() => setTemplateId(tpl.id)}
                      >
                        {t(`templates.${tpl.id}`, { defaultValue: tpl.name })}
                      </button>
                    );
                  })}
                </div>
                {columns > 1 && (
                  <label className="flex items-start gap-3 text-base">
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
            </div>

            <aside className="flex flex-col gap-3">
              <div className="flex flex-col items-center gap-3 rounded-xl bg-muted p-4">
                {one.data ? (
                  <LabelPreviewSvg
                    single
                    preview={one.data}
                    title={t("print.preview")}
                    className="w-full rounded-md bg-white text-black shadow-md"
                  />
                ) : (
                  <div className="h-28 w-full animate-pulse rounded-md bg-background/60 motion-reduce:animate-none" />
                )}
                <p className="text-center text-sm text-muted-foreground">
                  {one.data
                    ? t("print.previewNote", {
                        w: one.data.label_w_mm,
                        h: one.data.label_h_mm,
                      })
                    : t("print.previewLoading")}
                  {many && previewName && (
                    <span className="block font-medium text-foreground">{previewName}</span>
                  )}
                </p>
              </div>
              {warnings.length > 0 && (
                <ul className="flex flex-col gap-1 text-sm" aria-label={t("print.warnings")}>
                  {warnings.map((w) => (
                    <li
                      key={`${w.code}:${w.product_id}`}
                      role="status"
                      className="rounded-md bg-warning/15 px-3 py-2"
                    >
                      {t(`warnings.${w.code}`, { name: names.get(w.product_id) ?? "" })}
                    </li>
                  ))}
                </ul>
              )}
            </aside>
          </div>

          {error && (
            <p role="alert" className="text-base text-destructive">
              {error}
            </p>
          )}
          <DialogFooter className="items-center gap-3 sm:justify-between">
            <p className="text-sm text-muted-foreground">
              {columns > 1 ? t("print.summary", { count: total, rows }) : t("print.count", { count: total })}
            </p>
            <div className="flex gap-2">
              <Button type="button" size="pos" variant="outline" onClick={onClose}>
                {t("common:actions.cancel")}
              </Button>
              <Button
                type="button"
                size="pos-lg"
                disabled={!job || print.isPending}
                onClick={() => send(null)}
              >
                <Printer aria-hidden />
                {t("print.submit", { count: total })}
              </Button>
            </div>
          </DialogFooter>
        </DialogContent>
      </Dialog>
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
