import type { Category } from "@/db/bindings";

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

/** How many parents a category sits under (0 = top level); capped like `categoryPath`. */
export function categoryDepth(c: Category, all: Category[]): number {
  const byId = new Map(all.map((x) => [x.id, x]));
  let depth = 0;
  let parent = c.parent_id ? byId.get(c.parent_id) : undefined;
  while (parent && depth < 16) {
    depth += 1;
    parent = parent.parent_id ? byId.get(parent.parent_id) : undefined;
  }
  return depth;
}
