import type { Perm, Role } from "@/db/bindings";

/** Every permission a person-level override can touch, in bit order (§3.5). */
export const ALL_PERMS: Perm[] = [
  "SALE_CREATE",
  "QUICK_SALE",
  "PRICE_OVERRIDE",
  "LINE_DISCOUNT",
  "ORDER_DISCOUNT",
  "VOID_ORDER",
  "DRAWER_NO_SALE",
  "CASH_PAY_IN_OUT",
  "CASH_SESSION_MANAGE",
  "CASH_SESSION_FORCE_CLOSE",
  "REPRINT_RECEIPT",
  "STOCK_ADJUST",
  "STOCK_DAMAGE",
  "BATCH_MANAGE",
  "PRODUCT_MANAGE",
  "QUICK_SALE_PROMOTE",
  "LABEL_PRINT",
  "VIEW_COST",
  "REPORT_SALES",
  "REPORT_FINANCIAL",
  "EXPENSE_MANAGE",
  "SETTINGS_MANAGE",
  "USER_MANAGE",
  "AUDIT_VIEW",
  "SYNC_MANAGE",
  "BACKUP_RESTORE",
  "DATA_EXPORT",
  "CUSTOMER_VIEW",
  "BACKUP_CREATE",
];

export type PermissionGroupId = "sales" | "cash" | "inventory" | "reports" | "system";

/** `id` is the i18n key under `staff:groups.*`. */
export const PERMISSION_GROUPS: { id: PermissionGroupId; perms: Perm[] }[] = [
  {
    id: "sales",
    perms: [
      "SALE_CREATE",
      "QUICK_SALE",
      "PRICE_OVERRIDE",
      "LINE_DISCOUNT",
      "ORDER_DISCOUNT",
      "VOID_ORDER",
      "DRAWER_NO_SALE",
      "REPRINT_RECEIPT",
      "CUSTOMER_VIEW",
    ],
  },
  {
    id: "cash",
    perms: ["CASH_PAY_IN_OUT", "CASH_SESSION_MANAGE", "CASH_SESSION_FORCE_CLOSE"],
  },
  {
    id: "inventory",
    perms: [
      "PRODUCT_MANAGE",
      "STOCK_ADJUST",
      "STOCK_DAMAGE",
      "BATCH_MANAGE",
      "QUICK_SALE_PROMOTE",
      "LABEL_PRINT",
      "VIEW_COST",
    ],
  },
  {
    id: "reports",
    perms: ["REPORT_SALES", "REPORT_FINANCIAL", "EXPENSE_MANAGE", "AUDIT_VIEW"],
  },
  {
    id: "system",
    perms: [
      "USER_MANAGE",
      "SETTINGS_MANAGE",
      "SYNC_MANAGE",
      "BACKUP_CREATE",
      "BACKUP_RESTORE",
      "DATA_EXPORT",
    ],
  },
];

/** "5" / "12.5" → basis points; "" → the role's limit (null). Invalid → undefined. */
export function parseLimit(text: string): number | null | undefined {
  const s = text.trim();
  if (s === "") return null;
  const m = /^(\d{1,3})(?:\.(\d{1,2}))?$/.exec(s);
  if (!m) return undefined;
  const bp = Number.parseInt(m[1] ?? "0", 10) * 100 + Number.parseInt((m[2] ?? "").padEnd(2, "0") || "0", 10);
  return bp <= 10_000 ? bp : undefined;
}

export function limitText(bp: number | null): string {
  if (bp === null) return "";
  return bp % 100 === 0 ? String(bp / 100) : `${Math.trunc(bp / 100)}.${String(bp % 100).padStart(2, "0")}`;
}

export function initials(name: string): string {
  const letters = name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => Array.from(w)[0] ?? "");
  return letters.join("").toUpperCase() || "?";
}

export function minPin(roles: Role[], roleId: string): number {
  return roles.find((r) => r.id === roleId)?.min_pin_len ?? 4;
}

export function isOwnerRole(role: Role | undefined): boolean {
  return role?.is_system === true && role.name === "Owner";
}
