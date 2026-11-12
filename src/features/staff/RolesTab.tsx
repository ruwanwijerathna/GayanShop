import * as React from "react";
import { Check, Lock, Minus, Shield } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import type { Role, StaffMember_Serialize as StaffMember } from "@/db/bindings";
import { ALL_PERMS, isOwnerRole, limitText, PERMISSION_GROUPS } from "./permissions";

/** Role overview cards plus the permission matrix. */
export function RolesTab({ roles, staff }: { roles: Role[]; staff: StaffMember[] }) {
  const { t } = useTranslation("staff");
  return (
    <div className="flex flex-col gap-6">
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {roles.map((role, i) => {
          const members = staff.filter((s) => s.role_id === role.id).length;
          const owner = isOwnerRole(role);
          const permCount = owner ? ALL_PERMS.length : role.permissions.length;
          const pct = Math.round((permCount / ALL_PERMS.length) * 100);
          return (
            <Card
              key={role.id}
              className="gap-4 p-5 transition-shadow hover:shadow-md motion-safe:animate-in motion-safe:fade-in-0 motion-safe:slide-in-from-bottom-1"
              style={{ animationDelay: `${Math.min(i, 8) * 40}ms`, animationFillMode: "backwards" }}
            >
              <CardHeader className="gap-1 p-0">
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
                <CardDescription className="text-xs">
                  {t("rolesTab.maxDiscount", { pct: limitText(role.max_discount_bp) || "0" })} ·{" "}
                  {t("rolesTab.pinLength", { count: role.min_pin_len })}
                </CardDescription>
              </CardHeader>
              <CardContent className="flex flex-col gap-2 p-0">
                <div
                  className="h-1.5 overflow-hidden rounded-full bg-muted"
                  role="progressbar"
                  aria-valuemin={0}
                  aria-valuemax={ALL_PERMS.length}
                  aria-valuenow={permCount}
                  aria-label={t("rolesTab.permsCount", { count: permCount, total: ALL_PERMS.length })}
                >
                  <div
                    className={owner ? "h-full bg-amber-500" : "h-full bg-primary"}
                    style={{ width: `${pct}%` }}
                  />
                </div>
                <div className="flex items-center justify-between text-xs text-muted-foreground">
                  <span>{t("rolesTab.permsCount", { count: permCount, total: ALL_PERMS.length })}</span>
                  {owner && (
                    <Badge variant="outline" className="border-amber-300 text-amber-600">
                      <Shield aria-hidden /> {t("rolesTab.fullAccess")}
                    </Badge>
                  )}
                </div>
              </CardContent>
            </Card>
          );
        })}
      </div>

      <Card className="gap-0 overflow-hidden py-0">
        <div className="border-b p-4 sm:p-5">
          <h2 className="text-lg font-semibold">{t("rolesTab.matrixTitle")}</h2>
          <p className="text-sm text-muted-foreground">{t("rolesTab.matrixSubtitle")}</p>
        </div>
        <div className="max-h-[32rem] overflow-auto">
          <Table>
            <TableHeader className="sticky top-0 z-10 bg-card shadow-[0_1px_0_var(--color-border)]">
              <TableRow className="hover:bg-transparent">
                <TableHead className="sticky left-0 min-w-64 bg-card font-semibold">
                  {t("columns.name")}
                </TableHead>
                {roles.map((r) => (
                  <TableHead key={r.id} className="min-w-28 text-center font-semibold">
                    {r.name}
                  </TableHead>
                ))}
              </TableRow>
            </TableHeader>
            <TableBody>
              {PERMISSION_GROUPS.map((group) => (
                <React.Fragment key={group.id}>
                  <TableRow className="bg-muted/40 text-xs font-semibold uppercase tracking-wider text-muted-foreground hover:bg-muted/40">
                    <TableCell colSpan={roles.length + 1} className="px-4 py-2">
                      {t(`groups.${group.id}`)}
                    </TableCell>
                  </TableRow>
                  {group.perms.map((p) => (
                    <TableRow key={p} className="group/row">
                      <TableCell className="sticky left-0 bg-card px-4 py-3 font-normal transition-colors group-hover/row:bg-muted">
                        <div className="text-sm font-medium">{t(`permissions.${p}`)}</div>
                        <div className="font-mono text-xs text-muted-foreground">{p}</div>
                      </TableCell>
                      {roles.map((r) => {
                        const on = isOwnerRole(r) || r.permissions.includes(p);
                        return (
                          <TableCell key={r.id} className="py-3 text-center">
                            {on ? (
                              <span className="mx-auto flex size-6 items-center justify-center rounded-full bg-emerald-500/15">
                                <Check
                                  className="size-4 text-emerald-600 dark:text-emerald-400"
                                  aria-label={t("rolesTab.allowed")}
                                />
                              </span>
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
