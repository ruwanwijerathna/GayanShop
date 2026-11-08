import { fireEvent, render, screen } from "@testing-library/react";
import type { ColumnDef } from "@tanstack/react-table";
import { I18nextProvider } from "react-i18next";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { createI18n } from "@/i18n";
import { DataTable } from "./DataTable";

interface Row {
  id: string;
  name: string;
  qty: number;
}

const columns: ColumnDef<Row>[] = [
  { accessorKey: "name", header: "Name", size: 200 },
  { accessorKey: "qty", header: "Qty", size: 80 },
];

const many: Row[] = Array.from({ length: 10_000 }, (_, i) => ({
  id: `r${i}`,
  name: `Item ${String(i).padStart(5, "0")}`,
  qty: i % 7,
}));

beforeAll(() => {
  // jsdom has no layout: give the scroll viewport a real size so the virtualiser computes a window.
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    width: 800,
    height: 480,
    top: 0,
    left: 0,
    bottom: 480,
    right: 800,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  });
  // virtual-core measures the scroll element with clientWidth/clientHeight.
  Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 480 });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => 800 });
});

function renderTable(props: Partial<Parameters<typeof DataTable<Row>>[0]> = {}) {
  return render(
    <I18nextProvider i18n={createI18n("en")}>
      <DataTable<Row>
        columns={columns}
        data={many}
        getRowId={(r) => r.id}
        ariaLabel="items"
        height={480}
        {...props}
      />
    </I18nextProvider>,
  );
}

describe("DataTable", () => {
  it("renders only a window of rows out of 10,000", () => {
    renderTable();
    const rendered = screen.getAllByRole("row").length - 1; // minus header row
    expect(rendered).toBeGreaterThan(0);
    expect(rendered).toBeLessThan(40); // 480px / 48px = 10 visible + overscan
    expect(screen.getByRole("grid")).toHaveAttribute("aria-rowcount", "10001");
  });

  it("moves the row cursor with the keyboard and activates with Enter", () => {
    const onRowActivate = vi.fn();
    renderTable({ onRowActivate });
    const grid = screen.getByRole("grid");
    fireEvent.keyDown(grid, { key: "ArrowDown" });
    fireEvent.keyDown(grid, { key: "ArrowDown" });
    fireEvent.keyDown(grid, { key: "Enter" });
    expect(onRowActivate).toHaveBeenCalledWith(many[2]);
    fireEvent.keyDown(grid, { key: "Home" });
    fireEvent.keyDown(grid, { key: "Enter" });
    expect(onRowActivate).toHaveBeenLastCalledWith(many[0]);
  });

  it("activates a row on click (touch)", () => {
    const onRowActivate = vi.fn();
    renderTable({ onRowActivate });
    fireEvent.click(screen.getByText("Item 00003"));
    expect(onRowActivate).toHaveBeenCalledWith(many[3]);
  });

  it("sorts client-side from the header (numbers descend first, then ascend)", () => {
    renderTable({ data: many.slice(0, 5) });
    const firstQty = () => screen.getAllByRole("row")[1]?.querySelectorAll("[role=gridcell]")[1]?.textContent;
    fireEvent.click(screen.getByRole("button", { name: "Qty" }));
    expect(firstQty()).toBe("4");
    expect(screen.getAllByRole("columnheader")[1]).toHaveAttribute("aria-sort", "descending");
    fireEvent.click(screen.getByRole("button", { name: /Qty/ }));
    expect(firstQty()).toBe("0");
    expect(screen.getAllByRole("columnheader")[1]).toHaveAttribute("aria-sort", "ascending");
  });

  it("asks for the next page near the end", () => {
    const onEndReached = vi.fn();
    renderTable({ data: many.slice(0, 8), onEndReached });
    expect(onEndReached).toHaveBeenCalled();
  });

  it("shows the empty state", () => {
    renderTable({ data: [] });
    expect(screen.getByText("Nothing to show yet")).toBeInTheDocument();
  });
});
