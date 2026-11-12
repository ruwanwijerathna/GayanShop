import type { LoginStaff } from "@/db/bindings";

/** How a typed username is compared with a staff member's name: trimmed, spaces collapsed, any case. */
export function normalizeUsername(name: string): string {
  return name.trim().replace(/\s+/g, " ").toLocaleLowerCase();
}

export type StaffMatch = { staff: LoginStaff } | { problem: "notFound" | "ambiguous" };

/** The active staff member a typed username names, or why there isn't exactly one (names aren't unique). */
export function findStaff(staff: readonly LoginStaff[], username: string): StaffMatch {
  const want = normalizeUsername(username);
  const hits = staff.filter((s) => normalizeUsername(s.display_name) === want);
  const [first] = hits;
  if (!first || want.length === 0) return { problem: "notFound" };
  if (hits.length > 1) return { problem: "ambiguous" };
  return { staff: first };
}
