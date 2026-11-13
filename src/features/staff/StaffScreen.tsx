import * as React from "react";
import { Check, KeyRound, Lock, Minus, Pencil, Search, Shield, ShieldCheck, UserPlus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { SelectField } from "@/components/common/SelectField";
import type { Perm, Role, StaffMember_Serialize as StaffMember } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useCreateStaff, useResetStaffPin, useRoles, useStaffList, useUpdateStaff } from "@/db/hooks";
import { translateError } from "@/i18n";
import { cn } from "@/lib/utils";

const FIELD = "h-12 text-base";

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

export const PERMISSION_GROUPS: { name: string; perms: Perm[] }[] = [
  {
    name: "Sales & Till",
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
    name: "Cash Management",
    perms: ["CASH_PAY_IN_OUT", "CASH_SESSION_MANAGE", "CASH_SESSION_FORCE_CLOSE"],
  },
  {
    name: "Inventory & Products",
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
    name: "Reports & Financials",
    perms: ["REPORT_SALES", "REPORT_FINANCIAL", "EXPENSE_MANAGE", "AUDIT_VIEW"],
  },
  {
    name: "System Administration",
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

function limitText(bp: number | null): string {
  if (bp === null) return "";
  return bp % 100 === 0 ? String(bp / 100) : `${Math.trunc(bp / 100)}.${String(bp % 100).padStart(2, "0")}`;
}

function initials(name: string): string {
  const letters = name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => Array.from(w)[0] ?? "");
  return letters.join("").toUpperCase() || "?";
}

function minPin(roles: Role[], roleId: string): number {
  return roles.find((r) => r.id === roleId)?.min_pin_len ?? 4;
}

type Tab = "staff" | "roles";

/** Staff management (doc §3.5; `USER_MANAGE`). Modern team overview with role permission matrix. */
export function StaffScreen() {
  const { t } = useTranslation("staff");
  const staff = useStaffList();
  const roles = useRoles();
  const update = useUpdateStaff();

  const [tab, setTab] = React.useState<Tab>("staff");
  const [query, setQuery] = React.useState("");
  const [roleFilter, setRoleFilter] = React.useState("all");
  const [statusFilter, setStatusFilter] = React.useState("all");

  const [editing, setEditing] = React.useState<StaffMember | "new" | null>(null);
  const [pinFor, setPinFor] = React.useState<StaffMember | null>(null);

  const list = staff.data ?? [];
  const rolesList = roles.data ?? [];
  const loading = staff.isLoading;
  const activeCount = list.filter((s) => s.is_active).length;

  const filtered = list.filter((s) => {
    const q = query.trim().toLowerCase();
    const matchesQuery =
      !q || s.display_name.toLowerCase().includes(q) || s.role_name.toLowerCase().includes(q);
    const matchesRole = roleFilter === "all" || s.role_id === roleFilter;
    const matchesStatus =
      statusFilter === "all" ||
      (statusFilter === "active" && s.is_active) ||
      (statusFilter === "inactive" && !s.is_active);
    return matchesQuery && matchesRole && matchesStatus;
  });

  const handleToggleActive = (member: StaffMember, nextActive: boolean) => {
    if (member.last_owner && !nextActive) {
      toast.error(t("lastOwnerCannotDeactivate"));
      return;
    }
    update.mutate(
      {
        id: member.id,
        input: {
          display_name: member.display_name,
          role_id: member.role_id,
          ui_language: member.ui_language,
          grants: member.grants,
          denies: member.denies,
          max_discount_bp: member.max_discount_bp,
          is_active: nextActive,
        },
      },
      {
        onSuccess: () => toast.success(t("saved")),
      },
    );
  };

  return (
    <section className="flex w-full flex-1 flex-col gap-6">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">{t("title")}</h1>
          <p className="text-sm text-muted-foreground">{t("subtitle")}</p>
        </div>
        <Button size="pos" className="gap-2" onClick={() => setEditing("new")} disabled={!roles.data}>
          <UserPlus aria-hidden className="size-4" />
          <span>{t("add")}</span>
        </Button>
      </div>

      <div className="grid gap-4 sm:grid-cols-3">
        <StatCard label={t("stats.total")} value={list.length} />
        <StatCard label={t("stats.active")} value={activeCount} />
        <StatCard label={t("stats.roles")} value={rolesList.length} />
      </div>

      <div role="tablist" className="inline-flex self-start rounded-lg bg-muted p-1 text-sm">
        <button
          type="button"
          role="tab"
          aria-selected={tab === "staff"}
          onClick={() => setTab("staff")}
          className={cn(
            "rounded-md px-4 py-2 font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
            tab === "staff" ? "bg-background shadow-sm" : "text-muted-foreground hover:text-foreground",
          )}
        >
          {t("tabs.staff")}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === "roles"}
          onClick={() => setTab("roles")}
          className={cn(
            "rounded-md px-4 py-2 font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
            tab === "roles" ? "bg-background shadow-sm" : "text-muted-foreground hover:text-foreground",
          )}
        >
          {t("tabs.roles")}
        </button>
      </div>

      {tab === "staff" ? (
        <Card className="gap-0 py-0 overflow-hidden">
          <div className="flex flex-col gap-3 border-b p-4 sm:flex-row">
            <div className="relative flex-1">
              <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                placeholder={t("searchPlaceholder")}
                className="h-11 pl-9 text-base"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                aria-label={t("searchPlaceholder")}
              />
            </div>
            <Select value={roleFilter} onValueChange={setRoleFilter}>
              <SelectTrigger className="h-11 sm:w-44" aria-label={t("filters.allRoles")}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t("filters.allRoles")}</SelectItem>
                {rolesList.map((r) => (
                  <SelectItem key={r.id} value={r.id}>
                    {r.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Select value={statusFilter} onValueChange={setStatusFilter}>
              <SelectTrigger className="h-11 sm:w-40" aria-label={t("filters.allStatuses")}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t("filters.allStatuses")}</SelectItem>
                <SelectItem value="active">{t("active")}</SelectItem>
                <SelectItem value="inactive">{t("inactive")}</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow className="hover:bg-transparent">
                  <TableHead>{t("columns.name")}</TableHead>
                  <TableHead>{t("columns.role")}</TableHead>
                  <TableHead className="hidden sm:table-cell">{t("columns.language")}</TableHead>
                  <TableHead>{t("columns.status")}</TableHead>
                  <TableHead className="text-right">{t("columns.actions")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {loading && Array.from({ length: 3 }, (_, i) => <SkeletonRow key={i} />)}
                {!loading &&
                  filtered.map((m) => (
                    <TableRow key={m.id} className="h-16 hover:bg-muted/40 transition-colors">
                      <TableCell>
                        <div className="flex items-center gap-3">
                          <span
                            aria-hidden
                            className={cn(
                              "flex size-10 shrink-0 items-center justify-center rounded-full text-sm font-semibold transition-colors",
                              m.is_active ? "bg-primary/10 text-primary" : "bg-muted text-muted-foreground",
                            )}
                          >
                            {initials(m.display_name)}
                          </span>
                          <div className="min-w-0">
                            <div className="flex items-center gap-1.5 truncate font-medium">
                              <span>{m.display_name}</span>
                              {m.last_owner && (
                                <ShieldCheck
                                  className="size-4 shrink-0 text-amber-500"
                                  aria-label={t("form.lastOwner")}
                                  role="img"
                                >
                                  <title>{t("form.lastOwner")}</title>
                                </ShieldCheck>
                              )}
                            </div>
                            <div className="truncate text-xs text-muted-foreground">{m.role_name}</div>
                          </div>
                        </div>
                      </TableCell>
                      <TableCell>
                        <Badge variant={m.is_owner ? "secondary" : "outline"} className="gap-1 font-medium">
                          {m.is_owner && <Shield className="size-3 text-amber-600" />}
                          {m.role_name}
                        </Badge>
                      </TableCell>
                      <TableCell className="hidden text-muted-foreground sm:table-cell">
                        {t(`languages.${m.ui_language === "si" ? "si" : "en"}`)}
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center gap-2">
                          <Switch
                            checked={m.is_active}
                            disabled={m.last_owner}
                            onCheckedChange={(c) => handleToggleActive(m, c)}
                            aria-label={`${m.is_active ? "Deactivate" : "Activate"} ${m.display_name}`}
                          />
                          <span className="text-xs text-muted-foreground">
                            {m.is_active ? t("active") : t("inactive")}
                          </span>
                        </div>
                      </TableCell>
                      <TableCell>
                        <div className="flex justify-end gap-2">
                          <Button
                            variant="outline"
                            size="pos"
                            className="rounded-xl transition-all active:scale-[0.98]"
                            onClick={() => setEditing(m)}
                            aria-label={`${t("edit")} ${m.display_name}`}
                          >
                            <Pencil aria-hidden />
                            {t("edit")}
                          </Button>
                          <Button
                            variant="outline"
                            size="pos"
                            className="rounded-xl transition-all active:scale-[0.98]"
                            onClick={() => setPinFor(m)}
                            aria-label={`${t("resetPin")} ${m.display_name}`}
                          >
                            <KeyRound aria-hidden />
                            {t("resetPin")}
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                {!loading && filtered.length === 0 && (
                  <TableRow className="hover:bg-transparent">
                    <TableCell colSpan={5} className="py-12 text-center text-muted-foreground">
                      {t("emptySearch")}
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </div>
        </Card>
      ) : (
        <RolesTab roles={rolesList} staff={list} />
      )}

      {editing !== null && roles.data && (
        <StaffDialog
          member={editing === "new" ? null : editing}
          roles={roles.data}
          onClose={() => setEditing(null)}
        />
      )}
      {pinFor && roles.data && (
        <PinDialog member={pinFor} roles={roles.data} onClose={() => setPinFor(null)} />
      )}
    </section>
  );
}

function StatCard({ label, value }: { label: string; value: number }) {
  return (
    <Card className="gap-1 py-4">
      <CardHeader className="gap-0 py-0 px-5">
        <CardDescription>{label}</CardDescription>
      </CardHeader>
      <CardContent className="py-0 px-5 pt-1">
        <div className="text-3xl font-semibold tabular-nums">{value}</div>
      </CardContent>
    </Card>
  );
}

function RolesTab({ roles, staff }: { roles: Role[]; staff: StaffMember[] }) {
  const { t } = useTranslation("staff");
  return (
    <div className="space-y-6">
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
        {roles.map((role) => {
          const members = staff.filter((s) => s.role_id === role.id).length;
          const isOwner = role.is_system && role.name === "Owner";
          const permCount = isOwner ? ALL_PERMS.length : role.permissions.length;
          return (
            <Card key={role.id} className="gap-3 p-5">
              <CardHeader className="p-0">
                <div className="flex items-start justify-between gap-2">
                  <CardTitle className="flex items-center gap-2 text-lg">
                    {role.name}
                    {role.is_system && (
                      <Lock className="size-3.5 text-muted-foreground" aria-label="System role" />
                    )}
                  </CardTitle>
                  <Badge variant="secondary">{t("rolesTab.members", { count: members })}</Badge>
                </div>
                <CardDescription className="text-xs pt-1">
                  {t("rolesTab.maxDiscount", { pct: limitText(role.max_discount_bp) || "0" })} ·{" "}
                  {role.min_pin_len} digit PIN
                </CardDescription>
              </CardHeader>
              <CardContent className="p-0 pt-3 flex items-center justify-between text-xs text-muted-foreground border-t">
                <span>{t("rolesTab.permsCount", { count: permCount, total: ALL_PERMS.length })}</span>
                {isOwner && (
                  <Badge variant="outline" className="text-amber-600 border-amber-300">
                    <Shield className="size-3 mr-1" /> Full access
                  </Badge>
                )}
              </CardContent>
            </Card>
          );
        })}
      </div>

      <Card className="gap-0 py-0 overflow-hidden">
        <div className="border-b p-4 sm:p-5">
          <h2 className="text-lg font-semibold">{t("rolesTab.matrixTitle")}</h2>
          <p className="text-sm text-muted-foreground">{t("rolesTab.matrixSubtitle")}</p>
        </div>
        <div className="overflow-x-auto">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead className="min-w-64 font-semibold">{t("columns.name")}</TableHead>
                {roles.map((r) => (
                  <TableHead key={r.id} className="text-center min-w-28 font-semibold">
                    {r.name}
                  </TableHead>
                ))}
              </TableRow>
            </TableHeader>
            <TableBody>
              {PERMISSION_GROUPS.map((group) => (
                <React.Fragment key={group.name}>
                  <TableRow className="bg-muted/40 hover:bg-muted/40 font-semibold text-xs text-muted-foreground uppercase tracking-wider">
                    <TableCell colSpan={roles.length + 1} className="py-2 px-4">
                      {group.name}
                    </TableCell>
                  </TableRow>
                  {group.perms.map((p) => (
                    <TableRow key={p} className="hover:bg-muted/20">
                      <TableCell className="py-3 px-4 font-normal">
                        <div className="text-sm font-medium">{t(`permissions.${p}`)}</div>
                        <div className="text-xs font-mono text-muted-foreground">{p}</div>
                      </TableCell>
                      {roles.map((r) => {
                        const isOwner = r.is_system && r.name === "Owner";
                        const on = isOwner || r.permissions.includes(p);
                        return (
                          <TableCell key={r.id} className="text-center py-3">
                            {on ? (
                              <Check className="mx-auto size-4 text-emerald-600" aria-label="Allowed" />
                            ) : (
                              <Minus
                                className="mx-auto size-4 text-muted-foreground/40"
                                aria-label="Not allowed"
                              />
                            )}
                          </TableCell>
                        );
                      })}
                    </TableRow>
                  ))}
                </React.Fragment>
              ))}
            </TableBody>
          </Table>
        </div>
      </Card>
    </div>
  );
}

function SkeletonRow() {
  return (
    <tr className="h-16" aria-hidden>
      <td className="px-4">
        <div className="flex items-center gap-3">
          <div className="size-10 animate-pulse rounded-full bg-muted" />
          <div className="h-4 w-32 animate-pulse rounded bg-muted" />
        </div>
      </td>
      <td className="px-4">
        <div className="h-7 w-20 animate-pulse rounded-full bg-muted" />
      </td>
      <td className="hidden px-4 sm:table-cell">
        <div className="h-4 w-16 animate-pulse rounded bg-muted" />
      </td>
      <td className="px-4">
        <div className="h-7 w-20 animate-pulse rounded-full bg-muted" />
      </td>
      <td className="px-4">
        <div className="ml-auto h-12 w-40 animate-pulse rounded-xl bg-muted" />
      </td>
    </tr>
  );
}

function StaffDialog({
  member,
  roles,
  onClose,
}: {
  member: StaffMember | null;
  roles: Role[];
  onClose: () => void;
}) {
  const { t, i18n } = useTranslation(["staff", "common"]);
  const create = useCreateStaff();
  const update = useUpdateStaff();
  const cashier = roles.find((r) => r.name === "Cashier") ?? roles.at(-1);
  const [name, setName] = React.useState(member?.display_name ?? "");
  const [roleId, setRoleId] = React.useState(member?.role_id ?? cashier?.id ?? "");
  const [active, setActive] = React.useState(member?.is_active ?? true);
  const [pin, setPin] = React.useState("");

  const locked = member?.last_owner === true;
  const pending = create.isPending || update.isPending;
  const error = [create.error, update.error].find((e) => e instanceof CommandError);
  const minDigits = minPin(roles, roleId);

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    if (!member && pin.length < minDigits) return;

    const input = {
      display_name: name.trim(),
      role_id: roleId,
      ui_language: member?.ui_language ?? (i18n.resolvedLanguage === "si" ? "si" : "en"),
      grants: member?.grants ?? [],
      denies: member?.denies ?? [],
      max_discount_bp: member?.max_discount_bp ?? null,
      is_active: active,
    };
    const done = () => {
      toast.success(t("saved"));
      onClose();
    };
    if (member) update.mutate({ id: member.id, input }, { onSuccess: done });
    else create.mutate({ staff: input, pin }, { onSuccess: done });
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-md gap-0 overflow-hidden rounded-2xl p-0 sm:max-w-lg">
        <DialogHeader className="border-b bg-muted/30 px-6 py-5">
          <div className="flex items-center gap-3">
            <span
              aria-hidden
              className="flex size-11 shrink-0 items-center justify-center rounded-full bg-primary/10 text-base font-semibold text-primary"
            >
              {member ? initials(name || member.display_name) : <UserPlus className="size-5" />}
            </span>
            <div>
              <DialogTitle className="text-xl tracking-tight">
                {member ? t("form.editTitle", { name: member.display_name }) : t("form.createTitle")}
              </DialogTitle>
              <DialogDescription className="text-sm text-muted-foreground mt-0.5">
                {member
                  ? t("form.editSubtitle", "Update staff details and role.")
                  : t("form.createSubtitle", "Add a new staff member to sign in to the POS.")}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <form onSubmit={submit} className="flex flex-col">
          <div className="flex flex-col gap-5 px-6 py-6">
            <div className="flex flex-col gap-2">
              <Label htmlFor="staff-name">{t("form.name")}</Label>
              <Input
                id="staff-name"
                className={FIELD}
                maxLength={40}
                value={name}
                placeholder={t("form.namePlaceholder", "e.g. Kasun Perera")}
                onChange={(e) => setName(e.target.value)}
                required
              />
            </div>

            <div className="flex flex-col gap-2">
              <Label htmlFor="staff-role">{t("form.role")}</Label>
              <SelectField
                id="staff-role"
                value={roleId}
                disabled={locked}
                onValueChange={setRoleId}
                aria-describedby={locked ? "staff-locked" : undefined}
                options={roles.map((r) => ({ value: r.id, label: r.name }))}
              />
            </div>

            {!member && (
              <div className="flex flex-col gap-2">
                <Label htmlFor="staff-pin">{t("form.pin", { min: minDigits })}</Label>
                <Input
                  id="staff-pin"
                  className={cn(FIELD, "tracking-[0.3em]")}
                  type="password"
                  inputMode="numeric"
                  autoComplete="new-password"
                  maxLength={8}
                  value={pin}
                  placeholder="••••"
                  onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
                  required
                />
              </div>
            )}

            <label
              htmlFor="staff-active"
              className={cn(
                "flex min-h-12 cursor-pointer items-center gap-3 rounded-xl border px-4 text-base transition-colors",
                active ? "border-emerald-500/30 bg-emerald-500/5" : "bg-muted/30",
                locked && "cursor-not-allowed opacity-70",
              )}
            >
              <Checkbox
                id="staff-active"
                className="size-5"
                checked={active}
                disabled={locked}
                onCheckedChange={(v) => setActive(v === true)}
              />
              <span className="text-sm font-medium">{t("form.activeLabel")}</span>
            </label>

            {locked && (
              <p
                id="staff-locked"
                className="flex items-start gap-2 rounded-xl border border-amber-500/30 bg-amber-500/5 px-4 py-3 text-sm text-muted-foreground"
              >
                <ShieldCheck aria-hidden className="mt-0.5 size-4 shrink-0 text-amber-500" />
                {t("form.lastOwner")}
              </p>
            )}

            {error instanceof CommandError && (
              <p
                role="alert"
                className="rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-3 text-base text-destructive motion-safe:animate-in motion-safe:fade-in-0 motion-safe:slide-in-from-top-1"
              >
                {translateError(i18n.t, error.error)}
              </p>
            )}
          </div>

          <DialogFooter className="border-t bg-muted/20 px-6 py-4">
            <Button type="button" variant="outline" size="pos" onClick={onClose} disabled={pending}>
              {i18n.t("common:actions.cancel", { defaultValue: "Cancel" })}
            </Button>
            <Button
              type="submit"
              size="pos"
              disabled={pending || !name.trim() || (!member && pin.length < minDigits)}
            >
              {member ? t("form.save") : t("form.create")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function PinDialog({ member, roles, onClose }: { member: StaffMember; roles: Role[]; onClose: () => void }) {
  const { t, i18n } = useTranslation(["staff", "common"]);
  const reset = useResetStaffPin();
  const [pin, setPin] = React.useState("");
  const minDigits = minPin(roles, member.role_id);
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (pin.length < minDigits) return;
    reset.mutate(
      { id: member.id, pin },
      {
        onSuccess: () => {
          toast.success(t("pin.done", { name: member.display_name }));
          onClose();
        },
      },
    );
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="gap-0 overflow-hidden rounded-2xl p-0 sm:max-w-md">
        <DialogHeader className="border-b bg-muted/30 px-6 py-5">
          <div className="flex items-center gap-3">
            <span
              aria-hidden
              className="flex size-11 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary"
            >
              <KeyRound className="size-5" />
            </span>
            <div>
              <DialogTitle className="text-xl tracking-tight">
                {t("pin.title", { name: member.display_name })}
              </DialogTitle>
              <DialogDescription className="text-sm text-muted-foreground mt-0.5">
                {t("pin.subtitle", "Set a new login PIN")}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>
        <form onSubmit={submit}>
          <div className="flex flex-col gap-4 px-6 py-6">
            <Label htmlFor="new-pin">{t("pin.label", { min: minDigits })}</Label>
            <Input
              id="new-pin"
              className={cn(FIELD, "text-center text-2xl tracking-[0.5em]")}
              type="password"
              inputMode="numeric"
              autoComplete="new-password"
              maxLength={8}
              value={pin}
              onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
            />
            {reset.error instanceof CommandError && (
              <p
                role="alert"
                className="rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-3 text-base text-destructive motion-safe:animate-in motion-safe:fade-in-0 motion-safe:slide-in-from-top-1"
              >
                {translateError(i18n.t, reset.error.error)}
              </p>
            )}
          </div>
          <DialogFooter className="border-t bg-muted/20 px-6 py-4">
            <Button type="button" variant="outline" size="pos" onClick={onClose} disabled={reset.isPending}>
              {i18n.t("common:actions.cancel", { defaultValue: "Cancel" })}
            </Button>
            <Button type="submit" size="pos" disabled={reset.isPending || pin.length < minDigits}>
              {t("pin.save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
