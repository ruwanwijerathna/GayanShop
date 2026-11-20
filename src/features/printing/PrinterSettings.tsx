import { ImagePlus, Printer, Trash2, ZoomIn, ZoomOut } from "lucide-react";
import { useEffect, useRef, useState, type ChangeEvent, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Can } from "@/components/auth/Can";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type {
  CodePage,
  CutMode,
  DrawerPin,
  PaperWidth,
  ReceiptPrinterProfile,
  TransportConfig,
} from "@/db/bindings";
import { CommandError } from "@/db/client";
import {
  usePrinterProfile,
  usePrinterStatus,
  usePrintTestPage,
  useReceiptHeader,
  useSetPrinterProfile,
  useSetReceiptLogo,
} from "@/db/hooks";
import { canvasFromLogo, logoFromFileWithSource, renderScaledLogo } from "@/lib/logoBitmap";
import { usePrintReceiptStore } from "@/components/receipt/PrintableReceiptPortal";
import { ReceiptPreview } from "./ReceiptPreview";
import { translateError } from "@/i18n";
import { AdvancedSection } from "@/components/common/AdvancedSection";
import { SettingsCard, StatusPill } from "@/components/common/SettingsCard";
import { PrinterSelect } from "@/components/common/PrinterSelect";
import { RadioCards } from "@/components/common/RadioCards";
import { SelectField } from "@/components/common/SelectField";
import { Checkbox } from "@/components/ui/checkbox";
import { Switch } from "@/components/ui/switch";

const FIELD = "h-12 text-base";

type Kind = TransportConfig["kind"];
const KINDS: Kind[] = ["spooler", "network", "serial"];
const WIDTHS: PaperWidth[] = ["mm80", "mm58"];
const CODEPAGES: CodePage[] = ["pc437", "wpc1252"];
const CUTS: CutMode[] = ["partial", "full", "none"];
const PINS: DrawerPin[] = ["pin2", "pin5"];
const BAUDS = [9600, 19200, 38400, 57600, 115200, 4800];

export interface PrinterForm {
  kind: Kind;
  printerName: string;
  host: string;
  port: string;
  comPort: string;
  baud: number;
  width: PaperWidth;
  codepage: CodePage;
  cut: CutMode;
  feedLines: string;
  qr: boolean;
  drawerPin: DrawerPin;
  openDrawer: boolean;
}

export const DEFAULT_FORM: PrinterForm = {
  kind: "spooler",
  printerName: "",
  host: "",
  port: "9100",
  comPort: "COM1",
  baud: 9600,
  width: "mm80",
  codepage: "pc437",
  cut: "partial",
  feedLines: "4",
  qr: true,
  drawerPin: "pin2",
  openDrawer: true,
};

export function toForm(p: ReceiptPrinterProfile | null): PrinterForm {
  if (!p) return DEFAULT_FORM;
  const t = p.transport;
  return {
    kind: t.kind,
    printerName: t.kind === "spooler" ? t.printer_name : "",
    host: t.kind === "network" ? t.host : "",
    port: t.kind === "network" ? String(t.port) : "9100",
    comPort: t.kind === "serial" ? t.port : "COM1",
    baud: t.kind === "serial" ? t.baud : 9600,
    width: p.width,
    codepage: p.codepage,
    cut: p.cut,
    feedLines: String(p.feed_lines),
    qr: p.qr,
    drawerPin: p.drawer_pin,
    openDrawer: p.open_drawer ?? true,
  };
}

/** Integers only (port, feed lines); anything else becomes -1 and Rust rejects it with a message. */
function int(s: string): number {
  return /^\d{1,5}$/.test(s.trim()) ? Number.parseInt(s.trim(), 10) : -1;
}

export function toProfile(f: PrinterForm): ReceiptPrinterProfile {
  const transport: TransportConfig =
    f.kind === "spooler"
      ? { kind: "spooler", printer_name: f.printerName.trim() }
      : f.kind === "network"
        ? { kind: "network", host: f.host.trim(), port: Math.max(0, int(f.port)) }
        : { kind: "serial", port: f.comPort.trim().toUpperCase(), baud: f.baud };
  return {
    transport,
    width: f.width,
    codepage: f.codepage,
    cut: f.cut,
    feed_lines: Math.min(255, Math.max(0, int(f.feedLines))),
    qr: f.qr,
    drawer_pin: f.drawerPin,
    open_drawer: f.openDrawer,
  };
}

/** Settings → Printer (doc §7.2): this counter's receipt printer and drawer, plus the test page. */
export function PrinterSettings() {
  const profile = usePrinterProfile();
  return profile.data !== undefined ? <PrinterCard profile={profile.data} /> : null;
}

function PrinterCard({ profile }: { profile: ReceiptPrinterProfile | null }) {
  const { t, i18n } = useTranslation("printing");
  const status = usePrinterStatus();
  const save = useSetPrinterProfile();
  const test = usePrintTestPage();
  const header = useReceiptHeader();
  const setLogo = useSetReceiptLogo();
  const fileInput = useRef<HTMLInputElement>(null);
  const [form, setForm] = useState<PrinterForm>(() => toForm(profile));
  const [saved, setSaved] = useState(false);

  const sourceRef = useRef<CanvasImageSource | null>(null);
  const sourceDimRef = useRef<{ width: number; height: number } | null>(null);
  const zoomTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [scalePercent, setScalePercent] = useState<number>(() => {
    const stored = localStorage.getItem("receipt_logo_scale");
    return stored ? Math.min(100, Math.max(10, Number.parseInt(stored, 10))) : 100;
  });
  const [sliderVal, setSliderVal] = useState<number | null>(null);
  const activeZoom = sliderVal ?? scalePercent;

  useEffect(() => {
    return () => {
      if (zoomTimerRef.current) clearTimeout(zoomTimerRef.current);
    };
  }, []);

  useEffect(() => {
    if (!header.data?.logo || sourceRef.current) return;
    const dataUrl = localStorage.getItem("receipt_logo_source");
    if (dataUrl) {
      const img = new Image();
      img.onload = () => {
        sourceRef.current = img;
        sourceDimRef.current = { width: img.naturalWidth, height: img.naturalHeight };
      };
      img.src = dataUrl;
    } else {
      const canvas = canvasFromLogo(header.data.logo);
      sourceRef.current = canvas;
      sourceDimRef.current = { width: canvas.width, height: canvas.height };
    }
  }, [header.data?.logo]);

  const set = <K extends keyof PrinterForm>(key: K, value: PrinterForm[K]) => {
    setSaved(false);
    setForm((f) => ({ ...f, [key]: value }));
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    save.mutate(toProfile(form), { onSuccess: () => setSaved(true) });
  };

  const error = [save.error, test.error, setLogo.error].find((x) => x instanceof CommandError);
  const health = status.data?.health ?? "not_configured";

  const chooseImage = async (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    const res = await logoFromFileWithSource(file, 100);
    if (!res) {
      toast.error(t("settings.logoUnreadable"));
      return;
    }
    sourceRef.current = null;
    sourceDimRef.current = { width: res.width, height: res.height };
    setScalePercent(100);
    setSliderVal(null);
    try {
      if (res.dataUrl) localStorage.setItem("receipt_logo_source", res.dataUrl);
      localStorage.setItem("receipt_logo_scale", "100");
    } catch {
      /* ignore */
    }
    setLogo.mutate(res.logo, { onSuccess: () => toast.success(t("settings.logoSaved")) });
  };

  const removeImage = () => {
    sourceRef.current = null;
    sourceDimRef.current = null;
    try {
      localStorage.removeItem("receipt_logo_source");
      localStorage.removeItem("receipt_logo_scale");
    } catch {
      /* ignore */
    }
    setLogo.mutate(null, { onSuccess: () => toast.success(t("settings.logoRemoved")) });
  };

  const handleZoom = (percent: number, immediate = false) => {
    const target = Math.min(100, Math.max(10, Math.round(percent)));
    if (immediate) setSliderVal(null);
    setScalePercent(target);
    try {
      localStorage.setItem("receipt_logo_scale", String(target));
    } catch {
      /* ignore */
    }

    const source = sourceRef.current ?? (header.data?.logo ? canvasFromLogo(header.data.logo) : null);
    if (!source) return;
    const dim =
      sourceDimRef.current ??
      ("width" in source
        ? {
            width: Number.parseInt(String(source.width), 10),
            height: Number.parseInt(String(source.height), 10),
          }
        : null);
    if (!dim || dim.width <= 0) return;

    const newLogo = renderScaledLogo(source, dim.width, dim.height, target);
    if (!newLogo) return;

    if (zoomTimerRef.current) clearTimeout(zoomTimerRef.current);
    if (immediate) {
      setLogo.mutate(newLogo);
    } else {
      zoomTimerRef.current = setTimeout(() => {
        setLogo.mutate(newLogo);
      }, 150);
    }
  };

  const tone = health === "ready" ? "ok" : health === "not_configured" ? "idle" : "bad";

  return (
    <SettingsCard
      id="printer-title"
      icon={<Printer />}
      title={t("settings.printerTitle")}
      description={t("settings.printerHint")}
      badge={<StatusPill tone={tone}>{t(`settings.health.${health}`)}</StatusPill>}
      onSubmit={submit}
      footer={
        <Can perm="SETTINGS_MANAGE">
          <Button type="submit" size="pos" disabled={save.isPending}>
            {t("settings.save")}
          </Button>
          <Button
            type="button"
            size="pos"
            variant="outline"
            disabled={test.isPending || profile === null}
            onClick={() => test.mutate()}
          >
            {t("settings.testPage")}
          </Button>
          <Button
            type="button"
            size="pos"
            variant="outline"
            onClick={() => {
              usePrintReceiptStore.getState().print({
                order_id: "SAMPLE-RECEIPT",
                store_name: header.data?.store_name ?? "GayanShop",
                receipt_no: "T01-20261120-0001",
                status: "completed",
                business_date: new Date().toISOString().slice(0, 10),
                completed_at: new Date().toISOString(),
                cashier: "Cashier 01",
                customer_masked: null,
                lines: [
                  {
                    name: "Sample item A",
                    qty: 2,
                    list_price: 45000,
                    unit_price: 45000,
                    line_gross: 90000,
                    line_discount: 0,
                    line_total: 90000,
                  },
                  {
                    name: "Sample item B",
                    qty: 1,
                    list_price: 125000,
                    unit_price: 125000,
                    line_gross: 125000,
                    line_discount: 0,
                    line_total: 125000,
                  },
                  {
                    name: "Sample item C",
                    qty: 3,
                    list_price: 9500,
                    unit_price: 9500,
                    line_gross: 28500,
                    line_discount: 0,
                    line_total: 28500,
                  },
                ],
                item_count: 6,
                gross_sales: 243500,
                discount_total: 0,
                grand_total: 243500,
                payments: [{ method: "cash", amount: 243500, tendered: 300000, reference: null }],
                change_due: 56500,
                returns: [],
                exchange_credit: 0,
                footer: header.data?.footer ?? "",
              });
            }}
          >
            {t("settings.testPageSinhala", { defaultValue: "සිංහල ටෙස්ට් මුද්‍රණය (HTML)" })}
          </Button>
        </Can>
      }
    >
      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_auto]">
        <div className="flex min-w-0 flex-col gap-5">
          {form.kind === "spooler" ? (
            <div className="flex flex-col gap-2">
              <Label htmlFor="printer-name">{t("settings.printerName")}</Label>
              <PrinterSelect
                id="printer-name"
                value={form.printerName}
                onChange={(v) => set("printerName", v)}
              />
            </div>
          ) : (
            <p className="rounded-lg border bg-muted/40 p-3 text-base">
              {t("settings.otherConnection", { kind: t(`settings.kinds.${form.kind}`) })}
            </p>
          )}

          <RadioCards
            name="printer-width"
            aria-label={t("settings.paper")}
            value={form.width}
            onValueChange={(v) => set("width", v)}
            options={WIDTHS.map((w) => ({
              value: w,
              label: t(`settings.widths.${w}`),
              description: t(`settings.widthHints.${w}`),
            }))}
          />

          <div className="flex items-center justify-between gap-6 rounded-lg border p-4">
            <div className="flex flex-col gap-1">
              <span id="printer-drawer-label" className="text-base font-medium">
                {t("settings.drawer")}
              </span>
              <span id="printer-drawer-hint" className="text-sm text-muted-foreground">
                {t("settings.drawerHint")}
              </span>
            </div>
            <Switch
              checked={form.openDrawer}
              aria-labelledby="printer-drawer-label"
              aria-describedby="printer-drawer-hint"
              onCheckedChange={(v) => set("openDrawer", v)}
            />
          </div>

          <div className="flex flex-col gap-3 rounded-lg border p-4">
            <div className="flex flex-col gap-1">
              <span id="printer-logo-label" className="text-base font-medium">
                {t("settings.logo")}
              </span>
              <span className="text-sm text-muted-foreground">{t("settings.logoHint")}</span>
            </div>
            <Can perm="SETTINGS_MANAGE">
              <div className="flex flex-wrap items-center gap-3">
                <input
                  ref={fileInput}
                  type="file"
                  accept="image/*"
                  className="sr-only"
                  aria-labelledby="printer-logo-label"
                  tabIndex={-1}
                  onChange={(e) => void chooseImage(e)}
                />
                <Button
                  type="button"
                  variant="outline"
                  disabled={setLogo.isPending}
                  onClick={() => fileInput.current?.click()}
                >
                  <ImagePlus aria-hidden />
                  {header.data?.logo ? t("settings.logoReplace") : t("settings.logoChoose")}
                </Button>
                {header.data?.logo && (
                  <Button type="button" variant="outline" disabled={setLogo.isPending} onClick={removeImage}>
                    <Trash2 aria-hidden />
                    {t("settings.logoRemove")}
                  </Button>
                )}
              </div>

              {header.data?.logo && (
                <div className="mt-1 flex flex-col gap-2 rounded-lg border bg-muted/30 p-3">
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-sm font-medium">{t("settings.logoSize")}</span>
                    <span className="text-xs font-mono text-muted-foreground">
                      {header.data.logo.width} × {header.data.logo.height} dots ({activeZoom}%)
                    </span>
                  </div>
                  <div className="flex items-center gap-3">
                    <Button
                      type="button"
                      variant="outline"
                      size="icon"
                      className="size-8 shrink-0"
                      disabled={setLogo.isPending || activeZoom <= 10}
                      onClick={() => handleZoom(activeZoom - 5, true)}
                      title={t("settings.zoomOut")}
                      aria-label={t("settings.zoomOut")}
                    >
                      <ZoomOut className="size-4" />
                    </Button>
                    <input
                      type="range"
                      min={10}
                      max={100}
                      step={1}
                      value={activeZoom}
                      onChange={(e) => {
                        const v = Number.parseInt(e.target.value, 10);
                        setSliderVal(v);
                        handleZoom(v, false);
                      }}
                      onPointerUp={() => {
                        setSliderVal(null);
                        handleZoom(activeZoom, true);
                      }}
                      onKeyUp={() => {
                        setSliderVal(null);
                        handleZoom(activeZoom, true);
                      }}
                      className="h-2 flex-1 cursor-pointer accent-primary"
                      aria-label={t("settings.logoSize")}
                    />
                    <Button
                      type="button"
                      variant="outline"
                      size="icon"
                      className="size-8 shrink-0"
                      disabled={setLogo.isPending || activeZoom >= 100}
                      onClick={() => handleZoom(activeZoom + 5, true)}
                      title={t("settings.zoomIn")}
                      aria-label={t("settings.zoomIn")}
                    >
                      <ZoomIn className="size-4" />
                    </Button>
                  </div>
                </div>
              )}
            </Can>
          </div>

          <AdvancedSection title={t("settings.advanced")}>
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="flex flex-col gap-2 sm:col-span-2">
                <Label htmlFor="printer-kind">{t("settings.connection")}</Label>
                <SelectField
                  id="printer-kind"
                  value={form.kind}
                  onValueChange={(v) => set("kind", KINDS.find((k) => k === v) ?? "spooler")}
                  options={KINDS.map((k) => ({ value: k, label: t(`settings.kinds.${k}`) }))}
                />
              </div>
              {form.kind === "network" && (
                <>
                  <div className="flex flex-col gap-2">
                    <Label htmlFor="printer-host">{t("settings.host")}</Label>
                    <Input
                      id="printer-host"
                      className={FIELD}
                      value={form.host}
                      onChange={(e) => set("host", e.target.value)}
                    />
                  </div>
                  <div className="flex flex-col gap-2">
                    <Label htmlFor="printer-port">{t("settings.port")}</Label>
                    <Input
                      id="printer-port"
                      className={FIELD}
                      inputMode="numeric"
                      value={form.port}
                      onChange={(e) => set("port", e.target.value)}
                    />
                  </div>
                </>
              )}
              {form.kind === "serial" && (
                <>
                  <div className="flex flex-col gap-2">
                    <Label htmlFor="printer-com">{t("settings.comPort")}</Label>
                    <Input
                      id="printer-com"
                      className={FIELD}
                      value={form.comPort}
                      onChange={(e) => set("comPort", e.target.value)}
                    />
                  </div>
                  <div className="flex flex-col gap-2">
                    <Label htmlFor="printer-baud">{t("settings.baud")}</Label>
                    <SelectField
                      id="printer-baud"
                      value={String(form.baud)}
                      onValueChange={(v) => set("baud", BAUDS.find((b) => String(b) === v) ?? 9600)}
                      options={BAUDS.map((b) => ({ value: String(b), label: String(b) }))}
                    />
                  </div>
                </>
              )}
              <Choice
                id="printer-codepage"
                label={t("settings.codepage")}
                value={form.codepage}
                options={CODEPAGES}
                render={(c) => c.toUpperCase()}
                onChange={(v) => set("codepage", v)}
              />
              <Choice
                id="printer-cut"
                label={t("settings.cut")}
                value={form.cut}
                options={CUTS}
                render={(c) => t(`settings.cuts.${c}`)}
                onChange={(v) => set("cut", v)}
              />
              <div className="flex flex-col gap-2">
                <Label htmlFor="printer-feed">{t("settings.feed")}</Label>
                <Input
                  id="printer-feed"
                  className={FIELD}
                  inputMode="numeric"
                  value={form.feedLines}
                  onChange={(e) => set("feedLines", e.target.value)}
                />
              </div>
              <Choice
                id="printer-pin"
                label={t("settings.drawerPin")}
                value={form.drawerPin}
                options={PINS}
                render={(p) => t(`settings.pins.${p}`)}
                onChange={(v) => set("drawerPin", v)}
              />
              <label className="flex min-h-12 items-center gap-3 text-base sm:col-span-2">
                <Checkbox
                  className="size-6"
                  checked={form.qr}
                  onCheckedChange={(v) => set("qr", v === true)}
                />
                {t("settings.qr")}
              </label>
            </div>
          </AdvancedSection>

          {error instanceof CommandError && (
            <p role="alert" className="text-base text-destructive">
              {translateError(i18n.t, error.error)}
            </p>
          )}
          {saved && <p role="status">{t("settings.saved")}</p>}
          {test.isSuccess && <p role="status">{t("settings.testQueued")}</p>}
        </div>
        <aside aria-label={t("preview.title")} className="flex justify-center lg:justify-end">
          <ReceiptPreview header={header.data} width={form.width} />
        </aside>
      </div>
    </SettingsCard>
  );
}

function Choice<T extends string>(props: {
  id: string;
  label: string;
  value: T;
  options: readonly T[];
  render: (v: T) => string;
  onChange: (v: T) => void;
}) {
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor={props.id}>{props.label}</Label>
      <SelectField
        id={props.id}
        value={props.value}
        onValueChange={(v) => props.onChange(props.options.find((o) => o === v) ?? props.value)}
        options={props.options.map((o) => ({ value: o, label: props.render(o) }))}
      />
    </div>
  );
}
