// A chart with its title and the same numbers as a visually hidden table, so screen readers (and tests)
// get every value, in the UI language. Charts only ever receive pre-aggregated rows from Rust (doc §1.4).
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

export interface ChartTable {
  columns: string[];
  rows: { key: string; cells: string[] }[];
}

export function ChartFigure({
  title,
  table,
  children,
  className,
}: {
  title: string;
  table: ChartTable;
  children: ReactNode;
  className?: string;
}) {
  const { t } = useTranslation("charts");
  return (
    <figure className={className ?? "flex flex-col gap-2"} aria-label={title}>
      <figcaption className="text-lg font-semibold">{title}</figcaption>
      {table.rows.length === 0 ? (
        <p className="flex h-40 items-center justify-center text-muted-foreground">{t("empty")}</p>
      ) : (
        <>
          <div aria-hidden>{children}</div>
          <table className="sr-only">
            <caption>{title}</caption>
            <thead>
              <tr>
                {table.columns.map((c) => (
                  <th key={c} scope="col">
                    {c}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {table.rows.map((r) => (
                <tr key={r.key}>
                  {r.cells.map((c, i) => (
                    <td key={i}>{c}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </figure>
  );
}
