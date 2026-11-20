import { encodeCode128 } from "@/lib/barcode128";

interface Barcode128Props {
  data: string;
  height?: number;
  moduleWidth?: number;
  className?: string;
}

export function Barcode128({ data, height = 42, moduleWidth = 1.6, className }: Barcode128Props) {
  const widths = encodeCode128(data);
  if (!widths) return null;

  const totalModules = widths.reduce((sum, w) => sum + w, 0);
  const totalWidth = totalModules * moduleWidth;

  const rects: { x: number; width: number }[] = [];
  let currentX = 0;

  for (let i = 0; i < widths.length; i++) {
    const w = (widths[i] ?? 0) * moduleWidth;
    if (i % 2 === 0) {
      rects.push({ x: currentX, width: w });
    }
    currentX += w;
  }

  return (
    <div className={`flex flex-col items-center gap-1 ${className ?? ""}`}>
      <svg
        width={totalWidth}
        height={height}
        viewBox={`0 0 ${totalWidth} ${height}`}
        className="overflow-visible"
        aria-label={`Barcode: ${data}`}
      >
        {rects.map((r, idx) => (
          <rect key={idx} x={r.x} y={0} width={r.width} height={height} fill="currentColor" />
        ))}
      </svg>
      <span className="font-mono text-xs tracking-wider text-black">{data}</span>
    </div>
  );
}
