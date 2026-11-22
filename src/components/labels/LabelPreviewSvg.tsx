// WYSIWYG label preview (doc §7.4): draws the Rust-computed `LabelPreview` (the same data the TSPL / ZPL
// emitters print) as SVG in millimetres: the roll, each label, its fitted text lines and the real bars.
import type { LabelPreview, PlacedBarcode, PlacedText } from "@/db/bindings";

/** Built-in font cell heights in dots for steps 1–6 (mirrors `layout::font_dots`). */
const FONT_DOTS_H_203 = [12, 12, 20, 24, 32, 40, 48];
const FONT_DOTS_H_300 = [20, 20, 28, 32, 48, 60, 72];

function fontMm(step: number, dpi: number): number {
  const table = dpi >= 300 ? FONT_DOTS_H_300 : FONT_DOTS_H_203;
  return ((table[step] ?? 48) * 25.4) / dpi;
}

function Text({ t, dpi }: { t: PlacedText; dpi: number }) {
  const size = fontMm(t.font_size, dpi);
  const anchor = t.align === "center" ? "middle" : t.align === "right" ? "end" : "start";
  const x = t.align === "center" ? t.x_mm + t.w_mm / 2 : t.align === "right" ? t.x_mm + t.w_mm : t.x_mm;
  return (
    <g fontWeight={t.bold ? 700 : 400} fontSize={size * 0.85} textAnchor={anchor} fontFamily="monospace">
      {t.lines.map((line, i) => (
        <text key={i} x={x} y={t.y_mm + size * (i + 0.8)} fill="currentColor">
          {line}
        </text>
      ))}
    </g>
  );
}

function Bars({ b }: { b: PlacedBarcode }) {
  let x = b.x_mm;
  const rects: { x: number; w: number }[] = [];
  b.widths.forEach((w, i) => {
    const width = w * b.module_mm;
    if (i % 2 === 0) rects.push({ x, w: width });
    x += width;
  });
  return (
    <g data-testid="barcode" data-fits={b.fits}>
      {rects.map((r, i) => (
        <rect key={i} x={r.x} y={b.y_mm} width={r.w} height={b.bar_h_mm} fill="currentColor" />
      ))}
      <text
        x={(b.x_mm + x) / 2}
        y={b.y_mm + b.bar_h_mm + 2.1}
        fontSize={2.1}
        textAnchor="middle"
        fontFamily="monospace"
        fill="currentColor"
      >
        {b.data}
      </text>
    </g>
  );
}

const SINGLE_PAD_MM = 1.5;

/**
 * The roll as it will print, or with `single` just the first label, cropped to it: what "Print labels" shows,
 * since twenty copies of the same label add nothing.
 */
export function LabelPreviewSvg({
  preview,
  title,
  maxRows = 6,
  single = false,
  className = "w-full rounded-md border border-border bg-white text-black",
}: {
  preview: LabelPreview;
  title: string;
  maxRows?: number;
  single?: boolean;
  className?: string;
}) {
  const rows = Math.min(preview.rows, maxRows);
  const pitch = preview.label_h_mm + preview.row_gap_mm;
  const first = preview.labels[0];
  if (single && !first) return null;
  const labels = single && first ? [first] : preview.labels.filter((l) => l.row < rows);
  const viewBox =
    single && first
      ? `${first.x_mm - SINGLE_PAD_MM} ${-SINGLE_PAD_MM} ${preview.label_w_mm + 2 * SINGLE_PAD_MM} ${
          preview.label_h_mm + 2 * SINGLE_PAD_MM
        }`
      : `0 0 ${preview.roll_width_mm} ${Math.max(rows, 1) * pitch}`;
  return (
    <svg role="img" aria-label={title} viewBox={viewBox} className={className}>
      {labels.map((l) => (
        <g key={`${l.row}:${l.col}`} transform={`translate(0 ${single ? 0 : l.row * pitch})`}>
          <rect
            x={l.x_mm}
            y={0}
            width={preview.label_w_mm}
            height={preview.label_h_mm}
            rx={1}
            fill="white"
            stroke="currentColor"
            strokeOpacity={0.35}
            strokeWidth={0.2}
          />
          {l.texts.map((t, i) => (
            <Text key={i} t={t} dpi={preview.dpi} />
          ))}
          {l.barcode && <Bars b={l.barcode} />}
        </g>
      ))}
    </svg>
  );
}
