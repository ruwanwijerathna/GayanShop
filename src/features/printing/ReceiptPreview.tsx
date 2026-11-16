// Settings → Receipt printer: a picture of the sample receipt (header image, shop name, a few made-up lines,
// totals, QR and the shop's own footer), at the paper width chosen in the form. Receipts print in English, so
// the sample text is the same in both UI languages.
import { QrCode } from "lucide-react";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { Logo, PaperWidth, ReceiptHeaderView } from "@/db/bindings";

/** Printable dots across the paper: the printer's 203 dpi head. */
const PAPER_DOTS: Record<PaperWidth, number> = { mm80: 576, mm58: 384 };
/** On-screen pixels per paper dot. */
const SCALE = 0.5;

function LogoCanvas({ logo, className }: { logo: Logo; className?: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const ctx = ref.current?.getContext("2d");
    if (!ctx) return;
    const image = ctx.createImageData(logo.width, logo.height);
    const rowBytes = logo.width / 8;
    for (let y = 0; y < logo.height; y++) {
      for (let x = 0; x < logo.width; x++) {
        const black = ((logo.data[y * rowBytes + (x >> 3)] ?? 0) & (0x80 >> (x & 7))) !== 0;
        const at = (y * logo.width + x) * 4;
        const v = black ? 0 : 255;
        image.data.set([v, v, v, 255], at);
      }
    }
    ctx.putImageData(image, 0, 0);
  }, [logo]);
  return (
    <canvas
      ref={ref}
      width={logo.width}
      height={logo.height}
      className={className}
      style={{ width: logo.width * SCALE, height: logo.height * SCALE, imageRendering: "pixelated" }}
    />
  );
}

const ROWS: { key: string; qty: string; amount: string }[] = [
  { key: "itemA", qty: "2 x 450.00", amount: "900.00" },
  { key: "itemB", qty: "1 x 1,250.00", amount: "1,250.00" },
  { key: "itemC", qty: "3 x 95.00", amount: "285.00" },
];

function Pair({ left, right, bold }: { left: string; right: string; bold?: boolean }) {
  return (
    <div className={bold ? "flex justify-between gap-2 font-bold" : "flex justify-between gap-2"}>
      <span>{left}</span>
      <span>{right}</span>
    </div>
  );
}

export function ReceiptPreview({
  header,
  width,
}: {
  header: ReceiptHeaderView | undefined;
  width: PaperWidth;
}) {
  const { t } = useTranslation("printing");
  return (
    <figure className="flex flex-col items-center gap-2">
      <div
        role="img"
        aria-label={t("preview.title")}
        className="flex flex-col gap-1 rounded-sm border border-border bg-white p-3 font-mono text-[11px] leading-snug text-black shadow-sm"
        style={{ width: PAPER_DOTS[width] * SCALE + 24 }}
      >
        {header?.logo && (
          <div className="flex justify-center pb-1">
            <LogoCanvas logo={header.logo} />
          </div>
        )}
        <p className="text-center text-base font-bold">{header?.store_name}</p>
        <p className="text-center font-bold">{t("preview.banner")}</p>
        <Pair left={t("preview.receipt")} right="SAMPLE" />
        <hr className="border-dashed border-black/60" />
        {ROWS.map((r) => (
          <div key={r.key}>
            <p>{t(`preview.${r.key}`)}</p>
            <Pair left={`  ${r.qty}`} right={r.amount} />
          </div>
        ))}
        <hr className="border-dashed border-black/60" />
        <Pair left={t("preview.subtotal")} right="2,435.00" />
        <Pair left={t("preview.total")} right="Rs. 2,435.00" bold />
        <Pair left={t("preview.cash")} right="3,000.00" />
        <Pair left={t("preview.change")} right="565.00" />
        <QrCode aria-hidden className="mx-auto my-1 size-12" strokeWidth={1.5} />
        <p className="text-center">{header?.footer}</p>
      </div>
      <figcaption className="text-sm text-muted-foreground">{t("preview.title")}</figcaption>
    </figure>
  );
}
