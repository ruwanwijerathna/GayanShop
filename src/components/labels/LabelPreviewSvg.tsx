// WYSIWYG label preview (doc §7.4): draws the Rust-computed `LabelPreview` (the same data the TSPL / ZPL
// emitters print) as SVG in millimetres: the roll, each label, its fitted text lines and the real bars.
import type { LabelPreview, PlacedBarcode, PlacedText } from "@/db/bindings";

/** Built-in font cell heights in dots for steps 1–4 (mirrors `layout::font_dots`). */
const FONT_DOTS_H = [12, 12, 20, 24, 32];

function fontMm(step: number, dpi: number): number {
  return ((FONT_DOTS_H[step] ?? 32) * 25.4) / dpi;
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
        y={b.y_mm + b.bar_h_mm + 1.6}
        fontSize={1.6}
        textAnchor="middle"
        fontFamily="monospace"
        fill="currentColor"
      >
        {b.data}
      </text>
    </g>
  );
}

export function LabelPreviewSvg({
  preview,
  title,
  maxRows = 6,
}: {
  preview: LabelPreview;
  title: string;
  maxRows?: number;
}) {
  const rows = Math.min(preview.rows, maxRows);
  const pitch = preview.label_h_mm + preview.row_gap_mm;
  const height = Math.max(rows, 1) * pitch;
  return (
    <svg
      role="img"
      aria-label={title}
      viewBox={`0 0 ${preview.roll_width_mm} ${height}`}
      className="w-full rounded-md border border-border bg-white text-black"
    >
      {preview.labels
        .filter((l) => l.row < rows)
        .map((l) => (
          <g key={`${l.row}:${l.col}`} transform={`translate(0 ${l.row * pitch})`}>
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
