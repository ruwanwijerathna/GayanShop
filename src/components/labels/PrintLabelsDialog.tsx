// "Print labels" (doc §7.4), from the product list or a received batch: choose a template, adjust copies,
// see exactly what will print (the Rust layout as SVG, with warnings), then queue it for the label printer.
// `LABEL_PRINT` is approvable, so a cashier can print with a supervisor's PIN.
import { Printer } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ApprovalDialog } from "@/components/auth/ApprovalDialog";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { LabelJob } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useLabelPreview, useLabelTemplates, usePrintLabels } from "@/db/hooks";
import { translateError } from "@/i18n";
import { LabelPreviewSvg } from "./LabelPreviewSvg";
import { lastTemplate, rememberTemplate } from "./templatePreference";
import { SelectField } from "@/components/common/SelectField";
import { Checkbox } from "@/components/ui/checkbox";

export interface LabelItem {
  product_id: string;
  name: string;
  copies: number;
}

/** Must match `commands::labels`'s approval target. */
export const LABELS_TARGET = "labels";

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

  const parsed = items.map((i) => {
    const s = (copies[i.product_id] ?? "").trim();
    const n = /^\d{1,3}$/.test(s) ? Number.parseInt(s, 10) : 0;
    return { product_id: i.product_id, copies: n };
  });
  const valid = parsed.every((p) => p.copies >= 1);
  const job: LabelJob | null = valid ? { template_id: templateId, items: parsed, fill_last_row: fill } : null;
  const preview = useLabelPreview(job);
  const total = parsed.reduce((a, p) => a + p.copies, 0);
  const names = new Map(items.map((i) => [i.product_id, i.name]));

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
    (preview.error instanceof CommandError && translateError(i18n.t, preview.error.error)) ||
    null;

  return (
    <>
      <Dialog open={!approving} onOpenChange={(o) => !o && onClose()}>
        <DialogContent className="max-h-[90vh] max-w-3xl overflow-y-auto">
          <DialogHeader>
            <DialogTitle>{t("print.title")}</DialogTitle>
            <DialogDescription>{t("print.count", { count: total })}</DialogDescription>
          </DialogHeader>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="flex flex-col gap-2">
              <Label htmlFor="label-template">{t("print.template")}</Label>
              <SelectField
                id="label-template"
                value={templateId}
                onValueChange={setTemplateId}
                options={(templates.data ?? []).map((tpl) => ({
                  value: tpl.id,
                  label: t(`templates.${tpl.id}`, { defaultValue: tpl.name }),
                }))}
              />
            </div>
            <label className="flex min-h-12 items-center gap-3 self-end text-base">
              <Checkbox className="size-6" checked={fill} onCheckedChange={(v) => setFill(v === true)} />
              {t("print.fill")}
            </label>
          </div>
          <ul className="flex flex-col gap-2">
            {items.map((i) => (
              <li key={i.product_id} className="flex items-center gap-3">
                <span className="flex-1 truncate">{i.name}</span>
                <Label htmlFor={`copies-${i.product_id}`} className="sr-only">
                  {t("print.copiesFor", { name: i.name })}
                </Label>
                <Input
                  id={`copies-${i.product_id}`}
                  inputMode="numeric"
                  className="h-12 w-24 text-right font-mono text-base"
                  value={copies[i.product_id] ?? ""}
                  onChange={(e) => setCopies((c) => ({ ...c, [i.product_id]: e.target.value }))}
                  aria-invalid={(parsed.find((p) => p.product_id === i.product_id)?.copies ?? 0) < 1}
                />
              </li>
            ))}
          </ul>
          {preview.data && (
            <>
              <LabelPreviewSvg preview={preview.data} title={t("print.preview")} />
              {preview.data.warnings.length > 0 && (
                <ul className="flex flex-col gap-1 text-sm" aria-label={t("print.warnings")}>
                  {preview.data.warnings.map((w) => (
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
            </>
          )}
          {error && (
            <p role="alert" className="text-base text-destructive">
              {error}
            </p>
          )}
          <DialogFooter className="mt-2">
            <Button type="button" size="pos" disabled={!job || print.isPending} onClick={() => send(null)}>
              <Printer aria-hidden />
              {t("print.submit", { count: total })}
            </Button>
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
