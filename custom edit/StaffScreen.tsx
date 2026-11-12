/* eslint-disable */
import * as React from "react";
import { Check, Lock, Minus, Pencil, Plus, Search, Shield, Trash2, UserPlus, Users } from "lucide-react";

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
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { cn } from "@/lib/utils";
import { RoleDialog } from "./role-dialog";
import { StaffDialog } from "./staff-dialog";
import { useStaffStore, type Result } from "./store";
import { PERMISSIONS, type Role, type StaffMember } from "./types";

type Tab = "staff" | "roles";
type Notice = { type: "error" | "success"; text: string };

const initials = (name: string) =>
  name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((n) => n[0]!.toUpperCase())
    .join("");

export function StaffPage() {
  const store = useStaffStore();
  const { staff, roles } = store;

  const [tab, setTab] = React.useState<Tab>("staff");
  const [query, setQuery] = React.useState("");
  const [roleFilter, setRoleFilter] = React.useState("all");
  const [statusFilter, setStatusFilter] = React.useState("all");

  const [staffDialog, setStaffDialog] = React.useState<{ open: boolean; member: StaffMember | null }>({
    open: false,
    member: null,
  });
  const [roleDialog, setRoleDialog] = React.useState<{ open: boolean; role: Role | null }>({
    open: false,
    role: null,
  });
  const [confirm, setConfirm] = React.useState<{ kind: "staff" | "role"; id: string; label: string } | null>(
    null,
  );
  const [notice, setNotice] = React.useState<Notice | null>(null);

  React.useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(null), 4000);
    return () => clearTimeout(t);
  }, [notice]);

  const roleById = React.useMemo(() => new Map(roles.map((r) => [r.id, r])), [roles]);

  const filtered = staff.filter((s) => {
    const q = query.trim().toLowerCase();
    const matchesQuery = !q || [s.name, s.username, s.email].some((v) => v.toLowerCase().includes(q));
    return (
      matchesQuery &&
      (roleFilter === "all" || s.roleId === roleFilter) &&
      (statusFilter === "all" || s.status === statusFilter)
    );
  });

  const report = (r: Result, success?: string) =>
    setNotice(
      r.ok ? (success ? { type: "success", text: success } : null) : { type: "error", text: r.error },
    );

  function submitStaff(input: Parameters<typeof store.addStaff>[0]): Result {
    const member = staffDialog.member;
    const result = member ? store.updateStaff(member.id, input) : store.addStaff(input);
    if (result.ok)
      setNotice({ type: "success", text: member ? "Staff member updated." : "Staff member added." });
    return result;
  }

  function submitRole(role: Parameters<typeof store.saveRole>[0]): Result {
    const result = store.saveRole(role);
    if (result.ok) setNotice({ type: "success", text: role.id ? "Role updated." : "Role created." });
    return result;
  }

  function runConfirm() {
    if (!confirm) return;
    if (confirm.kind === "staff") report(store.removeStaff(confirm.id), "Staff member deleted.");
    else report(store.removeRole(confirm.id), "Role deleted.");
    setConfirm(null);
  }

  const activeCount = staff.filter((s) => s.status === "active").length;

  return (
    <div className="min-h-svh bg-muted/30">
      <header className="border-b bg-background">
        <div className="mx-auto flex h-14 max-w-6xl items-center justify-between px-4 sm:px-6">
          <div className="flex items-center gap-2 font-semibold">
            <span className="flex size-8 items-center justify-center rounded-lg bg-primary text-primary-foreground">
              <Users className="size-4" />
            </span>
            Acme POS
          </div>
          <a href="#/login" className="text-sm text-muted-foreground hover:text-foreground">
            Sign out
          </a>
        </div>
      </header>

      <main className="mx-auto max-w-6xl space-y-6 px-4 py-8 sm:px-6">
        <div className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
          <div>
            <h1 className="text-2xl font-semibold tracking-tight">Staff</h1>
            <p className="text-sm text-muted-foreground">
              Manage who can sign in to the POS and what they're allowed to do.
            </p>
          </div>
          {tab === "staff" ? (
            <Button onClick={() => setStaffDialog({ open: true, member: null })}>
              <UserPlus /> Add staff member
            </Button>
          ) : (
            <Button onClick={() => setRoleDialog({ open: true, role: null })}>
              <Plus /> New role
            </Button>
          )}
        </div>

        <div className="grid gap-4 sm:grid-cols-3">
          <Stat label="Total staff" value={staff.length} />
          <Stat label="Active" value={activeCount} />
          <Stat label="Roles" value={roles.length} />
        </div>

        {notice && (
          <div
            role={notice.type === "error" ? "alert" : "status"}
            className={cn(
              "rounded-md border px-3 py-2 text-sm",
              notice.type === "error"
                ? "border-destructive/30 bg-destructive/10 text-destructive"
                : "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-400",
            )}
          >
            {notice.text}
          </div>
        )}

        <div role="tablist" className="inline-flex rounded-lg bg-muted p-1 text-sm">
          {(["staff", "roles"] as const).map((t) => (
            <button
              key={t}
              role="tab"
              aria-selected={tab === t}
              onClick={() => setTab(t)}
              className={cn(
                "rounded-md px-4 py-1.5 font-medium transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
                tab === t ? "bg-background shadow-sm" : "text-muted-foreground hover:text-foreground",
              )}
            >
              {t === "staff" ? "Team members" : "Roles & permissions"}
            </button>
          ))}
        </div>

        {tab === "staff" ? (
          <Card className="gap-0 py-0">
            <div className="flex flex-col gap-3 border-b p-4 sm:flex-row">
              <div className="relative flex-1">
                <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input
                  placeholder="Search by name, username or email"
                  className="pl-9"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  aria-label="Search staff"
                />
              </div>
              <Select value={roleFilter} onValueChange={setRoleFilter}>
                <SelectTrigger className="sm:w-44" aria-label="Filter by role">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">All roles</SelectItem>
                  {roles.map((r) => (
                    <SelectItem key={r.id} value={r.id}>
                      {r.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Select value={statusFilter} onValueChange={setStatusFilter}>
                <SelectTrigger className="sm:w-40" aria-label="Filter by status">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">All statuses</SelectItem>
                  <SelectItem value="active">Active</SelectItem>
                  <SelectItem value="inactive">Inactive</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <Table>
              <TableHeader>
                <TableRow className="hover:bg-transparent">
                  <TableHead>Member</TableHead>
                  <TableHead>Role</TableHead>
                  <TableHead>Phone</TableHead>
                  <TableHead>Active</TableHead>
                  <TableHead className="text-right">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filtered.map((s) => (
                  <TableRow key={s.id}>
                    <TableCell>
                      <div className="flex items-center gap-3">
                        <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-primary/10 text-xs font-semibold text-primary">
                          {initials(s.name)}
                        </span>
                        <div className="min-w-0">
                          <div className="truncate font-medium">{s.name}</div>
                          <div className="truncate text-xs text-muted-foreground">
                            @{s.username} · {s.email}
                          </div>
                        </div>
                      </div>
                    </TableCell>
                    <TableCell>
                      <Badge variant={s.roleId === "owner" ? "warning" : "default"}>
                        {s.roleId === "owner" && <Shield className="size-3" />}
                        {roleById.get(s.roleId)?.name ?? "Unknown"}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-muted-foreground">{s.phone || "—"}</TableCell>
                    <TableCell>
                      <Switch
                        checked={s.status === "active"}
                        onCheckedChange={(c) => report(store.setStatus(s.id, c ? "active" : "inactive"))}
                        aria-label={`${s.status === "active" ? "Deactivate" : "Activate"} ${s.name}`}
                      />
                    </TableCell>
                    <TableCell>
                      <div className="flex justify-end gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={`Edit ${s.name}`}
                          onClick={() => setStaffDialog({ open: true, member: s })}
                        >
                          <Pencil />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={`Delete ${s.name}`}
                          className="text-destructive hover:text-destructive"
                          onClick={() => setConfirm({ kind: "staff", id: s.id, label: s.name })}
                        >
                          <Trash2 />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
                {filtered.length === 0 && (
                  <TableRow className="hover:bg-transparent">
                    <TableCell colSpan={5} className="py-12 text-center text-muted-foreground">
                      No staff match your filters.
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </Card>
        ) : (
          <RolesTab
            roles={roles}
            staff={staff}
            onEdit={(role) => setRoleDialog({ open: true, role })}
            onDelete={(role) => setConfirm({ kind: "role", id: role.id, label: role.name })}
          />
        )}
      </main>

      <StaffDialog
        open={staffDialog.open}
        onOpenChange={(open) => setStaffDialog((d) => ({ ...d, open }))}
        member={staffDialog.member}
        roles={roles}
        onSubmit={submitStaff}
      />
      <RoleDialog
        open={roleDialog.open}
        onOpenChange={(open) => setRoleDialog((d) => ({ ...d, open }))}
        role={roleDialog.role}
        onSubmit={submitRole}
      />

      <Dialog open={!!confirm} onOpenChange={(open) => !open && setConfirm(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Delete {confirm?.kind === "role" ? "role" : "staff member"}?</DialogTitle>
            <DialogDescription>
              <strong>{confirm?.label}</strong> will be permanently removed. This can't be undone.
              {confirm?.kind === "staff" && " To keep their history, deactivate them instead."}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirm(null)}>
              Cancel
            </Button>
            <Button className="bg-destructive text-white hover:bg-destructive/90" onClick={runConfirm}>
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <Card className="gap-1 py-4">
      <CardHeader className="gap-0">
        <CardDescription>{label}</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="text-3xl font-semibold tabular-nums">{value}</div>
      </CardContent>
    </Card>
  );
}

function RolesTab({
  roles,
  staff,
  onEdit,
  onDelete,
}: {
  roles: Role[];
  staff: StaffMember[];
  onEdit: (r: Role) => void;
  onDelete: (r: Role) => void;
}) {
  return (
    <div className="space-y-6">
      <div className="grid gap-4 md:grid-cols-2">
        {roles.map((role) => {
          const members = staff.filter((s) => s.roleId === role.id).length;
          return (
            <Card key={role.id} className="gap-4">
              <CardHeader>
                <div className="flex items-start justify-between gap-2">
                  <CardTitle className="flex items-center gap-2 text-lg">
                    {role.name}
                    {role.locked && (
                      <Lock className="size-3.5 text-muted-foreground" aria-label="Locked role" />
                    )}
                  </CardTitle>
                  <Badge variant="secondary">
                    {members} member{members === 1 ? "" : "s"}
                  </Badge>
                </div>
                <CardDescription>{role.description || "No description."}</CardDescription>
              </CardHeader>
              <CardContent className="flex items-center justify-between gap-2">
                <span className="text-sm text-muted-foreground">
                  {role.permissions.length} of {PERMISSIONS.length} permissions
                </span>
                {!role.locked && (
                  <div className="flex gap-1">
                    <Button variant="outline" size="sm" onClick={() => onEdit(role)}>
                      <Pencil /> Edit
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="size-8 text-destructive hover:text-destructive"
                      aria-label={`Delete ${role.name}`}
                      onClick={() => onDelete(role)}
                    >
                      <Trash2 />
                    </Button>
                  </div>
                )}
              </CardContent>
            </Card>
          );
        })}
      </div>

      <Card className="gap-0 py-0">
        <div className="border-b p-4">
          <h2 className="font-semibold">Permission matrix</h2>
          <p className="text-sm text-muted-foreground">What each role can do at a glance.</p>
        </div>
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead>Permission</TableHead>
              {roles.map((r) => (
                <TableHead key={r.id} className="text-center">
                  {r.name}
                </TableHead>
              ))}
            </TableRow>
          </TableHeader>
          <TableBody>
            {PERMISSIONS.map((p) => (
              <TableRow key={p.key}>
                <TableCell>
                  <div>{p.label}</div>
                  <div className="text-xs text-muted-foreground">{p.group}</div>
                </TableCell>
                {roles.map((r) => {
                  const on = r.permissions.includes(p.key);
                  return (
                    <TableCell key={r.id} className="text-center">
                      {on ? (
                        <Check className="mx-auto size-4 text-emerald-600" aria-label="Allowed" />
                      ) : (
                        <Minus className="mx-auto size-4 text-muted-foreground/40" aria-label="Not allowed" />
                      )}
                    </TableCell>
                  );
                })}
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </Card>
    </div>
  );
}
