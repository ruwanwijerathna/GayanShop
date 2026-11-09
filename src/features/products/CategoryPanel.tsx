import { Pencil, Plus, Trash2 } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Can } from "@/components/auth/Can";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { Category } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useDeleteCategory, useSaveCategory } from "@/db/hooks";
import { translateError } from "@/i18n";
import { playCue } from "@/lib/audio";

/** "Kitchen" or "Home › Kitchen" — the parent chain, for lists and pickers. */
export function categoryPath(c: Category, all: Category[]): string {
  const byId = new Map(all.map((x) => [x.id, x]));
  const names = [c.name];
  let parent = c.parent_id ? byId.get(c.parent_id) : undefined;
  for (let depth = 0; parent && depth < 16; depth += 1) {
    names.unshift(parent.name);
    parent = parent.parent_id ? byId.get(parent.parent_id) : undefined;
  }
  return names.join(" › ");
}

/** Category list with add / rename / delete (PRODUCT_MANAGE). Rust refuses deleting a category in use. */
export function CategoryPanel({ categories }: { categories: Category[] }) {
  const { t, i18n } = useTranslation(["products", "common"]);
  const save = useSaveCategory();
  const remove = useDeleteCategory();
  const [editing, setEditing] = useState<string | null>(null); // category id, or "new"
  const [name, setName] = useState("");
  const [parentId, setParentId] = useState("");

  const failure = save.error ?? remove.error;
  const error = failure instanceof CommandError ? translateError(i18n.t, failure.error) : null;
  const sorted = [...categories].sort((a, b) =>
    categoryPath(a, categories).localeCompare(categoryPath(b, categories)),
  );

  const start = (c: Category | null) => {
    save.reset();
    remove.reset();
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

  return (
    <section
      aria-labelledby="categories-title"
      className="flex flex-col gap-3 rounded-lg border border-border p-4"
    >
      <header className="flex items-center gap-2">
        <h2 id="categories-title" className="flex-1 text-lg font-semibold">
          {t("categories.title")}
        </h2>
        <Can perm="PRODUCT_MANAGE">
          <Button type="button" size="pos" variant="outline" onClick={() => start(null)}>
            <Plus aria-hidden />
            {t("categories.add")}
          </Button>
        </Can>
      </header>
      {editing && (
        <form
          className="flex flex-col gap-3 rounded-md bg-muted p-3"
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
        >
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
            <select
              id="category-parent"
              className="h-12 w-full rounded-md border border-input bg-background px-3 text-base"
              value={parentId}
              onChange={(e) => setParentId(e.target.value)}
            >
              <option value="">{t("categories.topLevel")}</option>
              {sorted
                .filter((c) => c.id !== editing)
                .map((c) => (
                  <option key={c.id} value={c.id}>
                    {categoryPath(c, categories)}
                  </option>
                ))}
            </select>
          </div>
          <div className="flex gap-2">
            <Button type="submit" size="pos" disabled={save.isPending}>
              {t("common:actions.save")}
            </Button>
            <Button type="button" size="pos" variant="outline" onClick={() => setEditing(null)}>
              {t("common:actions.cancel")}
            </Button>
          </div>
        </form>
      )}
      {error && (
        <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
          {error}
        </p>
      )}
      {sorted.length === 0 ? (
        <p className="text-muted-foreground">{t("categories.empty")}</p>
      ) : (
        <ul className="flex flex-col divide-y divide-border">
          {sorted.map((c) => {
            const path = categoryPath(c, categories);
            return (
              <li key={c.id} className="flex min-h-12 items-center gap-2 py-1">
                <span className="flex-1">
                  {path}
                  <span className="ml-2 text-sm text-muted-foreground">
                    {t("categories.products", { count: c.product_count })}
                  </span>
                </span>
                <Can perm="PRODUCT_MANAGE">
                  <Button
                    type="button"
                    size="icon-pos"
                    variant="outline"
                    aria-label={t("categories.renameLabel", { name: path })}
                    onClick={() => start(c)}
                  >
                    <Pencil aria-hidden />
                  </Button>
                  <Button
                    type="button"
                    size="icon-pos"
                    variant="outline"
                    aria-label={t("categories.deleteLabel", { name: path })}
                    disabled={remove.isPending}
                    onClick={() => {
                      save.reset();
                      remove.mutate(c.id, { onError: () => playCue("error") });
                    }}
                  >
                    <Trash2 aria-hidden />
                  </Button>
                </Can>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
