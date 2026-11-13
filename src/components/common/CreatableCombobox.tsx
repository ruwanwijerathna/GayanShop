import { Check, ChevronsUpDown, Plus } from "lucide-react";
import { useId, useState, type KeyboardEvent } from "react";
import { Input } from "@/components/ui/input";
import { Popover, PopoverAnchor, PopoverContent } from "@/components/ui/popover";
import { cn } from "@/lib/utils";

export interface ComboboxOption {
  value: string;
  label: string;
}

export interface CreatableComboboxProps {
  id?: string;
  value: string;
  options: readonly ComboboxOption[];
  onValueChange: (value: string) => void;
  /** Create an option from the typed text and return its value (or null if it failed). */
  onCreate: (label: string) => Promise<string | null>;
  /** The "nothing chosen" row shown while the box is empty. */
  emptyLabel: string;
  createLabel: (typed: string) => string;
  noMatchLabel: string;
  placeholder?: string;
  disabled?: boolean;
  "aria-label"?: string;
}

type Row = { kind: "none" } | { kind: "option"; option: ComboboxOption } | { kind: "create"; typed: string };

/**
 * A searchable select that can also create: type to filter, pick a suggestion with the mouse, touch or
 * arrow keys + Enter, or choose "Create “text”" when nothing matches exactly. Focus stays in the input.
 */
export function CreatableCombobox({
  id,
  value,
  options,
  onValueChange,
  onCreate,
  emptyLabel,
  createLabel,
  noMatchLabel,
  placeholder,
  disabled,
  "aria-label": ariaLabel,
}: CreatableComboboxProps) {
  const listId = useId();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState<string | null>(null);
  const [active, setActive] = useState(0);
  const [creating, setCreating] = useState(false);

  const selected = options.find((o) => o.value === value);
  const typed = (query ?? "").trim();
  const needle = typed.toLowerCase();
  const matches = needle === "" ? options : options.filter((o) => o.label.toLowerCase().includes(needle));
  const exact = options.some((o) => o.label.toLowerCase() === needle);
  const rows: Row[] = [
    ...(needle === "" ? [{ kind: "none" } as const] : []),
    ...matches.map((option) => ({ kind: "option", option }) as const),
    ...(typed !== "" && !exact ? [{ kind: "create", typed } as const] : []),
  ];
  const current = Math.min(active, Math.max(rows.length - 1, 0));

  const close = () => {
    setOpen(false);
    setQuery(null);
    setActive(0);
  };

  const choose = async (row: Row | undefined) => {
    if (!row) return;
    if (row.kind === "none") onValueChange("");
    else if (row.kind === "option") onValueChange(row.option.value);
    else {
      setCreating(true);
      const created = await onCreate(row.typed).finally(() => setCreating(false));
      if (created === null) return;
      onValueChange(created);
    }
    close();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) setOpen(true);
      const step = e.key === "ArrowDown" ? 1 : -1;
      setActive((Math.max(rows.length, 1) + current + step) % Math.max(rows.length, 1));
    } else if (e.key === "Enter" && open) {
      e.preventDefault();
      void choose(rows[current]);
    } else if (e.key === "Escape" && open) {
      e.stopPropagation();
      close();
    }
  };

  return (
    <Popover open={open} onOpenChange={(o) => (o ? setOpen(true) : close())}>
      <PopoverAnchor asChild>
        <div className="relative">
          <Input
            id={id}
            role="combobox"
            aria-label={ariaLabel}
            aria-expanded={open}
            aria-controls={listId}
            aria-autocomplete="list"
            aria-activedescendant={open && rows.length > 0 ? `${listId}-${current}` : undefined}
            autoComplete="off"
            disabled={disabled || creating}
            className="h-12 pr-10 text-base"
            placeholder={selected ? undefined : placeholder}
            value={query ?? selected?.label ?? ""}
            onFocus={(e) => {
              setOpen(true);
              e.currentTarget.select();
            }}
            onChange={(e) => {
              setQuery(e.target.value);
              setActive(0);
              setOpen(true);
            }}
            onKeyDown={onKeyDown}
          />
          <ChevronsUpDown
            aria-hidden
            className="pointer-events-none absolute top-1/2 right-3 size-4 -translate-y-1/2 text-muted-foreground"
          />
        </div>
      </PopoverAnchor>
      <PopoverContent
        className="w-(--radix-popover-trigger-width) p-1"
        onOpenAutoFocus={(e) => e.preventDefault()}
        onInteractOutside={(e) => {
          if (e.target instanceof Node && document.getElementById(id ?? "")?.contains(e.target))
            e.preventDefault();
        }}
      >
        <ul
          id={listId}
          role="listbox"
          aria-label={ariaLabel}
          className="flex max-h-64 flex-col overflow-y-auto"
        >
          {rows.length === 0 && <li className="px-3 py-3 text-sm text-muted-foreground">{noMatchLabel}</li>}
          {rows.map((row, i) => {
            const isCreate = row.kind === "create";
            const label =
              row.kind === "none"
                ? emptyLabel
                : row.kind === "option"
                  ? row.option.label
                  : createLabel(row.typed);
            const isSelected =
              row.kind === "option" ? row.option.value === value : row.kind === "none" && value === "";
            return (
              <li
                key={row.kind === "option" ? row.option.value : row.kind}
                id={`${listId}-${i}`}
                role="option"
                aria-selected={isSelected}
                onMouseEnter={() => setActive(i)}
                onMouseDown={(e) => {
                  e.preventDefault();
                  void choose(row);
                }}
                className={cn(
                  "flex min-h-11 cursor-pointer items-center gap-2 rounded-sm px-3 text-base",
                  i === current && "bg-accent text-accent-foreground",
                  isCreate && "font-medium",
                )}
              >
                {isCreate ? (
                  <Plus aria-hidden className="size-4" />
                ) : (
                  <Check aria-hidden className={cn("size-4", isSelected ? "opacity-100" : "opacity-0")} />
                )}
                <span className="truncate">{label}</span>
              </li>
            );
          })}
        </ul>
      </PopoverContent>
    </Popover>
  );
}
