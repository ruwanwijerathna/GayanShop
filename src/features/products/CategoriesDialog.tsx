import { Check, ChevronRight, FolderPlus, FolderTree, Pencil, Search, Trash2, X } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Can } from "@/components/auth/Can";
import { SelectField } from "@/components/common/SelectField";
import { WithTooltip } from "@/components/common/WithTooltip";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { Category } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useDeleteCategory, useSaveCategory } from "@/db/hooks";
import { translateError } from "@/i18n";
import { playCue } from "@/lib/audio";
import { categoryDepth, categoryPath } from "./categories";

const SEARCH_FROM = 8;

/**
 * Category manager (add / rename / delete need `PRODUCT_MANAGE`; Rust refuses deleting a category in use).
 * Opened from the products toolbar; "Show products" applies the category filter and closes the dialog.
 */
export function CategoriesDialog({
  open,
  onOpenChange,
  categories,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  categories: Category[];
}) {
  const { t, i18n } = useTranslation(["products", "common"]);
  const save = useSaveCategory();
  const remove = useDeleteCategory();
  const [editing, setEditing] = useState<string | null>(null); // category id, or "new"
  const [name, setName] = useState("");
  const [parentId, setParentId] = useState("");
  const [confirming, setConfirming] = useState<string | null>(null);
  const [query, setQuery] = useState("");

  const failure = save.error ?? remove.error;
  const error = failure instanceof CommandError ? translateError(i18n.t, failure.error) : null;

  const sorted = useMemo(
    () =>
      [...categories].sort((a, b) => categoryPath(a, categories).localeCompare(categoryPath(b, categories))),
    [categories],
  );
  const q = query.trim().toLowerCase();
  const visible = q ? sorted.filter((c) => categoryPath(c, categories).toLowerCase().includes(q)) : sorted;

  const start = (c: Category | null) => {
    save.reset();
    remove.reset();
    setConfirming(null);
    setEditing(c?.id ?? "new");
    setName(c?.name ?? "");
    setParentId(c?.parent_id ?? "");
  };

  const submit = () => {
    if (!editing) return;
    save.mutate(
      {
        id: editing === "new" ? null : editing,
        input: { name, parent_id: parentId === "" ? null : parentId },
      },
      { onSuccess: () => setEditing(null), onError: () => playCue("error") },
    );
  };

  const close = (next: boolean) => {
    if (!next) {
      setEditing(null);
      setConfirming(null);
      setQuery("");
      save.reset();
      remove.reset();
    }
    onOpenChange(next);
  };

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="flex max-h-[85vh] flex-col gap-0 overflow-hidden rounded-2xl p-0 sm:max-w-xl">
        <DialogHeader className="border-b bg-muted/30 px-6 py-5">
          <div className="flex items-center gap-3 pr-8">
            <span
              aria-hidden
              className="flex size-11 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary ring-1 ring-primary/15"
            >
              <FolderTree className="size-5" />
            </span>
            <div className="min-w-0 flex-1">
              <DialogTitle className="text-xl tracking-tight">{t("categories.title")}</DialogTitle>
              <DialogDescription>{t("categories.description")}</DialogDescription>
            </div>
            <Can perm="PRODUCT_MANAGE">
              <Button type="button" size="pos" className="rounded-xl" onClick={() => start(null)}>
                <FolderPlus aria-hidden />
                {t("categories.add")}
              </Button>
            </Can>
          </div>
        </DialogHeader>

        <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-6 py-5">
          {editing && (
            <form
              className="flex flex-col gap-4 rounded-xl border bg-muted/30 p-4 motion-safe:animate-in motion-safe:fade-in-0 motion-safe:slide-in-from-top-1"
              onSubmit={(e) => {
                e.preventDefault();
                submit();
              }}
            >
              <h3 className="text-sm font-semibold">
                {editing === "new" ? t("categories.newTitle") : t("categories.editTitle")}
              </h3>
              <div className="grid gap-4 sm:grid-cols-2">
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="category-name">{t("categories.name")}</Label>
                  <Input
                    id="category-name"
                    className="h-12 text-base"
                    value={name}
                    maxLength={60}
                    autoComplete="off"
                    onChange={(e) => setName(e.target.value)}
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="category-parent">{t("categories.parent")}</Label>
                  <SelectField
                    id="category-parent"
                    value={parentId}
                    onValueChange={setParentId}
                    options={[
                      { value: "", label: t("categories.topLevel") },
                      ...sorted
                        .filter((c) => c.id !== editing)
                        .map((c) => ({ value: c.id, label: categoryPath(c, categories) })),
                    ]}
                  />
                </div>
              </div>
              <div className="flex justify-end gap-2">
                <Button
                  type="button"
                  size="pos"
                  variant="ghost"
                  className="rounded-xl"
                  onClick={() => setEditing(null)}
                >
                  {t("common:actions.cancel")}
                </Button>
                <Button type="submit" size="pos" className="rounded-xl px-6" disabled={save.isPending}>
                  {t("common:actions.save")}
                </Button>
              </div>
            </form>
          )}

          {error && (
            <p
              role="alert"
              className="rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-3 text-destructive motion-safe:animate-in motion-safe:fade-in-0"
            >
              {error}
            </p>
          )}

          {categories.length >= SEARCH_FROM && (
            <div className="relative">
              <Search
                aria-hidden
                className="pointer-events-none absolute top-1/2 left-3.5 size-4 -translate-y-1/2 text-muted-foreground"
              />
              <Input
                type="search"
                aria-label={t("categories.search")}
                placeholder={t("categories.search")}
                className="h-12 rounded-xl pl-10 text-base"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </div>
          )}

          {categories.length === 0 ? (
            <div className="flex flex-col items-center gap-2 py-10 text-center text-muted-foreground">
              <span className="flex size-12 items-center justify-center rounded-full bg-muted">
                <FolderTree aria-hidden className="size-6" />
              </span>
              <p>{t("categories.empty")}</p>
            </div>
          ) : visible.length === 0 ? (
            <p className="py-6 text-center text-muted-foreground">{t("categories.noMatch")}</p>
          ) : (
            <ul className="flex flex-col gap-1.5">
              {visible.map((c) => {
                const path = categoryPath(c, categories);
                const depth = q ? 0 : categoryDepth(c, categories);
                const isConfirming = confirming === c.id;
                return (
                  <li
                    key={c.id}
                    style={{ marginLeft: depth * 20 }}
                    className={
                      isConfirming
                        ? "flex min-h-14 flex-wrap items-center gap-2 rounded-xl border border-destructive/40 bg-destructive/5 px-3 py-2"
                        : "group flex min-h-14 items-center gap-2 rounded-xl border bg-card px-3 py-1.5 transition-colors hover:bg-muted/50"
                    }
                  >
                    {isConfirming ? (
                      <>
                        <span className="min-w-0 flex-1 text-sm font-medium">
                          {t("categories.confirmDelete", { name: c.name })}
                        </span>
                        <Button
                          type="button"
                          size="pos"
                          variant="ghost"
                          className="rounded-xl"
                          onClick={() => setConfirming(null)}
                        >
                          <X aria-hidden />
                          {t("common:actions.cancel")}
                        </Button>
                        <Button
                          type="button"
                          size="pos"
                          variant="destructive"
                          className="rounded-xl"
                          disabled={remove.isPending}
                          onClick={() => {
                            save.reset();
                            remove.mutate(c.id, {
                              onSuccess: () => setConfirming(null),
                              onError: () => {
                                setConfirming(null);
                                playCue("error");
                              },
                            });
                          }}
                        >
                          <Check aria-hidden />
                          {t("categories.confirmYes")}
                        </Button>
                      </>
                    ) : (
                      <>
                        {depth > 0 && (
                          <ChevronRight aria-hidden className="size-4 shrink-0 text-muted-foreground" />
                        )}
                        <span className="min-w-0 flex-1">
                          <span className="block truncate font-medium">{q ? path : c.name}</span>
                        </span>
                        <Badge variant="secondary" className="tabular-nums">
                          {t("categories.products", { count: c.product_count })}
                        </Badge>
                        <Can perm="PRODUCT_MANAGE">
                          <WithTooltip label={t("categories.renameLabel", { name: path })}>
                            <Button
                              type="button"
                              size="icon-pos"
                              variant="ghost"
                              className="rounded-xl"
                              aria-label={t("categories.renameLabel", { name: path })}
                              onClick={() => start(c)}
                            >
                              <Pencil aria-hidden />
                            </Button>
                          </WithTooltip>
                          <WithTooltip label={t("categories.deleteLabel", { name: path })}>
                            <Button
                              type="button"
                              size="icon-pos"
                              variant="ghost"
                              className="rounded-xl text-destructive hover:text-destructive"
                              aria-label={t("categories.deleteLabel", { name: path })}
                              onClick={() => {
                                save.reset();
                                remove.reset();
                                setEditing(null);
                                setConfirming(c.id);
                              }}
                            >
                              <Trash2 aria-hidden />
                            </Button>
                          </WithTooltip>
                        </Can>
                      </>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
