// Settings → Labels (doc §7.4): the label printer and the sticker size up front, with the alignment test;
// connection, TSPL/ZPL, dpi, darkness, speed, offsets and the chosen roll's gaps and margin under "Advanced
// Calibration".
import { Tag } from "lucide-react";
import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Can } from "@/components/auth/Can";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { LabelPreviewSvg } from "@/components/labels/LabelPreviewSvg";
import type { LabelLanguage, LabelPrinterProfile, LabelTemplate, TransportConfig } from "@/db/bindings";
import { CommandError } from "@/db/client";
import {
  useCalibrateLabelTemplate,
  useLabelAlignmentTest,
  useLabelPrinterProfile,
  useLabelSamplePreview,
  useLabelSamplePrint,
  useLabelTemplates,
  useSetLabelPrinterProfile,
} from "@/db/hooks";
import { translateError } from "@/i18n";
import { AdvancedSection } from "@/components/common/AdvancedSection";
import { SettingsCard } from "@/components/common/SettingsCard";
import { PrinterSelect } from "@/components/common/PrinterSelect";
import { RadioCards } from "@/components/common/RadioCards";
import { SelectField } from "@/components/common/SelectField";
import { lastTemplate, rememberTemplate } from "@/components/labels/templatePreference";

const FIELD = "h-12 text-base";

type Kind = TransportConfig["kind"];
const KINDS: Kind[] = ["spooler", "network", "serial"];
const BAUDS = [9600, 19200, 38400, 57600, 115200, 4800];

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
    <span aria-hidden className="mb-1 flex gap-1.5">
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
  const align = useLabelAlignmentTest();
  const sample = useLabelSamplePrint();
  const [form, setForm] = useState(() => toForm(profile));
  const [invalid, setInvalid] = useState(false);
  const [chosen, setChosen] = useState(lastTemplate);
  const selected = templates.find((tpl) => tpl.id === chosen) ?? templates[0];
  const set = <K extends keyof LabelPrinterForm>(k: K, v: LabelPrinterForm[K]) =>
    setForm((f) => ({ ...f, [k]: v }));

  const choose = (id: string) => {
    setChosen(id);
    rememberTemplate(id);
  };

  const savePrinter = () => {
    const p = toProfile(form);
    setInvalid(p === null);
    if (p) save.mutate(p, { onSuccess: () => toast.success(t("settings.saved")) });
  };

  const targetLabel =
    form.kind === "spooler"
      ? t("settings.printerName")
      : form.kind === "network"
        ? t("settings.host")
        : t("settings.comPort");
  const samplePreview = useLabelSamplePreview(selected?.id ?? null);
  const error = [save.error, align.error, sample.error].find((e) => e instanceof CommandError);

  return (
    <SettingsCard
      id="label-printer-title"
      icon={<Tag />}
      title={t("settings.printerTitle")}
      description={t("settings.printerHint")}
      footer={
        <Can perm="SETTINGS_MANAGE">
          <Button type="button" size="pos" disabled={save.isPending} onClick={savePrinter}>
            {t("settings.save")}
          </Button>
          <Button
            type="button"
            size="pos"
            variant="outline"
            disabled={!selected || sample.isPending}
            onClick={() =>
              selected &&
              sample.mutate(selected.id, { onSuccess: () => toast.success(t("settings.testQueued")) })
            }
          >
            {t("settings.testLabel")}
          </Button>
        </Can>
      }
    >
      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,22rem)]">
        <div className="flex min-w-0 flex-col gap-5">
          {form.kind === "spooler" ? (
            <Field id="lp-target" label={t("settings.printerName")}>
              <PrinterSelect
                id="lp-target"
                value={form.target}
                onChange={(v) => {
                  const lower = v.toLowerCase();
                  const detectedDpi = lower.includes("300") ? 300 : lower.includes("203") ? 203 : undefined;
                  const detectedLang =
                    lower.includes("zpl") || lower.includes("zebra")
                      ? "zpl"
                      : lower.includes("tspl") || lower.includes("tsc") || lower.includes("xprinter")
                        ? "tspl"
                        : undefined;
                  setForm((f) => ({
                    ...f,
                    target: v,
                    ...(detectedDpi !== undefined ? { dpi: detectedDpi } : {}),
                    ...(detectedLang !== undefined ? { language: detectedLang } : {}),
                  }));
                }}
              />
            </Field>
          ) : (
            <p className="rounded-lg border bg-muted/40 p-3 text-base">
              {t("settings.otherConnection", { kind: t(`settings.kinds.${form.kind}`) })}
            </p>
          )}

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

          <AdvancedSection title={t("settings.advanced")}>
            <div className="grid gap-4 sm:grid-cols-2">
              <Field id="lp-kind" label={t("settings.connection")}>
                <SelectField
                  id="lp-kind"
                  value={form.kind}
                  onValueChange={(v) => {
                    const kind = KINDS.find((k) => k === v) ?? "spooler";
                    setForm((f) => ({
                      ...f,
                      kind,
                      target: (kind === "spooler") === (f.kind === "spooler") ? f.target : "",
                    }));
                  }}
                  options={KINDS.map((k) => ({ value: k, label: t(`settings.kinds.${k}`) }))}
                />
              </Field>
              {form.kind !== "spooler" && (
                <Field id="lp-target" label={targetLabel}>
                  <Input
                    id="lp-target"
                    className={FIELD}
                    value={form.target}
                    onChange={(e) => set("target", e.target.value)}
                  />
                </Field>
              )}
              {form.kind === "network" && (
                <Field id="lp-port" label={t("settings.port")}>
                  <Input
                    id="lp-port"
                    className={FIELD}
                    inputMode="numeric"
                    value={form.port}
                    onChange={(e) => set("port", e.target.value)}
                  />
                </Field>
              )}
              {form.kind === "serial" && (
                <Field id="lp-baud" label={t("settings.baud")}>
                  <SelectField
                    id="lp-baud"
                    value={String(form.baud)}
                    onValueChange={(v) => set("baud", BAUDS.find((b) => String(b) === v) ?? 9600)}
                    options={BAUDS.map((b) => ({ value: String(b), label: String(b) }))}
                  />
                </Field>
              )}
              <Field id="lp-language" label={t("settings.language")}>
                <SelectField
                  id="lp-language"
                  value={form.language}
                  onValueChange={(v) => set("language", v === "zpl" ? "zpl" : "tspl")}
                  options={[
                    { value: "tspl", label: t("settings.languages.tspl") },
                    { value: "zpl", label: t("settings.languages.zpl") },
                  ]}
                />
              </Field>
              <Field id="lp-darkness" label={t("settings.darkness")}>
                <Input
                  id="lp-darkness"
                  className={FIELD}
                  inputMode="numeric"
                  value={form.darkness}
                  onChange={(e) => set("darkness", e.target.value)}
                />
              </Field>
              <Field id="lp-speed" label={t("settings.speed")}>
                <Input
                  id="lp-speed"
                  className={FIELD}
                  inputMode="numeric"
                  value={form.speed}
                  onChange={(e) => set("speed", e.target.value)}
                />
              </Field>
              <Field id="lp-ox" label={t("settings.offsetX")}>
                <Input
                  id="lp-ox"
                  className={FIELD}
                  inputMode="decimal"
                  value={form.offsetX}
                  onChange={(e) => set("offsetX", e.target.value)}
                />
              </Field>
              <Field id="lp-oy" label={t("settings.offsetY")}>
                <Input
                  id="lp-oy"
                  className={FIELD}
                  inputMode="decimal"
                  value={form.offsetY}
                  onChange={(e) => set("offsetY", e.target.value)}
                />
              </Field>
            </div>
            {selected && (
              <Can perm="SETTINGS_MANAGE">
                <div className="flex flex-col items-start gap-2">
                  <Button
                    type="button"
                    variant="outline"
                    disabled={align.isPending}
                    onClick={() =>
                      align.mutate(selected.id, { onSuccess: () => toast.success(t("settings.alignQueued")) })
                    }
                  >
                    {t("settings.alignTest")}
                  </Button>
                  <p className="text-sm text-muted-foreground">{t("settings.alignTestHint")}</p>
                </div>
              </Can>
            )}
            {selected && (
              <RollCalibration
                key={`${selected.id}:${selected.col_gap_mm}:${selected.row_gap_mm}:${selected.left_margin_mm}`}
                template={selected}
              />
            )}
          </AdvancedSection>
        </div>
        <aside aria-label={t("settings.previewTitle")} className="flex flex-col gap-2">
          {samplePreview.data ? (
            <LabelPreviewSvg preview={samplePreview.data} title={t("settings.previewTitle")} maxRows={1} />
          ) : (
            <div className="aspect-[2/1] rounded-md border border-dashed bg-muted/40" aria-hidden />
          )}
          <p className="text-center text-sm text-muted-foreground">{t("settings.previewTitle")}</p>
        </aside>
      </div>
    </SettingsCard>
  );
}

/** The chosen roll's gaps and left margin: save, or go back to the template's own values. */
function RollCalibration({ template }: { template: LabelTemplate }) {
  const { t, i18n } = useTranslation("labels");
  const calibrate = useCalibrateLabelTemplate();
  const [colGap, setColGap] = useState(String(template.col_gap_mm));
  const [rowGap, setRowGap] = useState(String(template.row_gap_mm));
  const [margin, setMargin] = useState(String(template.left_margin_mm));
  const values = [parseMm(colGap), parseMm(rowGap), parseMm(margin)];
  const ok = values.every((v) => v !== null && v >= 0);
  const name = t(`templates.${template.id}`, { defaultValue: template.name });

  const saveRoll = () => {
    const [c, r, m] = values;
    if (c === null || r === null || m === null || c === undefined || r === undefined || m === undefined)
      return;
    calibrate.mutate(
      { id: template.id, calibration: { col_gap_mm: c, row_gap_mm: r, left_margin_mm: m } },
      { onSuccess: () => toast.success(t("settings.calibrationSaved")) },
    );
  };

  return (
    <div role="group" aria-label={name} className="flex flex-col gap-4 border-t pt-4">
      <p className="text-base font-medium">{t("settings.rollCalibration", { name })}</p>
      <p className="text-sm text-muted-foreground">
        {t("settings.templateHint", { dpi: template.dpi, cols: template.columns })}
      </p>
      <div className="grid gap-4 sm:grid-cols-3">
        {(
          [
            ["colGap", colGap, setColGap],
            ["rowGap", rowGap, setRowGap],
            ["margin", margin, setMargin],
          ] as const
        ).map(([key, value, setter]) => (
          <Field key={key} id={`${template.id}-${key}`} label={t(`settings.${key}`)}>
            <Input
              id={`${template.id}-${key}`}
              className={FIELD}
              inputMode="decimal"
              value={value}
              onChange={(e) => setter(e.target.value)}
            />
          </Field>
        ))}
      </div>
      {calibrate.error instanceof CommandError && (
        <p role="alert" className="text-base text-destructive">
          {translateError(i18n.t, calibrate.error.error)}
        </p>
      )}
      <Can perm="SETTINGS_MANAGE">
        <div className="flex flex-wrap gap-3">
          <Button type="button" size="pos" disabled={!ok || calibrate.isPending} onClick={saveRoll}>
            {t("settings.saveCalibration")}
          </Button>
          <Button
            type="button"
            size="pos"
            variant="outline"
            disabled={calibrate.isPending}
            onClick={() =>
              calibrate.mutate(
                { id: template.id, calibration: null },
                { onSuccess: () => toast.success(t("settings.calibrationReset")) },
              )
            }
          >
            {t("settings.reset")}
          </Button>
        </div>
      </Can>
    </div>
  );
}
