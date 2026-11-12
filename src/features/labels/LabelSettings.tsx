// Settings → Labels (doc §7.4): this counter's label printer (connection, TSPL/ZPL, dpi, darkness, speed,
// calibration offsets) and each roll template's gaps and margin, with the alignment test that makes
// calibration a one-minute job.
import { Ruler, Tag } from "lucide-react";
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Can } from "@/components/auth/Can";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { LabelLanguage, LabelPrinterProfile, LabelTemplate, TransportConfig } from "@/db/bindings";
import { CommandError } from "@/db/client";
import {
  useCalibrateLabelTemplate,
  useLabelAlignmentTest,
  useLabelPrinterProfile,
  useLabelTemplates,
  useSetLabelPrinterProfile,
} from "@/db/hooks";
import { translateError } from "@/i18n";
import { SelectField } from "@/components/common/SelectField";

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
  };
}

export function LabelSettings() {
  const profile = useLabelPrinterProfile();
  const templates = useLabelTemplates();
  if (profile.data === undefined || !templates.data) return null;
  return (
    <>
      <PrinterCard profile={profile.data} />
      {templates.data.map((tpl) => (
        <TemplateCard
          key={`${tpl.id}:${tpl.col_gap_mm}:${tpl.row_gap_mm}:${tpl.left_margin_mm}`}
          template={tpl}
        />
      ))}
    </>
  );
}

function PrinterCard({ profile }: { profile: LabelPrinterProfile | null }) {
  const { t, i18n } = useTranslation("labels");
  const save = useSetLabelPrinterProfile();
  const [form, setForm] = useState(() => toForm(profile));
  const [invalid, setInvalid] = useState(false);
  const set = <K extends keyof LabelPrinterForm>(k: K, v: LabelPrinterForm[K]) =>
    setForm((f) => ({ ...f, [k]: v }));

  const submit = (e: FormEvent) => {
    e.preventDefault();
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

  return (
    <form
      onSubmit={submit}
      className="flex flex-col gap-4 rounded-lg border bg-card p-5"
      aria-labelledby="label-printer-title"
    >
      <h2 id="label-printer-title" className="flex items-center gap-3 text-xl font-semibold">
        <Tag aria-hidden className="size-6" />
        {t("settings.printerTitle")}
      </h2>
      <div className="grid gap-4 sm:grid-cols-2">
        <div className="flex flex-col gap-2">
          <Label htmlFor="lp-kind">{t("settings.connection")}</Label>
          <SelectField
            id="lp-kind"
            value={form.kind}
            onValueChange={(v) => set("kind", KINDS.find((k) => k === v) ?? "spooler")}
            options={KINDS.map((k) => ({ value: k, label: t(`settings.kinds.${k}`) }))}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="lp-target">{targetLabel}</Label>
          <Input
            id="lp-target"
            className={FIELD}
            value={form.target}
            onChange={(e) => set("target", e.target.value)}
          />
        </div>
        {form.kind === "network" && (
          <div className="flex flex-col gap-2">
            <Label htmlFor="lp-port">{t("settings.port")}</Label>
            <Input
              id="lp-port"
              className={FIELD}
              inputMode="numeric"
              value={form.port}
              onChange={(e) => set("port", e.target.value)}
            />
          </div>
        )}
        {form.kind === "serial" && (
          <div className="flex flex-col gap-2">
            <Label htmlFor="lp-baud">{t("settings.baud")}</Label>
            <SelectField
              id="lp-baud"
              value={String(form.baud)}
              onValueChange={(v) => set("baud", BAUDS.find((b) => String(b) === v) ?? 9600)}
              options={BAUDS.map((b) => ({ value: String(b), label: String(b) }))}
            />
          </div>
        )}
        <div className="flex flex-col gap-2">
          <Label htmlFor="lp-language">{t("settings.language")}</Label>
          <SelectField
            id="lp-language"
            value={form.language}
            onValueChange={(v) => set("language", v === "zpl" ? "zpl" : "tspl")}
            options={[
              { value: "tspl", label: t("settings.languages.tspl") },
              { value: "zpl", label: t("settings.languages.zpl") },
            ]}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="lp-dpi">{t("settings.dpi")}</Label>
          <SelectField
            id="lp-dpi"
            value={String(form.dpi)}
            onValueChange={(v) => set("dpi", v === "300" ? 300 : 203)}
            options={[
              { value: "203", label: "203 dpi" },
              { value: "300", label: "300 dpi" },
            ]}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="lp-darkness">{t("settings.darkness")}</Label>
          <Input
            id="lp-darkness"
            className={FIELD}
            inputMode="numeric"
            value={form.darkness}
            onChange={(e) => set("darkness", e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="lp-speed">{t("settings.speed")}</Label>
          <Input
            id="lp-speed"
            className={FIELD}
            inputMode="numeric"
            value={form.speed}
            onChange={(e) => set("speed", e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="lp-ox">{t("settings.offsetX")}</Label>
          <Input
            id="lp-ox"
            className={FIELD}
            inputMode="decimal"
            value={form.offsetX}
            onChange={(e) => set("offsetX", e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="lp-oy">{t("settings.offsetY")}</Label>
          <Input
            id="lp-oy"
            className={FIELD}
            inputMode="decimal"
            value={form.offsetY}
            onChange={(e) => set("offsetY", e.target.value)}
          />
        </div>
      </div>
      {invalid && (
        <p role="alert" className="text-base text-destructive">
          {t("settings.invalid")}
        </p>
      )}
      {save.error instanceof CommandError && (
        <p role="alert" className="text-base text-destructive">
          {translateError(i18n.t, save.error.error)}
        </p>
      )}
      <Can perm="SETTINGS_MANAGE">
        <Button type="submit" size="pos" className="self-start" disabled={save.isPending}>
          {t("settings.save")}
        </Button>
      </Can>
    </form>
  );
}

function TemplateCard({ template }: { template: LabelTemplate }) {
  const { t, i18n } = useTranslation("labels");
  const calibrate = useCalibrateLabelTemplate();
  const align = useLabelAlignmentTest();
  const [colGap, setColGap] = useState(String(template.col_gap_mm));
  const [rowGap, setRowGap] = useState(String(template.row_gap_mm));
  const [margin, setMargin] = useState(String(template.left_margin_mm));
  const values = [parseMm(colGap), parseMm(rowGap), parseMm(margin)];
  const ok = values.every((v) => v !== null && v >= 0);
  const name = t(`templates.${template.id}`, { defaultValue: template.name });
  const error = [calibrate.error, align.error].find((e) => e instanceof CommandError);

  const save = (e: FormEvent) => {
    e.preventDefault();
    const [c, r, m] = values;
    if (c === null || r === null || m === null || c === undefined || r === undefined || m === undefined)
      return;
    calibrate.mutate({ id: template.id, calibration: { col_gap_mm: c, row_gap_mm: r, left_margin_mm: m } });
  };

  return (
    <form onSubmit={save} className="flex flex-col gap-4 rounded-lg border bg-card p-5" aria-label={name}>
      <h2 className="flex items-center gap-3 text-xl font-semibold">
        <Ruler aria-hidden className="size-6" />
        {name}
      </h2>
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
          <div key={key} className="flex flex-col gap-2">
            <Label htmlFor={`${template.id}-${key}`}>{t(`settings.${key}`)}</Label>
            <Input
              id={`${template.id}-${key}`}
              className={FIELD}
              inputMode="decimal"
              value={value}
              onChange={(e) => setter(e.target.value)}
            />
          </div>
        ))}
      </div>
      {error instanceof CommandError && (
        <p role="alert" className="text-base text-destructive">
          {translateError(i18n.t, error.error)}
        </p>
      )}
      <Can perm="SETTINGS_MANAGE">
        <div className="flex flex-wrap gap-2">
          <Button type="submit" size="pos" disabled={!ok || calibrate.isPending}>
            {t("settings.saveCalibration")}
          </Button>
          <Button
            type="button"
            size="pos"
            variant="outline"
            onClick={() =>
              align.mutate(template.id, { onSuccess: () => toast.success(t("settings.alignQueued")) })
            }
          >
            {t("settings.alignmentTest")}
          </Button>
          <Button
            type="button"
            size="pos"
            variant="outline"
            onClick={() => calibrate.mutate({ id: template.id, calibration: null })}
          >
            {t("settings.reset")}
          </Button>
        </div>
      </Can>
    </form>
  );
}
