// Settings → Labels (doc §7.4): everything on one card, nothing folded away. The label printer (picked from
// the Windows printers; its TSPL/ZPL language and dpi are detected from the name) and its dpi, the sticker
// sizes side by side, and the chosen size's calibration (gaps and margins) in one row with its save / reset /
// alignment-test buttons; the sample sticker beside it. Darkness, speed and the printer-wide shifts are not
// shown: a saved profile keeps its values, a new one starts at darkness 8, speed 4 and no shift.
import { Tag } from "lucide-react";
import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Can } from "@/components/auth/Can";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { LabelStickerPreview } from "@/components/labels/LabelSticker";
import { printSampleLabel } from "@/components/labels/stickerPrint";
import type {
  LabelLanguage,
  LabelPrinterProfile,
  LabelTemplate,
  TemplateCalibration,
  TransportConfig,
} from "@/db/bindings";
import { CommandError } from "@/db/client";
import {
  useCalibrateLabelTemplate,
  useLabelAlignmentTest,
  useLabelPrinterProfile,
  useLabelSamplePreview,
  useLabelTemplates,
  useSetLabelPrinterProfile,
} from "@/db/hooks";
import { translateError } from "@/i18n";
import { SettingsCard } from "@/components/common/SettingsCard";
import { PrinterSelect } from "@/components/common/PrinterSelect";
import { RadioCards } from "@/components/common/RadioCards";
import { SelectField } from "@/components/common/SelectField";
import { lastTemplate, rememberTemplate } from "@/components/labels/templatePreference";

const FIELD = "h-12 text-base";

type Kind = TransportConfig["kind"];

/** "1.5" / "-0.5" → mm; anything else → null. */
export function parseMm(text: string): number | null {
  const s = text.trim();
  const m = /^(-?)(\d{1,2})(?:\.(\d{1,2}))?$/.exec(s);
  if (!m) return null;
  const hundredths =
    Number.parseInt(m[2] ?? "0", 10) * 100 + Number.parseInt((m[3] ?? "").padEnd(2, "0"), 10);
  return ((m[1] === "-" ? -1 : 1) * hundredths) / 100;
}

export interface LabelPrinterForm {
  kind: Kind;
  target: string;
  port: string;
  baud: number;
  language: LabelLanguage;
  dpi: number;
  darkness: string;
  speed: string;
  offsetX: string;
  offsetY: string;
  autoPrint: boolean;
  /** The sticker size automatic printing uses (the roll picked under "Sticker size"). */
  defaultTemplate: string | null;
}

export function toForm(p: LabelPrinterProfile | null): LabelPrinterForm {
  const t = p?.transport;
  return {
    kind: t?.kind ?? "spooler",
    target:
      t?.kind === "spooler"
        ? t.printer_name
        : t?.kind === "network"
          ? t.host
          : t?.kind === "serial"
            ? t.port
            : "",
    port: t?.kind === "network" ? String(t.port) : "9100",
    baud: t?.kind === "serial" ? t.baud : 9600,
    language: p?.language ?? "tspl",
    dpi: p?.dpi ?? 203,
    darkness: String(p?.darkness ?? 8),
    speed: String(p?.speed ?? 4),
    offsetX: String(p?.offset_x_mm ?? 0),
    offsetY: String(p?.offset_y_mm ?? 0),
    autoPrint: p?.auto_print_on_receive ?? false,
    defaultTemplate: p?.default_template_id ?? null,
  };
}

/** The profile, or null when a number doesn't parse (Rust re-validates the ranges). */
export function toProfile(f: LabelPrinterForm): LabelPrinterProfile | null {
  const int = (s: string) => (/^\d{1,5}$/.test(s.trim()) ? Number.parseInt(s.trim(), 10) : null);
  const darkness = int(f.darkness);
  const speed = int(f.speed);
  const port = int(f.port);
  const offsetX = parseMm(f.offsetX);
  const offsetY = parseMm(f.offsetY);
  if (darkness === null || speed === null || offsetX === null || offsetY === null) return null;
  const transport: TransportConfig | null =
    f.kind === "spooler"
      ? { kind: "spooler", printer_name: f.target.trim() }
      : f.kind === "network"
        ? port === null
          ? null
          : { kind: "network", host: f.target.trim(), port }
        : { kind: "serial", port: f.target.trim().toUpperCase(), baud: f.baud };
  if (!transport) return null;
  return {
    transport,
    language: f.language,
    dpi: f.dpi,
    darkness,
    speed,
    offset_x_mm: offsetX,
    offset_y_mm: offsetY,
    auto_print_on_receive: f.autoPrint,
    default_template_id: f.defaultTemplate,
  };
}

export function LabelSettings() {
  const profile = useLabelPrinterProfile();
  const templates = useLabelTemplates();
  if (profile.data === undefined || !templates.data || templates.data.length === 0) return null;
  return <LabelCard profile={profile.data} templates={templates.data} />;
}

function Field({ id, label, children }: { id: string; label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor={id}>{label}</Label>
      {children}
    </div>
  );
}

/** A to-scale row of the roll's labels, so the two sizes are told apart at a glance. */
function RollVisual({ template }: { template: LabelTemplate }) {
  return (
    <span aria-hidden className="mb-1 flex h-9 items-center gap-1.5">
      {Array.from({ length: template.columns }, (_, i) => (
        <span
          key={i}
          className="rounded-sm border-2 border-foreground/60 bg-muted"
          style={{ width: `${template.label_w_mm * 1.4}px`, height: `${template.label_h_mm * 1.4}px` }}
        />
      ))}
    </span>
  );
}

function LabelCard({
  profile,
  templates,
}: {
  profile: LabelPrinterProfile | null;
  templates: LabelTemplate[];
}) {
  const { t, i18n } = useTranslation("labels");
  const save = useSetLabelPrinterProfile();
  const calibrate = useCalibrateLabelTemplate();
  const align = useLabelAlignmentTest();
  const [sampling, setSampling] = useState(false);
  const [sampleError, setSampleError] = useState<CommandError | null>(null);
  const [form, setForm] = useState(() => toForm(profile));
  const [invalid, setInvalid] = useState(false);
  const [chosen, setChosen] = useState(() => profile?.default_template_id ?? lastTemplate());
  const selected = templates.find((tpl) => tpl.id === chosen) ?? templates[0];
  const set = <K extends keyof LabelPrinterForm>(k: K, v: LabelPrinterForm[K]) =>
    setForm((f) => ({ ...f, [k]: v }));

  // The chosen size's calibration as typed. Edits belong to one size and its stored values, so switching
  // sizes, saving or resetting shows the stored values again.
  const calKey = selected ? calibrationKey(selected) : "";
  const [calEdit, setCalEdit] = useState<(CalibrationForm & { key: string }) | null>(null);
  const cal: CalibrationForm =
    calEdit && calEdit.key === calKey ? calEdit : selected ? calibrationForm(selected) : EMPTY_CALIBRATION;
  const setCal = (k: keyof CalibrationForm, v: string) => setCalEdit({ ...cal, [k]: v, key: calKey });

  const choose = (id: string) => {
    setChosen(id);
    rememberTemplate(id);
  };

  /** One Save: the printer profile, then the chosen size's calibration if it was changed. */
  const savePrinter = () => {
    // The roll shown under "Sticker size" is saved as this printer's default, for automatic printing.
    const p = toProfile({ ...form, defaultTemplate: selected?.id ?? null });
    const calibration = selected ? toCalibration(cal, selected) : null;
    const calOk = !selected || calibration !== null;
    setInvalid(p === null || !calOk);
    if (!p || !calOk) return;
    const changed = selected && calibration && !sameCalibration(calibration, selected);
    void (async () => {
      try {
        await save.mutateAsync(p);
        if (changed && selected && calibration) await calibrate.mutateAsync({ id: selected.id, calibration });
        toast.success(t("settings.saved"));
      } catch {
        // The refusal is shown on the card (save.error / calibrate.error).
      }
    })();
  };

  const resetRoll = () => {
    if (!selected) return;
    calibrate.mutate(
      { id: selected.id, calibration: null },
      { onSuccess: () => toast.success(t("settings.calibrationReset")) },
    );
  };

  const samplePreview = useLabelSamplePreview(selected?.id ?? null);
  const error = [save.error, calibrate.error, align.error, sampleError].find(
    (e) => e instanceof CommandError,
  );

  const printSample = (templateId: string) => {
    setSampling(true);
    setSampleError(null);
    printSampleLabel(templateId).then(
      () => {
        setSampling(false);
        toast.success(t("settings.testQueued"));
      },
      (e: unknown) => {
        setSampling(false);
        if (e instanceof CommandError) setSampleError(e);
        else toast.error(t("print.failed"));
      },
    );
  };

  return (
    <SettingsCard
      id="label-printer-title"
      icon={<Tag />}
      title={t("settings.printerTitle")}
      description={t("settings.printerHint")}
      footer={
        <Can perm="SETTINGS_MANAGE">
          <Button
            type="button"
            size="pos"
            disabled={save.isPending || calibrate.isPending}
            onClick={savePrinter}
          >
            {t("settings.save")}
          </Button>
          <Button
            type="button"
            size="pos"
            variant="outline"
            disabled={!selected || sampling}
            onClick={() => selected && printSample(selected.id)}
          >
            {t("settings.testLabel")}
          </Button>
          <Button
            type="button"
            size="pos"
            variant="outline"
            disabled={!selected || align.isPending}
            onClick={() =>
              selected &&
              align.mutate(selected.id, { onSuccess: () => toast.success(t("settings.alignQueued")) })
            }
          >
            {t("settings.alignTest")}
          </Button>
          <Button
            type="button"
            size="pos"
            variant="outline"
            disabled={!selected || calibrate.isPending}
            onClick={resetRoll}
          >
            {t("settings.reset")}
          </Button>
        </Can>
      }
    >
      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,22rem)]">
        <div className="flex min-w-0 flex-col gap-5">
          {form.kind !== "spooler" && (
            <p className="rounded-lg border bg-muted/40 p-3 text-base">
              {t("settings.otherConnection", { kind: t(`settings.kinds.${form.kind}`) })}
            </p>
          )}
          <Field id="lp-target" label={t("settings.printerName")}>
            <PrinterSelect
              id="lp-target"
              value={form.kind === "spooler" ? form.target : ""}
              onChange={(v) => {
                const lower = v.toLowerCase();
                const detectedDpi = lower.includes("300") ? 300 : lower.includes("203") ? 203 : undefined;
                const detectedLang =
                  lower.includes("zpl") || lower.includes("zebra")
                    ? "zpl"
                    : lower.includes("tspl") || lower.includes("tsc") || lower.includes("xprinter")
                      ? "tspl"
                      : undefined;
                // Picking a Windows printer always connects through the spooler.
                setForm((f) => ({
                  ...f,
                  kind: "spooler",
                  target: v,
                  ...(detectedDpi !== undefined ? { dpi: detectedDpi } : {}),
                  ...(detectedLang !== undefined ? { language: detectedLang } : {}),
                }));
              }}
            />
          </Field>

          <Field id="lp-dpi" label={t("settings.dpi")}>
            <SelectField
              id="lp-dpi"
              value={String(form.dpi)}
              onValueChange={(v) => set("dpi", v === "300" ? 300 : 203)}
              options={[
                { value: "203", label: "203 dpi" },
                { value: "300", label: "300 dpi" },
              ]}
            />
            <p className="text-sm text-muted-foreground">{t("settings.dpiHint")}</p>
          </Field>

          {selected && (
            <div className="flex flex-col gap-2">
              <p className="text-base font-medium">{t("settings.stickerSize")}</p>
              <RadioCards
                className="grid-cols-1 gap-3 sm:grid-cols-3"
                name="sticker-size"
                aria-label={t("settings.stickerSize")}
                value={selected.id}
                onValueChange={choose}
                options={templates.map((tpl) => ({
                  value: tpl.id,
                  visual: <RollVisual template={tpl} />,
                  label: t(`templates.${tpl.id}`, { defaultValue: tpl.name }),
                }))}
              />
              <p className="text-sm text-muted-foreground">{t("settings.stickerHint")}</p>
            </div>
          )}

          {selected && <RollCalibration template={selected} values={cal} onChange={setCal} />}

          <div className="flex items-center justify-between gap-6 rounded-lg border p-4">
            <div className="flex flex-col gap-1">
              <span id="label-auto-label" className="text-base font-medium">
                {t("settings.autoPrint")}
              </span>
              <span id="label-auto-hint" className="text-sm text-muted-foreground">
                {t("settings.autoPrintHint")}
              </span>
            </div>
            <Switch
              checked={form.autoPrint}
              aria-labelledby="label-auto-label"
              aria-describedby="label-auto-hint"
              onCheckedChange={(v) => set("autoPrint", v)}
            />
          </div>

          {invalid && (
            <p role="alert" className="text-base text-destructive">
              {t("settings.invalid")}
            </p>
          )}
          {error instanceof CommandError && (
            <p role="alert" className="text-base text-destructive">
              {translateError(i18n.t, error.error)}
            </p>
          )}
        </div>
        <aside aria-label={t("settings.previewTitle")} className="flex flex-col gap-2 self-start">
          {samplePreview.data?.stickers[0] ? (
            <LabelStickerPreview
              layout={samplePreview.data}
              sticker={samplePreview.data.stickers[0]}
              width={320}
              title={t("settings.previewTitle")}
              className="rounded-md border border-border bg-white"
            />
          ) : (
            <div className="aspect-[2/1] rounded-md border border-dashed bg-muted/40" aria-hidden />
          )}
          <p className="text-center text-sm text-muted-foreground">{t("settings.previewTitle")}</p>
        </aside>
      </div>
    </SettingsCard>
  );
}

/** The calibration fields as typed (text, so a half-typed "1." is allowed). */
interface CalibrationForm {
  colGap: string;
  topMargin: string;
  margin: string;
}

const EMPTY_CALIBRATION: CalibrationForm = { colGap: "", topMargin: "", margin: "" };

function calibrationKey(t: LabelTemplate): string {
  return `${t.id}:${t.col_gap_mm}:${t.top_margin_mm}:${t.left_margin_mm}`;
}

function calibrationForm(t: LabelTemplate): CalibrationForm {
  return {
    colGap: String(t.col_gap_mm),
    topMargin: String(t.top_margin_mm),
    margin: String(t.left_margin_mm),
  };
}

/** The calibration to save, or null when a field doesn't parse. The row gap isn't edited here: it keeps the
 * template's own value. */
function toCalibration(f: CalibrationForm, template: LabelTemplate): TemplateCalibration | null {
  const colGap = parseMm(f.colGap);
  const top = parseMm(f.topMargin);
  const margin = parseMm(f.margin);
  if (colGap === null || top === null || margin === null || colGap < 0 || top < 0 || margin < 0) return null;
  return { col_gap_mm: colGap, row_gap_mm: template.row_gap_mm, top_margin_mm: top, left_margin_mm: margin };
}

function sameCalibration(c: TemplateCalibration, t: LabelTemplate): boolean {
  return (
    c.col_gap_mm === t.col_gap_mm &&
    c.top_margin_mm === t.top_margin_mm &&
    c.left_margin_mm === t.left_margin_mm
  );
}

/** The chosen sticker size's gaps and margins in one row; the card's Save saves them with the printer. */
function RollCalibration({
  template,
  values,
  onChange,
}: {
  template: LabelTemplate;
  values: CalibrationForm;
  onChange: (key: keyof CalibrationForm, value: string) => void;
}) {
  const { t } = useTranslation("labels");
  const name = t(`templates.${template.id}`, { defaultValue: template.name });
  return (
    <div role="group" aria-label={name} className="flex flex-col gap-4 rounded-lg border p-4">
      <div className="flex flex-col gap-1">
        <p className="text-base font-medium">{t("settings.rollCalibration", { name })}</p>
        <p className="text-sm text-muted-foreground">
          {t("settings.templateHint", { dpi: template.dpi, cols: template.columns })}
        </p>
      </div>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        {(["colGap", "topMargin", "margin"] as const).map((key) => (
          <Field key={key} id={`${template.id}-${key}`} label={t(`settings.${key}`)}>
            <Input
              id={`${template.id}-${key}`}
              className={FIELD}
              inputMode="decimal"
              value={values[key]}
              onChange={(e) => onChange(key, e.target.value)}
            />
          </Field>
        ))}
      </div>
    </div>
  );
}
