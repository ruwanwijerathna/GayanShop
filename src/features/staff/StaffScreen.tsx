import * as React from "react";
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  Check,
  ChartColumn,
  KeyRound,
  Lock,
  Minus,
  MoreVertical,
  Pencil,
  RotateCcw,
  Search,
  SearchX,
  Shield,
  ShieldCheck,
  UserCheck,
  UserPlus,
  Users,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  useReactTable,
  type ColumnDef,
  type SortingState,
} from "@tanstack/react-table";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { SelectField } from "@/components/common/SelectField";
import type { Perm, Role, StaffMember_Serialize as StaffMember } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useCreateStaff, useResetStaffPin, useRoles, useStaffList, useUpdateStaff } from "@/db/hooks";
import { translateError } from "@/i18n";
import { cn } from "@/lib/utils";
import { StaffSalesDialog } from "./StaffSalesDialog";

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

export const PERMISSION_GROUPS: {
  id: "sales" | "cash" | "inventory" | "reports" | "system";
  perms: Perm[];
}[] = [
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

/** Staff management (doc §3.5; `USER_MANAGE`). Modern team overview with shadcn data table. */
export function StaffScreen() {
  const { t } = useTranslation("staff");
  const staff = useStaffList();
  const roles = useRoles();
  const update = useUpdateStaff();

  const [tab, setTab] = React.useState<Tab>("staff");
  const [editing, setEditing] = React.useState<StaffMember | "new" | null>(null);
  const [viewingSales, setViewingSales] = React.useState<StaffMember | null>(null);

  const list = staff.data ?? [];
  const rolesList = roles.data ?? [];
  const loading = staff.isLoading;
  const activeCount = list.filter((s) => s.is_active).length;

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
        <StatCard label={t("stats.total")} value={list.length} icon={<Users aria-hidden />} tone="primary" />
        <StatCard
          label={t("stats.active")}
          value={activeCount}
          icon={<UserCheck aria-hidden />}
          tone="success"
        />
        <StatCard
          label={t("stats.roles")}
          value={rolesList.length}
          icon={<Shield aria-hidden />}
          tone="warning"
        />
      </div>

      <Tabs value={tab} onValueChange={(v) => setTab(v === "roles" ? "roles" : "staff")} className="gap-4">
        <TabsList className="h-12 self-start rounded-xl p-1">
          <TabsTrigger value="staff" className="h-full min-w-36 rounded-lg px-5 text-sm">
            <Users aria-hidden />
            {t("tabs.staff")}
          </TabsTrigger>
          <TabsTrigger value="roles" className="h-full min-w-36 rounded-lg px-5 text-sm">
            <Shield aria-hidden />
            {t("tabs.roles")}
          </TabsTrigger>
        </TabsList>
        <TabsContent value="staff" className="motion-safe:animate-in motion-safe:fade-in-0">
          <StaffDataTable
            data={list}
            roles={rolesList}
            loading={loading}
            onEdit={(m) => setEditing(m)}
            onViewSales={(m) => setViewingSales(m)}
            onToggleActive={handleToggleActive}
          />
        </TabsContent>
        <TabsContent value="roles" className="motion-safe:animate-in motion-safe:fade-in-0">
          <RolesTab roles={rolesList} staff={list} />
        </TabsContent>
      </Tabs>

      {viewingSales !== null && (
        <StaffSalesDialog member={viewingSales} onClose={() => setViewingSales(null)} />
      )}

      {editing !== null && roles.data && (
        <StaffDialog
          member={editing === "new" ? null : editing}
          roles={roles.data}
          onClose={() => setEditing(null)}
        />
      )}
    </section>
  );
}

const STAT_TONES = {
  primary: "bg-primary/10 text-primary",
  success: "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
  warning: "bg-amber-500/10 text-amber-600 dark:text-amber-400",
} as const;

function StatCard({
  label,
  value,
  icon,
  tone,
}: {
  label: string;
  value: number;
  icon: React.ReactNode;
  tone: keyof typeof STAT_TONES;
}) {
  return (
    <Card className="flex-row items-center gap-4 px-5 py-4 shadow-sm transition-shadow hover:shadow-md">
      <span
        className={cn(
          "flex size-12 shrink-0 items-center justify-center rounded-xl [&_svg]:size-6",
          STAT_TONES[tone],
        )}
      >
        {icon}
      </span>
      <div className="min-w-0">
        <div className="truncate text-sm text-muted-foreground">{label}</div>
        <div className="text-3xl font-semibold leading-tight tabular-nums">{value}</div>
      </div>
    </Card>
  );
}

interface StaffDataTableProps {
  data: StaffMember[];
  roles: Role[];
  loading: boolean;
  onEdit: (member: StaffMember) => void;
  onViewSales: (member: StaffMember) => void;
  onToggleActive: (member: StaffMember, nextActive: boolean) => void;
}

function StaffDataTable({ data, roles, loading, onEdit, onViewSales, onToggleActive }: StaffDataTableProps) {
  const { t } = useTranslation("staff");
  const [sorting, setSorting] = React.useState<SortingState>([]);
  const [query, setQuery] = React.useState("");
  const [roleFilter, setRoleFilter] = React.useState("all");
  const [statusFilter, setStatusFilter] = React.useState("all");

  const filteredData = React.useMemo(() => {
    const q = query.trim().toLowerCase();
    return data.filter((s) => {
      const matchesQuery =
        !q || s.display_name.toLowerCase().includes(q) || s.role_name.toLowerCase().includes(q);
      const matchesRole = roleFilter === "all" || s.role_id === roleFilter;
      const matchesStatus =
        statusFilter === "all" ||
        (statusFilter === "active" && s.is_active) ||
        (statusFilter === "inactive" && !s.is_active);
      return matchesQuery && matchesRole && matchesStatus;
    });
  }, [data, query, roleFilter, statusFilter]);

  const hasActiveFilters = query.trim() !== "" || roleFilter !== "all" || statusFilter !== "all";

  const resetFilters = () => {
    setQuery("");
    setRoleFilter("all");
    setStatusFilter("all");
  };

  const columns = React.useMemo<ColumnDef<StaffMember>[]>(
    () => [
      {
        accessorKey: "staff_no",
        id: "staffNo",
        header: () => <span className="font-semibold">{t("columns.staffNo")}</span>,
        cell: ({ row }) => (
          <span className="font-mono text-base tabular-nums">{row.original.staff_no ?? "—"}</span>
        ),
      },
      {
        accessorKey: "display_name",
        id: "name",
        header: ({ column }) => (
          <Button
            variant="ghost"
            size="sm"
            className="-ml-3 h-8 data-[state=open]:bg-accent font-semibold"
            onClick={() => column.toggleSorting(column.getIsSorted() === "asc")}
          >
            <span>{t("columns.name")}</span>
            {column.getIsSorted() === "asc" ? (
              <ArrowUp className="ml-2 size-3.5" />
            ) : column.getIsSorted() === "desc" ? (
              <ArrowDown className="ml-2 size-3.5" />
            ) : (
              <ArrowUpDown className="ml-2 size-3.5 text-muted-foreground" />
            )}
          </Button>
        ),
        cell: ({ row }) => {
          const m = row.original;
          return (
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
          );
        },
      },
      {
        accessorKey: "role_name",
        id: "role",
        header: ({ column }) => (
          <Button
            variant="ghost"
            size="sm"
            className="-ml-3 h-8 data-[state=open]:bg-accent font-semibold"
            onClick={() => column.toggleSorting(column.getIsSorted() === "asc")}
          >
            <span>{t("columns.role")}</span>
            {column.getIsSorted() === "asc" ? (
              <ArrowUp className="ml-2 size-3.5" />
            ) : column.getIsSorted() === "desc" ? (
              <ArrowDown className="ml-2 size-3.5" />
            ) : (
              <ArrowUpDown className="ml-2 size-3.5 text-muted-foreground" />
            )}
          </Button>
        ),
        cell: ({ row }) => {
          const m = row.original;
          return (
            <Badge variant={m.is_owner ? "secondary" : "outline"} className="gap-1 font-medium">
              {m.is_owner && <Shield className="size-3 text-amber-600" />}
              {m.role_name}
            </Badge>
          );
        },
      },
      {
        accessorKey: "ui_language",
        id: "language",
        header: () => <span className="font-semibold">{t("columns.language")}</span>,
        cell: ({ row }) => (
          <span className="text-muted-foreground">
            {t(`languages.${row.original.ui_language === "si" ? "si" : "en"}`)}
          </span>
        ),
      },
      {
        accessorKey: "is_active",
        id: "status",
        header: ({ column }) => (
          <Button
            variant="ghost"
            size="sm"
            className="-ml-3 h-8 data-[state=open]:bg-accent font-semibold"
            onClick={() => column.toggleSorting(column.getIsSorted() === "asc")}
          >
            <span>{t("columns.status")}</span>
            {column.getIsSorted() === "asc" ? (
              <ArrowUp className="ml-2 size-3.5" />
            ) : column.getIsSorted() === "desc" ? (
              <ArrowDown className="ml-2 size-3.5" />
            ) : (
              <ArrowUpDown className="ml-2 size-3.5 text-muted-foreground" />
            )}
          </Button>
        ),
        cell: ({ row }) => {
          const m = row.original;
          return (
            <div className="flex items-center gap-2">
              <Switch
                checked={m.is_active}
                disabled={m.last_owner}
                onCheckedChange={(c) => onToggleActive(m, c)}
                aria-label={t(m.is_active ? "toggle.deactivate" : "toggle.activate", {
                  name: m.display_name,
                })}
              />
              <span className="text-xs text-muted-foreground">
                {m.is_active ? t("active") : t("inactive")}
              </span>
            </div>
          );
        },
      },
      {
        id: "actions",
        header: () => <div className="text-right font-semibold">{t("columns.actions")}</div>,
        cell: ({ row }) => {
          const m = row.original;
          return (
            <div className="flex justify-end">
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-11 w-11 rounded-lg hover:bg-muted"
                    aria-label={t("rowMenu", { name: m.display_name })}
                    title={t("rowMenu", { name: m.display_name })}
                  >
                    <MoreVertical className="size-5" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="min-w-52">
                  <DropdownMenuItem className="h-11 gap-2 text-base" onSelect={() => onViewSales(m)}>
                    <ChartColumn aria-hidden className="size-4" />
                    {t("sales.menu")}
                  </DropdownMenuItem>
                  <DropdownMenuItem className="h-11 gap-2 text-base" onSelect={() => onEdit(m)}>
                    <Pencil aria-hidden className="size-4" />
                    {t("edit")}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          );
        },
      },
    ],
    [t, onEdit, onViewSales, onToggleActive],
  );

  // eslint-disable-next-line react-hooks/incompatible-library
  const table = useReactTable({
    data: filteredData,
    columns,
    state: {
      sorting,
    },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
  });

  return (
    <Card className="gap-0 py-0 overflow-hidden">
      <div className="flex flex-col gap-3 border-b p-4 sm:flex-row sm:items-center">
        <div className="relative flex-1">
          <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            placeholder={t("searchPlaceholder")}
            className="h-11 pl-9 pr-9 text-base"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            aria-label={t("searchPlaceholder")}
          />
          {query && (
            <button
              type="button"
              onClick={() => setQuery("")}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
              aria-label="Clear search"
            >
              <X className="size-4" />
            </button>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Select value={roleFilter} onValueChange={setRoleFilter}>
            <SelectTrigger className="h-11 w-full sm:w-44" aria-label={t("filters.allRoles")}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t("filters.allRoles")}</SelectItem>
              {roles.map((r) => (
                <SelectItem key={r.id} value={r.id}>
                  {r.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Select value={statusFilter} onValueChange={setStatusFilter}>
            <SelectTrigger className="h-11 w-full sm:w-40" aria-label={t("filters.allStatuses")}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t("filters.allStatuses")}</SelectItem>
              <SelectItem value="active">{t("active")}</SelectItem>
              <SelectItem value="inactive">{t("inactive")}</SelectItem>
            </SelectContent>
          </Select>
          {hasActiveFilters && (
            <Button
              variant="ghost"
              size="sm"
              onClick={resetFilters}
              className="h-11 px-3 text-xs text-muted-foreground hover:text-foreground"
            >
              <RotateCcw className="mr-1.5 size-3.5" />
              {t("filters.reset")}
            </Button>
          )}
        </div>
      </div>

      <div className="overflow-x-auto">
        <Table>
          <TableHeader>
            {table.getHeaderGroups().map((headerGroup) => (
              <TableRow key={headerGroup.id} className="hover:bg-transparent">
                {headerGroup.headers.map((header) => (
                  <TableHead key={header.id}>
                    {header.isPlaceholder
                      ? null
                      : flexRender(header.column.columnDef.header, header.getContext())}
                  </TableHead>
                ))}
              </TableRow>
            ))}
          </TableHeader>
          <TableBody>
            {loading && Array.from({ length: 3 }, (_, i) => <SkeletonRow key={i} />)}
            {!loading &&
              table.getRowModel().rows?.length > 0 &&
              table.getRowModel().rows.map((row) => (
                <TableRow key={row.id} className="h-16 hover:bg-muted/40 transition-colors">
                  {row.getVisibleCells().map((cell) => (
                    <TableCell key={cell.id}>
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </TableCell>
                  ))}
                </TableRow>
              ))}
            {!loading && table.getRowModel().rows?.length === 0 && (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={columns.length} className="h-48 text-center">
                  <div className="flex flex-col items-center justify-center gap-2 text-muted-foreground">
                    <SearchX className="size-8 stroke-1 text-muted-foreground/60" />
                    <p className="text-sm font-medium">{t("emptySearch")}</p>
                    {hasActiveFilters && (
                      <Button
                        variant="link"
                        size="sm"
                        onClick={resetFilters}
                        className="text-xs text-primary"
                      >
                        {t("filters.clearAll")}
                      </Button>
                    )}
                  </div>
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>

      <div className="flex items-center justify-between border-t px-4 py-3 text-xs text-muted-foreground">
        <span>
          {hasActiveFilters
            ? `${filteredData.length} of ${data.length} staff`
            : `${data.length} staff member${data.length === 1 ? "" : "s"}`}
        </span>
      </div>
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
                      <Lock
                        className="size-3.5 text-muted-foreground"
                        aria-label={t("rolesTab.systemRole")}
                      />
                    )}
                  </CardTitle>
                  <Badge variant="secondary">{t("rolesTab.members", { count: members })}</Badge>
                </div>
                <CardDescription className="text-xs pt-1">
                  {t("rolesTab.maxDiscount", { pct: limitText(role.max_discount_bp) || "0" })} ·{" "}
                  {t("rolesTab.pinLength", { count: role.min_pin_len })}
                </CardDescription>
              </CardHeader>
              <CardContent className="p-0 pt-3 flex items-center justify-between text-xs text-muted-foreground border-t">
                <span>{t("rolesTab.permsCount", { count: permCount, total: ALL_PERMS.length })}</span>
                {isOwner && (
                  <Badge variant="outline" className="text-amber-600 border-amber-300">
                    <Shield className="size-3 mr-1" /> {t("rolesTab.fullAccess")}
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
                <React.Fragment key={group.id}>
                  <TableRow className="bg-muted/40 hover:bg-muted/40 font-semibold text-xs text-muted-foreground uppercase tracking-wider">
                    <TableCell colSpan={roles.length + 1} className="py-2 px-4">
                      {t(`groups.${group.id}`)}
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
                              <Check
                                className="mx-auto size-4 text-emerald-600"
                                aria-label={t("rolesTab.allowed")}
                              />
                            ) : (
                              <Minus
                                className="mx-auto size-4 text-muted-foreground/40"
                                aria-label={t("rolesTab.notAllowed")}
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
        <div className="ml-auto h-9 w-9 animate-pulse rounded-lg bg-muted" />
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
  const resetPin = useResetStaffPin();

  const cashier = roles.find((r) => r.name === "Cashier") ?? roles.at(-1);
  const [name, setName] = React.useState(member?.display_name ?? "");
  const [roleId, setRoleId] = React.useState(member?.role_id ?? cashier?.id ?? "");
  const [active, setActive] = React.useState(member?.is_active ?? true);

  // New staff PIN
  const [pin, setPin] = React.useState("");
  const [confirmPin, setConfirmPin] = React.useState("");

  // Edit staff PIN reset
  const [resetPinOpen, setResetPinOpen] = React.useState(false);
  const [newPin, setNewPin] = React.useState("");
  const [confirmNewPin, setConfirmNewPin] = React.useState("");

  const locked = member?.last_owner === true;
  const pending = create.isPending || update.isPending || resetPin.isPending;
  const error = [create.error, update.error, resetPin.error].find((e) => e instanceof CommandError);
  const minDigits = minPin(roles, roleId);

  const pinMismatch = !member && confirmPin.length > 0 && pin !== confirmPin;
  const editPinMismatch = member && resetPinOpen && confirmNewPin.length > 0 && newPin !== confirmNewPin;

  const pinValid = member
    ? !resetPinOpen || (newPin.length >= minDigits && newPin === confirmNewPin)
    : pin.length >= minDigits && pin === confirmPin;

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    if (!pinValid) return;

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

    if (member) {
      if (resetPinOpen && newPin) {
        try {
          await resetPin.mutateAsync({ id: member.id, pin: newPin });
        } catch {
          return;
        }
      }
      update.mutate({ id: member.id, input }, { onSuccess: done });
    } else {
      create.mutate({ staff: input, pin }, { onSuccess: done });
    }
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
                  ? t("form.editSubtitle", "Update staff details, role, and login credentials.")
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
              <div className="grid gap-4 sm:grid-cols-2">
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
                <div className="flex flex-col gap-2">
                  <Label htmlFor="staff-confirm-pin">{t("form.confirmPin")}</Label>
                  <Input
                    id="staff-confirm-pin"
                    className={cn(FIELD, "tracking-[0.3em]")}
                    type="password"
                    inputMode="numeric"
                    autoComplete="new-password"
                    maxLength={8}
                    value={confirmPin}
                    placeholder="••••"
                    onChange={(e) => setConfirmPin(e.target.value.replace(/\D/g, ""))}
                    required
                  />
                  {pinMismatch && <p className="text-xs text-destructive">{t("form.pinMismatch")}</p>}
                </div>
              </div>
            )}

            {member && (
              <div className="rounded-xl border bg-muted/20 p-4 space-y-3">
                <div className="flex items-center justify-between gap-3">
                  <div className="flex items-center gap-2.5">
                    <KeyRound className="size-4 text-muted-foreground shrink-0" />
                    <div>
                      <p className="text-sm font-medium leading-none">{t("pin.sectionTitle")}</p>
                      <p className="text-xs text-muted-foreground mt-1">{t("pin.sectionHint")}</p>
                    </div>
                  </div>
                  <Button
                    type="button"
                    variant={resetPinOpen ? "secondary" : "outline"}
                    size="sm"
                    onClick={() => {
                      setResetPinOpen((v) => !v);
                      setNewPin("");
                      setConfirmNewPin("");
                    }}
                  >
                    {resetPinOpen
                      ? i18n.t("common:actions.cancel", { defaultValue: "Cancel" })
                      : t("pin.resetButton")}
                  </Button>
                </div>

                {resetPinOpen && (
                  <div className="grid gap-3 pt-2 sm:grid-cols-2">
                    <div className="flex flex-col gap-2">
                      <Label htmlFor="staff-edit-pin">{t("pin.label", { min: minDigits })}</Label>
                      <Input
                        id="staff-edit-pin"
                        className={cn(FIELD, "tracking-[0.3em]")}
                        type="password"
                        inputMode="numeric"
                        autoComplete="new-password"
                        maxLength={8}
                        value={newPin}
                        placeholder="••••"
                        onChange={(e) => setNewPin(e.target.value.replace(/\D/g, ""))}
                        required={resetPinOpen}
                      />
                    </div>
                    <div className="flex flex-col gap-2">
                      <Label htmlFor="staff-edit-confirm-pin">{t("pin.confirmLabel")}</Label>
                      <Input
                        id="staff-edit-confirm-pin"
                        className={cn(FIELD, "tracking-[0.3em]")}
                        type="password"
                        inputMode="numeric"
                        autoComplete="new-password"
                        maxLength={8}
                        value={confirmNewPin}
                        placeholder="••••"
                        onChange={(e) => setConfirmNewPin(e.target.value.replace(/\D/g, ""))}
                        required={resetPinOpen}
                      />
                      {editPinMismatch && <p className="text-xs text-destructive">{t("pin.mismatch")}</p>}
                    </div>
                  </div>
                )}
              </div>
            )}

            <div
              className={cn(
                "flex min-h-14 items-center justify-between rounded-xl border p-4 transition-colors",
                active ? "border-emerald-500/30 bg-emerald-500/5" : "bg-muted/30",
                locked && "opacity-70",
              )}
            >
              <div className="space-y-0.5">
                <Label htmlFor="staff-active" className="text-sm font-medium cursor-pointer">
                  {t("form.activeLabel")}
                </Label>
                <p className="text-xs text-muted-foreground">{active ? t("active") : t("inactive")}</p>
              </div>
              <Switch
                id="staff-active"
                checked={active}
                disabled={locked}
                onCheckedChange={setActive}
                aria-label={t("form.activeLabel")}
              />
            </div>

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
            <Button type="submit" size="pos" disabled={pending || !name.trim() || !pinValid}>
              {member ? t("form.save") : t("form.create")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
