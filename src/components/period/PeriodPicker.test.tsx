import { fireEvent, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Period } from "@/db/bindings";
import { renderWithProviders } from "@/test/render";
import { PeriodPicker, periodFor } from "./PeriodPicker";

const TODAY = "2026-11-08";

describe("periodFor", () => {
  it("anchors every kind on today's business date", () => {
    expect(periodFor("day", TODAY)).toEqual({ kind: "day", date: TODAY });
    expect(periodFor("week", TODAY)).toEqual({ kind: "week", any_date: TODAY });
    expect(periodFor("month", TODAY)).toEqual({ kind: "month", year: 2026, month: 11 });
    expect(periodFor("year", TODAY)).toEqual({ kind: "year", year: 2026 });
    expect(periodFor("all_time", TODAY)).toEqual({ kind: "all_time" });
    expect(periodFor("custom", TODAY)).toEqual({ kind: "custom", from: "2026-11-01", to: TODAY });
  });
});

describe("PeriodPicker", () => {
  function setup(value: Period) {
    const onChange = vi.fn();
    renderWithProviders(<PeriodPicker value={value} onChange={onChange} today={TODAY} />);
    return onChange;
  }

  it("switches kinds with large buttons", () => {
    const onChange = setup({ kind: "day", date: TODAY });
    expect(screen.getByRole("button", { name: "Day" })).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByRole("button", { name: "Year" }));
    expect(onChange).toHaveBeenLastCalledWith({ kind: "year", year: 2026 });
  });

  it("edits a month, a year and a custom range, ignoring partial input", () => {
    const onMonth = setup({ kind: "month", year: 2026, month: 11 });
    fireEvent.change(screen.getByLabelText("Month"), { target: { value: "2026-02" } });
    expect(onMonth).toHaveBeenLastCalledWith({ kind: "month", year: 2026, month: 2 });
  });

  it("keeps the other end of a custom range", () => {
    const onChange = setup({ kind: "custom", from: "2026-11-01", to: TODAY });
    fireEvent.change(screen.getByLabelText("From"), { target: { value: "2026-10-15" } });
    expect(onChange).toHaveBeenLastCalledWith({ kind: "custom", from: "2026-10-15", to: TODAY });
    onChange.mockClear();
    fireEvent.change(screen.getByLabelText("To"), { target: { value: "" } });
    expect(onChange).not.toHaveBeenCalled();
  });

  it("accepts only a four-digit year", () => {
    const onChange = setup({ kind: "year", year: 2026 });
    fireEvent.change(screen.getByLabelText("Year"), { target: { value: "20" } });
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Year"), { target: { value: "2025" } });
    expect(onChange).toHaveBeenLastCalledWith({ kind: "year", year: 2025 });
  });
});
