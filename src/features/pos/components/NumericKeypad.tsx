import { CornerDownLeft, Delete } from "lucide-react";
import type { PointerEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";

const KEYS = ["7", "8", "9", "4", "5", "6", "1", "2", "3", "00", "0", "."] as const;

type Editable = HTMLInputElement | HTMLTextAreaElement;

function focusedField(): Editable | null {
  const el = document.activeElement;
  return el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement ? el : null;
}

/** Type like a keyboard would, so React's onChange sees it. */
export function typeInto(el: Editable, text: string | null) {
  const start = el.selectionStart ?? el.value.length;
  const end = el.selectionEnd ?? start;
  if (text === null) {
    if (start === end && start === 0) return;
    el.setRangeText("", start === end ? start - 1 : start, end, "end");
  } else {
    el.setRangeText(text, start, end, "end");
  }
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

/**
 * The 64 px touch keypad (doc §1.9). Keys act on the field that has focus (scan, phone, quick sale, a
 * payment amount) without stealing it: `pointerdown` is cancelled so focus never leaves the field.
 */
export function NumericKeypad() {
  const { t } = useTranslation("pos");
  const keep = (e: PointerEvent) => e.preventDefault();
  const press = (text: string | null) => {
    const el = focusedField();
    if (el) typeInto(el, text);
  };
  const enter = () => {
    const el = focusedField();
    el?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true, cancelable: true }),
    );
  };
  return (
    <div role="group" aria-label={t("keypad.label")} className="grid grid-cols-3 gap-2">
      {KEYS.map((k) => (
        <Button
          key={k}
          type="button"
          variant="outline"
          size="pos-lg"
          className="text-2xl tabular-nums"
          onPointerDown={keep}
          onClick={() => press(k)}
        >
          {k}
        </Button>
      ))}
      <Button
        type="button"
        variant="outline"
        size="pos-lg"
        onPointerDown={keep}
        onClick={() => press(null)}
        aria-label={t("keypad.back")}
      >
        <Delete aria-hidden />
      </Button>
      <Button
        type="button"
        variant="secondary"
        size="pos-lg"
        className="col-span-2"
        onPointerDown={keep}
        onClick={enter}
        aria-label={t("keypad.enter")}
      >
        <CornerDownLeft aria-hidden />
      </Button>
    </div>
  );
}
