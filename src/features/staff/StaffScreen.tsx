import * as React from "react";
import {
  KeyRound,
  Pencil,
  Search,
  Shield,
  ShieldCheck,
  UserCheck,
  UserPlus,
  Users,
  UserX,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { SelectField } from "@/components/common/SelectField";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { StaffMember_Serialize as StaffMember } from "@/db/bindings";
import { useRoles, useStaffList, useUpdateStaff } from "@/db/hooks";
import { cn } from "@/lib/utils";
import { PinDialog } from "./PinDialog";
import { initials } from "./permissions";
import { RolesTab } from "./RolesTab";
import { StaffSheet } from "./StaffSheet";

export { ALL_PERMS, PERMISSION_GROUPS, parseLimit } from "./permissions";

type TabId = "staff" | "roles";

/** Staff management (doc §3.5; `USER_MANAGE`). Team overview, per-person editor and role permission matrix. */
export function StaffScreen() {
  const { t } = useTranslation("staff");
  const staff = useStaffList();
  const roles = useRoles();
  const update = useUpdateStaff();

  const [tab, setTab] = React.useState<TabId>("staff");
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
      { onSuccess: () => toast.success(t("saved")) },
    );
  };

  return (
    <section className="mx-auto flex max-w-6xl flex-col gap-6 p-4 sm:p-6">
      <header className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight sm:text-3xl">{t("title")}</h1>
          <p className="text-sm text-muted-foreground">{t("subtitle")}</p>
        </div>
        <Button
          size="pos"
          className="gap-2 rounded-xl shadow-sm"
          onClick={() => setEditing("new")}
          disabled={!roles.data}
        >
          <UserPlus aria-hidden />
          <span>{t("add")}</span>
        </Button>
      </header>

      <div className="grid gap-4 sm:grid-cols-3">
        <StatCard
          label={t("stats.total")}
          value={list.length}
          loading={loading}
          icon={<Users aria-hidden />}
          tone="primary"
        />
        <StatCard
          label={t("stats.active")}
          value={activeCount}
          loading={loading}
          icon={<UserCheck aria-hidden />}
          tone="success"
        />
        <StatCard
          label={t("stats.roles")}
          value={rolesList.length}
          loading={roles.isLoading}
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
          <Card className="gap-0 overflow-hidden py-0 shadow-sm">
            <div className="flex flex-col gap-3 border-b bg-muted/20 p-4 sm:flex-row sm:items-center">
              <div className="relative flex-1">
                <Search
                  aria-hidden
                  className="pointer-events-none absolute left-3.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
                />
                <Input
                  placeholder={t("searchPlaceholder")}
                  className="h-12 rounded-xl bg-background pl-10 text-base"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  aria-label={t("searchPlaceholder")}
                />
              </div>
              <SelectField
                className="sm:w-48"
                value={roleFilter}
                onValueChange={setRoleFilter}
                aria-label={t("filters.allRoles")}
                options={[
                  { value: "all", label: t("filters.allRoles") },
                  ...rolesList.map((r) => ({ value: r.id, label: r.name })),
                ]}
              />
              <SelectField
                className="sm:w-44"
                value={statusFilter}
                onValueChange={setStatusFilter}
                aria-label={t("filters.allStatuses")}
                options={[
                  { value: "all", label: t("filters.allStatuses") },
                  { value: "active", label: t("active") },
                  { value: "inactive", label: t("inactive") },
                ]}
              />
            </div>

            <div className="overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow className="hover:bg-transparent">
                    <TableHead className="px-4">{t("columns.name")}</TableHead>
                    <TableHead>{t("columns.role")}</TableHead>
                    <TableHead className="hidden md:table-cell">{t("columns.language")}</TableHead>
                    <TableHead>{t("columns.status")}</TableHead>
                    <TableHead className="px-4 text-right">{t("columns.actions")}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {loading && Array.from({ length: 3 }, (_, i) => <SkeletonRow key={i} />)}
                  {!loading &&
                    filtered.map((m, i) => (
                      <StaffRow
                        key={m.id}
                        member={m}
                        index={i}
                        onToggle={handleToggleActive}
                        onEdit={() => setEditing(m)}
                        onResetPin={() => setPinFor(m)}
                      />
                    ))}
                  {!loading && filtered.length === 0 && (
                    <TableRow className="hover:bg-transparent">
                      <TableCell colSpan={5} className="py-14">
                        <div className="flex flex-col items-center gap-2 text-center text-muted-foreground">
                          <span className="flex size-12 items-center justify-center rounded-full bg-muted">
                            <UserX aria-hidden className="size-6" />
                          </span>
                          <span>{t("emptySearch")}</span>
                        </div>
                      </TableCell>
                    </TableRow>
                  )}
                </TableBody>
              </Table>
            </div>
            {!loading && list.length > 0 && (
              <div className="border-t bg-muted/20 px-4 py-2.5 text-xs text-muted-foreground tabular-nums">
                {t("showing", { shown: filtered.length, total: list.length })}
              </div>
            )}
          </Card>
        </TabsContent>

        <TabsContent value="roles" className="motion-safe:animate-in motion-safe:fade-in-0">
          <RolesTab roles={rolesList} staff={list} />
        </TabsContent>
      </Tabs>

      {editing !== null && roles.data && (
        <StaffSheet
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

const TONES = {
  primary: "bg-primary/10 text-primary",
  success: "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
  warning: "bg-amber-500/10 text-amber-600 dark:text-amber-400",
} as const;

function StatCard({
  label,
  value,
  loading,
  icon,
  tone,
}: {
  label: string;
  value: number;
  loading: boolean;
  icon: React.ReactNode;
  tone: keyof typeof TONES;
}) {
  return (
    <Card className="flex-row items-center gap-4 px-5 py-4 shadow-sm transition-shadow hover:shadow-md">
      <span
        className={cn(
          "flex size-12 shrink-0 items-center justify-center rounded-xl [&_svg]:size-6",
          TONES[tone],
        )}
      >
        {icon}
      </span>
      <div className="min-w-0">
        <div className="truncate text-sm text-muted-foreground">{label}</div>
        {loading ? (
          <Skeleton className="mt-1 h-8 w-12" />
        ) : (
          <div className="text-3xl font-semibold leading-tight tabular-nums">{value}</div>
        )}
      </div>
    </Card>
  );
}

function StaffRow({
  member: m,
  index,
  onToggle,
  onEdit,
  onResetPin,
}: {
  member: StaffMember;
  index: number;
  onToggle: (member: StaffMember, next: boolean) => void;
  onEdit: () => void;
  onResetPin: () => void;
}) {
  const { t } = useTranslation("staff");
  return (
    <TableRow
      className="h-[4.5rem] transition-colors hover:bg-muted/40 motion-safe:animate-in motion-safe:fade-in-0 motion-safe:slide-in-from-bottom-1"
      style={{ animationDelay: `${Math.min(index, 10) * 30}ms`, animationFillMode: "backwards" }}
    >
      <TableCell className="px-4">
        <div className="flex items-center gap-3">
          <Avatar className={cn("size-11", !m.is_active && "opacity-60 grayscale")}>
            <AvatarFallback
              className={cn(
                "text-sm font-semibold",
                m.is_active ? "bg-primary/10 text-primary" : "bg-muted text-muted-foreground",
              )}
            >
              {initials(m.display_name)}
            </AvatarFallback>
          </Avatar>
          <div className="min-w-0">
            <div
              className={cn(
                "flex items-center gap-1.5 truncate font-medium",
                !m.is_active && "text-muted-foreground",
              )}
            >
              <span>{m.display_name}</span>
              {m.last_owner && (
                <ShieldCheck
                  className="size-4 shrink-0 text-amber-500"
                  aria-label={t("form.lastOwner")}
                  role="img"
                />
              )}
            </div>
          </div>
        </div>
      </TableCell>
      <TableCell>
        <Badge variant={m.is_owner ? "secondary" : "outline"} className="gap-1 px-2.5 py-1 font-medium">
          {m.is_owner && <Shield aria-hidden className="text-amber-600" />}
          {m.role_name}
        </Badge>
      </TableCell>
      <TableCell className="hidden text-muted-foreground md:table-cell">
        {t(`languages.${m.ui_language === "si" ? "si" : "en"}`)}
      </TableCell>
      <TableCell>
        <div className="flex items-center gap-3">
          <Switch
            checked={m.is_active}
            disabled={m.last_owner}
            onCheckedChange={(c) => onToggle(m, c)}
            aria-label={t(m.is_active ? "toggle.deactivate" : "toggle.activate", { name: m.display_name })}
          />
          <span
            className={cn(
              "inline-flex items-center gap-1.5 text-xs font-medium",
              m.is_active ? "text-emerald-600 dark:text-emerald-400" : "text-muted-foreground",
            )}
          >
            <span
              aria-hidden
              className={cn(
                "size-1.5 rounded-full",
                m.is_active ? "bg-emerald-500" : "bg-muted-foreground/50",
              )}
            />
            {m.is_active ? t("active") : t("inactive")}
          </span>
        </div>
      </TableCell>
      <TableCell className="px-4">
        <div className="flex justify-end gap-2">
          <Button
            variant="outline"
            size="pos"
            className="rounded-xl"
            onClick={onEdit}
            aria-label={`${t("edit")} ${m.display_name}`}
          >
            <Pencil aria-hidden />
            <span className="hidden lg:inline">{t("edit")}</span>
          </Button>
          <Button
            variant="outline"
            size="pos"
            className="rounded-xl"
            onClick={onResetPin}
            aria-label={`${t("resetPin")} ${m.display_name}`}
          >
            <KeyRound aria-hidden />
            <span className="hidden lg:inline">{t("resetPin")}</span>
          </Button>
        </div>
      </TableCell>
    </TableRow>
  );
}

function SkeletonRow() {
  return (
    <TableRow className="h-[4.5rem] hover:bg-transparent" aria-hidden>
      <TableCell className="px-4">
        <div className="flex items-center gap-3">
          <Skeleton className="size-11 rounded-full" />
          <Skeleton className="h-4 w-32" />
        </div>
      </TableCell>
      <TableCell>
        <Skeleton className="h-6 w-20 rounded-full" />
      </TableCell>
      <TableCell className="hidden md:table-cell">
        <Skeleton className="h-4 w-16" />
      </TableCell>
      <TableCell>
        <Skeleton className="h-6 w-24 rounded-full" />
      </TableCell>
      <TableCell className="px-4">
        <Skeleton className="ml-auto h-12 w-24 rounded-xl" />
      </TableCell>
    </TableRow>
  );
}
