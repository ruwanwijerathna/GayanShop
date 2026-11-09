import { Printer } from "lucide-react";
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
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
import { usePrinterProfile, usePrinterStatus, usePrintTestPage, useSetPrinterProfile } from "@/db/hooks";
import { translateError } from "@/i18n";

const FIELD = "h-12 text-base";
const SELECT =
  "h-12 w-full rounded-md border border-input bg-background px-3 text-base focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none";

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
  const [form, setForm] = useState<PrinterForm>(() => toForm(profile));
  const [saved, setSaved] = useState(false);

  const set = <K extends keyof PrinterForm>(key: K, value: PrinterForm[K]) => {
    setSaved(false);
    setForm((f) => ({ ...f, [key]: value }));
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    save.mutate(toProfile(form), { onSuccess: () => setSaved(true) });
  };

  const error = [save.error, test.error].find((x) => x instanceof CommandError);
  const health = status.data?.health ?? "not_configured";

  return (
    <form
      onSubmit={submit}
      className="flex flex-col gap-4 rounded-lg border bg-card p-5"
      aria-labelledby="printer-title"
    >
      <div className="flex items-center gap-3">
        <Printer aria-hidden className="size-6" />
        <h2 id="printer-title" className="flex-1 text-xl font-semibold">
          {t("settings.printerTitle")}
        </h2>
        <span className="text-base text-muted-foreground">
          {t("settings.status")}:{" "}
          <strong className="text-foreground">{t(`settings.health.${health}`)}</strong>
        </span>
      </div>

      <div className="grid gap-4 sm:grid-cols-2">
        <div className="flex flex-col gap-2 sm:col-span-2">
          <Label htmlFor="printer-kind">{t("settings.connection")}</Label>
          <select
            id="printer-kind"
            className={SELECT}
            value={form.kind}
            onChange={(e) => set("kind", KINDS.find((k) => k === e.target.value) ?? "spooler")}
          >
            {KINDS.map((k) => (
              <option key={k} value={k}>
                {t(`settings.kinds.${k}`)}
              </option>
            ))}
          </select>
        </div>
        {form.kind === "spooler" && (
          <div className="flex flex-col gap-2 sm:col-span-2">
            <Label htmlFor="printer-name">{t("settings.printerName")}</Label>
            <Input
              id="printer-name"
              className={FIELD}
              value={form.printerName}
              onChange={(e) => set("printerName", e.target.value)}
            />
          </div>
        )}
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
              <select
                id="printer-baud"
                className={SELECT}
                value={form.baud}
                onChange={(e) => set("baud", BAUDS.find((b) => String(b) === e.target.value) ?? 9600)}
              >
                {BAUDS.map((b) => (
                  <option key={b} value={b}>
                    {b}
                  </option>
                ))}
              </select>
            </div>
          </>
        )}
        <Choice
          id="printer-width"
          label={t("settings.paper")}
          value={form.width}
          options={WIDTHS}
          render={(w) => t(`settings.widths.${w}`)}
          onChange={(v) => set("width", v)}
        />
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
          <input
            type="checkbox"
            className="size-6"
            checked={form.qr}
            onChange={(e) => set("qr", e.target.checked)}
          />
          {t("settings.qr")}
        </label>
      </div>

      {error instanceof CommandError && (
        <p role="alert" className="text-base text-destructive">
          {translateError(i18n.t, error.error)}
        </p>
      )}
      {saved && <p role="status">{t("settings.saved")}</p>}
      {test.isSuccess && <p role="status">{t("settings.testQueued")}</p>}

      <Can perm="SETTINGS_MANAGE">
        <div className="flex flex-wrap gap-3">
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
        </div>
      </Can>
    </form>
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
      <select
        id={props.id}
        className={SELECT}
        value={props.value}
        onChange={(e) => props.onChange(props.options.find((o) => o === e.target.value) ?? props.value)}
      >
        {props.options.map((o) => (
          <option key={o} value={o}>
            {props.render(o)}
          </option>
        ))}
      </select>
    </div>
  );
}
