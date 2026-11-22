// One sticker, drawn in HTML/CSS at exactly one CSS pixel per printer dot (decision #30). Rust supplies every
// box in whole dots, and the bars come from its encoder as whole-dot rectangles with crisp edges, so when this
// element is turned into the printer's 1-bit picture each bar lands on exact dots and scans. The preview on
// screen is this same element, scaled, so what you see is what prints.
import type { CSSProperties } from "react";
import type {
  LabelLayout_Serialize as LabelLayout,
  Sticker_Serialize as Sticker,
  StickerBarcode,
  StickerText,
} from "@/db/bindings";

const SANS = '"Inter", "Noto Sans Sinhala", sans-serif';
const MONO = '"JetBrains Mono", monospace';

function Text({ t }: { t: StickerText }) {
  const clamp = t.max_lines > 1;
  const style: CSSProperties = {
    position: "absolute",
    left: t.x,
    top: t.y,
    width: t.w,
    height: clamp ? t.h : Math.max(t.h, t.font_px),
    fontFamily: SANS,
    fontSize: t.font_px,
    lineHeight: `${t.font_px}px`,
    fontWeight: t.bold ? 800 : 500,
    textAlign: t.align,
    overflow: "hidden",
    ...(clamp
      ? { display: "-webkit-box", WebkitLineClamp: t.max_lines, WebkitBoxOrient: "vertical" }
      : { whiteSpace: "nowrap", textOverflow: "ellipsis" }),
  };
  return <div style={style}>{t.text}</div>;
}

/** The bars as rectangles on whole dots: every bar and space is `module` dots per module. */
function Bars({ b, labelH }: { b: StickerBarcode; labelH?: number }) {
  const rects: { x: number; w: number }[] = [];
  let x = 0;
  b.widths.forEach((modules, i) => {
    const w = modules * b.module;
    if (i % 2 === 0) rects.push({ x, w });
    x += w;
  });
  const is16 = (labelH ?? 25) <= 16;
  const is20 = (labelH ?? 25) > 16 && (labelH ?? 25) <= 20;
  const fontSize = is16 ? 13 : is20 ? 16 : Math.round(b.hri_px * 1.15);
  const gap = is16 ? 3 : is20 ? 4 : Math.max(6, Math.round(b.hri_px * 0.5));
  return (
    <>
      <svg
        data-testid="sticker-bars"
        width={x}
        height={b.bar_h}
        viewBox={`0 0 ${x} ${b.bar_h}`}
        shapeRendering="crispEdges"
        style={{ position: "absolute", left: b.x, top: b.y }}
      >
        {rects.map((r) => (
          <rect key={r.x} x={r.x} y={0} width={r.w} height={b.bar_h} fill="#000" />
        ))}
      </svg>
      <div
        style={{
          position: "absolute",
          left: b.x,
          top: b.y + b.bar_h + gap,
          width: x,
          textAlign: "center",
          fontFamily: MONO,
          fontSize,
          lineHeight: `${fontSize}px`,
        }}
      >
        {b.data}
      </div>
    </>
  );
}

/** The sticker at print size: `layout.width` × `layout.height` CSS pixels, black on white. */
export function LabelSticker({ layout, sticker }: { layout: LabelLayout; sticker: Sticker }) {
  return (
    <div
      data-sticker={sticker.product_id}
      style={{
        position: "relative",
        width: layout.width,
        height: layout.height,
        overflow: "hidden",
        background: "#fff",
        color: "#000",
      }}
    >
      {sticker.texts.map((t) => (
        <Text key={`${t.kind}:${t.y}:${t.x}`} t={t} />
      ))}
      {sticker.barcode && <Bars b={sticker.barcode} labelH={layout.label_h_mm} />}
    </div>
  );
}

/** The same sticker shrunk to `width` CSS pixels for the screen. */
export function LabelStickerPreview({
  layout,
  sticker,
  title,
  width,
  className,
}: {
  layout: LabelLayout;
  sticker: Sticker;
  title: string;
  width: number;
  className?: string;
}) {
  const scale = width / layout.width;
  return (
    <div
      role="img"
      aria-label={title}
      className={className}
      style={{ width, height: Math.round(layout.height * scale), overflow: "hidden" }}
    >
      <div style={{ transform: `scale(${scale})`, transformOrigin: "top left" }}>
        <LabelSticker layout={layout} sticker={sticker} />
      </div>
    </div>
  );
}
