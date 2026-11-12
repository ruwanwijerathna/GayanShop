import { KeyRound, Pencil, ShieldCheck, UserPlus } from "lucide-react";
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { Perm, Role, StaffMember_Serialize as StaffMember } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useCreateStaff, useResetStaffPin, useRoles, useStaffList, useUpdateStaff } from "@/db/hooks";
import { translateError } from "@/i18n";
import { SelectField } from "@/components/common/SelectField";
import { Checkbox } from "@/components/ui/checkbox";

const FIELD = "h-12 rounded-xl bg-background text-base shadow-xs transition-shadow";

/** Tiny class joiner so this file has no dependency beyond the imports above. */
const cx = (...parts: Array<string | false | null | undefined>) => parts.filter(Boolean).join(" ");

/** Every permission a person-level override can touch, in the doc's bit order (§3.5). */
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

/** Staff management (doc §3.5; `USER_MANAGE`). The owner invariant is shown, and enforced by Rust. */
export function StaffScreen() {
  const { t } = useTranslation("staff");
  const staff = useStaffList();
  const roles = useRoles();
  const [editing, setEditing] = useState<StaffMember | "new" | null>(null);
  const [pinFor, setPinFor] = useState<StaffMember | null>(null);
  const list = staff.data ?? [];
  const loading = staff.isLoading;

  return (
    <section className="mx-auto flex max-w-5xl flex-col gap-6 p-4 sm:p-6">
      <header className="flex items-center gap-3">
        <div className="flex flex-1 items-center gap-3">
          <h1 className="text-2xl font-semibold tracking-tight">{t("title")}</h1>
          {!loading && (
            <span className="rounded-full bg-muted px-2.5 py-0.5 text-sm font-medium tabular-nums text-muted-foreground">
              {list.length}
            </span>
          )}
        </div>
        <Button
          size="pos"
          className="rounded-xl shadow-sm transition-all hover:shadow-md active:scale-[0.98]"
          onClick={() => setEditing("new")}
          disabled={!roles.data}
        >
          <UserPlus aria-hidden />
          {t("add")}
        </Button>
      </header>

      <div className="overflow-hidden rounded-2xl border bg-card shadow-sm">
        <div className="overflow-x-auto">
          <table className="w-full text-base">
            <thead>
              <tr className="border-b bg-muted/40 text-left text-xs font-medium uppercase tracking-wide text-muted-foreground">
                <th className="px-4 py-3">{t("columns.name")}</th>
                <th className="px-4 py-3">{t("columns.role")}</th>
                <th className="hidden px-4 py-3 sm:table-cell">{t("columns.language")}</th>
                <th className="px-4 py-3">{t("columns.status")}</th>
                <th className="px-4 py-3 text-right">{t("columns.actions")}</th>
              </tr>
            </thead>
            <tbody className="divide-y">
              {loading && Array.from({ length: 4 }, (_, i) => <SkeletonRow key={i} />)}
              {list.map((m, i) => (
                <tr
                  key={m.id}
                  style={{ animationDelay: `${Math.min(i, 10) * 35}ms`, animationFillMode: "backwards" }}
                  className={cx(
                    "group h-16 transition-colors hover:bg-muted/40",
                    "motion-safe:animate-in motion-safe:fade-in-0 motion-safe:slide-in-from-bottom-1 motion-safe:duration-300",
                  )}
                >
                  <td className="px-4">
                    <div className="flex items-center gap-3">
                      <span
                        aria-hidden
                        className={cx(
                          "flex size-10 shrink-0 items-center justify-center rounded-full text-sm font-semibold transition-colors",
                          m.is_active
                            ? "bg-primary/10 text-primary group-hover:bg-primary/15"
                            : "bg-muted text-muted-foreground",
                        )}
                      >
                        {initials(m.display_name)}
                      </span>
                      <span className={cx("font-medium", !m.is_active && "text-muted-foreground")}>
                        {m.display_name}
                      </span>
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
                  </td>
                  <td className="px-4">
                    <span className="inline-flex items-center rounded-full border bg-background px-2.5 py-1 text-sm font-medium">
                      {m.role_name}
                    </span>
                  </td>
                  <td className="hidden px-4 text-muted-foreground sm:table-cell">
                    {t(`languages.${m.ui_language === "si" ? "si" : "en"}`)}
                  </td>
                  <td className="px-4">
                    <span
                      className={cx(
                        "inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-sm font-medium",
                        m.is_active
                          ? "bg-emerald-500/10 text-emerald-700 dark:text-emerald-400"
                          : "bg-muted text-muted-foreground",
                      )}
                    >
                      <span
                        aria-hidden
                        className={cx(
                          "size-1.5 rounded-full",
                          m.is_active ? "bg-emerald-500" : "bg-muted-foreground/50",
                        )}
                      />
                      {m.is_active ? t("active") : t("inactive")}
                    </span>
                  </td>
                  <td className="px-4">
                    <div className="flex justify-end gap-2">
                      <Button
                        size="pos"
                        variant="outline"
                        className="rounded-xl transition-all active:scale-[0.98]"
                        onClick={() => setEditing(m)}
                        aria-label={`${t("edit")} ${m.display_name}`}
                      >
                        <Pencil aria-hidden />
                        {t("edit")}
                      </Button>
                      <Button
                        size="pos"
                        variant="outline"
                        className="rounded-xl transition-all active:scale-[0.98]"
                        onClick={() => setPinFor(m)}
                        aria-label={`${t("resetPin")} ${m.display_name}`}
                      >
                        <KeyRound aria-hidden />
                        {t("resetPin")}
                      </Button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

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

function minPin(roles: Role[], roleId: string): number {
  return roles.find((r) => r.id === roleId)?.min_pin_len ?? 4;
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
  const { t, i18n } = useTranslation("staff");
  const create = useCreateStaff();
  const update = useUpdateStaff();
  const cashier = roles.find((r) => r.name === "Cashier") ?? roles.at(-1);
  const [name, setName] = useState(member?.display_name ?? "");
  const [roleId, setRoleId] = useState(member?.role_id ?? cashier?.id ?? "");
  const [language, setLanguage] = useState(member?.ui_language === "si" ? "si" : "en");
  const [active, setActive] = useState(member?.is_active ?? true);
  const [pin, setPin] = useState("");
  const [limit, setLimit] = useState(limitText(member?.max_discount_bp ?? null));
  const [grants, setGrants] = useState<Perm[]>(member?.grants ?? []);
  const [denies, setDenies] = useState<Perm[]>(member?.denies ?? []);

  const role = roles.find((r) => r.id === roleId);
  const isOwnerRole = role?.is_system === true && role.name === "Owner";
  const locked = member?.last_owner === true;
  const limitBp = parseLimit(limit);
  const pending = create.isPending || update.isPending;
  const error = [create.error, update.error].find((e) => e instanceof CommandError);
  const overrideCount = isOwnerRole ? 0 : grants.length + denies.length;

  const toggle = (
    perm: Perm,
    list: Perm[],
    set: (p: Perm[]) => void,
    other: Perm[],
    setOther: (p: Perm[]) => void,
  ) => {
    if (list.includes(perm)) set(list.filter((p) => p !== perm));
    else {
      set([...list, perm]);
      setOther(other.filter((p) => p !== perm));
    }
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (limitBp === undefined) return;
    const input = {
      display_name: name,
      role_id: roleId,
      ui_language: language,
      grants: isOwnerRole ? [] : grants,
      denies: isOwnerRole ? [] : denies,
      max_discount_bp: limitBp,
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
      <DialogContent className="max-h-[90vh] max-w-2xl gap-0 overflow-y-auto rounded-2xl p-0">
        <DialogHeader className="border-b bg-muted/30 px-6 py-5">
          <div className="flex items-center gap-3">
            <span
              aria-hidden
              className="flex size-11 shrink-0 items-center justify-center rounded-full bg-primary/10 text-base font-semibold text-primary"
            >
              {member ? initials(name || member.display_name) : <UserPlus className="size-5" />}
            </span>
            <DialogTitle className="text-xl tracking-tight">
              {member ? t("form.editTitle", { name: member.display_name }) : t("form.createTitle")}
            </DialogTitle>
          </div>
          <DialogDescription className="sr-only">{t("title")}</DialogDescription>
        </DialogHeader>

        <form onSubmit={submit} className="flex flex-col">
          <div className="flex flex-col gap-6 px-6 py-6">
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="flex flex-col gap-2">
                <Label htmlFor="staff-name">{t("form.name")}</Label>
                <Input
                  id="staff-name"
                  className={FIELD}
                  maxLength={40}
                  value={name}
                  onChange={(e) => setName(e.target.value)}
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
              <div className="flex flex-col gap-2">
                <Label htmlFor="staff-language">{t("form.language")}</Label>
                <SelectField
                  id="staff-language"
                  value={language}
                  onValueChange={(v) => setLanguage(v === "si" ? "si" : "en")}
                  options={[
                    { value: "en", label: t("languages.en") },
                    { value: "si", label: t("languages.si") },
                  ]}
                />
              </div>
              {!member && (
                <div className="flex flex-col gap-2">
                  <Label htmlFor="staff-pin">{t("form.pin", { min: minPin(roles, roleId) })}</Label>
                  <Input
                    id="staff-pin"
                    className={cx(FIELD, "tracking-[0.3em]")}
                    type="password"
                    inputMode="numeric"
                    autoComplete="new-password"
                    maxLength={8}
                    value={pin}
                    onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
                  />
                </div>
              )}
              {!isOwnerRole && (
                <div className="flex flex-col gap-2">
                  <Label htmlFor="staff-limit">{t("form.limit")}</Label>
                  <Input
                    id="staff-limit"
                    className={FIELD}
                    inputMode="decimal"
                    value={limit}
                    onChange={(e) => setLimit(e.target.value)}
                    aria-invalid={limitBp === undefined}
                    aria-describedby="staff-limit-hint"
                  />
                  <p id="staff-limit-hint" className="text-sm text-muted-foreground">
                    {t("form.limitHint", { pct: limitText(role?.max_discount_bp ?? 0) || "0" })}
                  </p>
                </div>
              )}
              <label
                className={cx(
                  "flex min-h-14 cursor-pointer items-center gap-3 rounded-xl border px-4 text-base transition-colors sm:col-span-2",
                  active ? "border-emerald-500/30 bg-emerald-500/5" : "bg-muted/30",
                  locked && "cursor-not-allowed opacity-70",
                )}
              >
                <Checkbox
                  className="size-6"
                  checked={active}
                  disabled={locked}
                  onCheckedChange={(v) => setActive(v === true)}
                />
                {t("form.activeLabel")}
              </label>
            </div>
            {locked && (
              <p
                id="staff-locked"
                className="flex items-start gap-2 rounded-xl border border-amber-500/30 bg-amber-500/5 px-4 py-3 text-base text-muted-foreground"
              >
                <ShieldCheck aria-hidden className="mt-0.5 size-5 shrink-0 text-amber-500" />
                {t("form.lastOwner")}
              </p>
            )}

            <fieldset className="flex flex-col gap-3">
              <legend className="flex items-center gap-2 text-base font-medium">
                {t("form.overrides")}
                {overrideCount > 0 && (
                  <span className="rounded-full bg-primary/10 px-2 py-0.5 text-xs font-semibold tabular-nums text-primary">
                    {overrideCount}
                  </span>
                )}
              </legend>
              {isOwnerRole ? (
                <p className="rounded-xl border bg-muted/30 px-4 py-3 text-sm text-muted-foreground">
                  {t("form.ownerAll")}
                </p>
              ) : (
                <>
                  <p className="text-sm text-muted-foreground">{t("form.overridesHint")}</p>
                  <ul className="grid gap-2 sm:grid-cols-2">
                    {ALL_PERMS.map((p) => {
                      const fromRole = role?.permissions.includes(p) ?? false;
                      const denied = denies.includes(p);
                      const granted = grants.includes(p);
                      return (
                        <li
                          key={p}
                          className={cx(
                            "flex min-h-12 items-center gap-2 rounded-xl border px-3 transition-colors",
                            denied && "border-destructive/40 bg-destructive/5",
                            granted && "border-primary/40 bg-primary/5",
                            !denied && !granted && fromRole && "bg-muted/40",
                          )}
                        >
                          <span
                            className={cx("flex-1 text-sm", denied && "text-muted-foreground line-through")}
                          >
                            {t(`permissions.${p}`)}
                            {fromRole && (
                              <span className="text-muted-foreground"> · {t("form.fromRole")}</span>
                            )}
                          </span>
                          {fromRole ? (
                            <label className="flex min-h-12 cursor-pointer items-center gap-1.5 text-sm font-medium">
                              <Checkbox
                                className="size-6"
                                checked={denied}
                                onCheckedChange={() => toggle(p, denies, setDenies, grants, setGrants)}
                              />
                              {t("form.deny")}
                            </label>
                          ) : (
                            <label className="flex min-h-12 cursor-pointer items-center gap-1.5 text-sm font-medium">
                              <Checkbox
                                className="size-6"
                                checked={granted}
                                onCheckedChange={() => toggle(p, grants, setGrants, denies, setDenies)}
                              />
                              {t("form.grant")}
                            </label>
                          )}
                        </li>
                      );
                    })}
                  </ul>
                </>
              )}
            </fieldset>

            {error instanceof CommandError && (
              <p
                role="alert"
                className="rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-3 text-base text-destructive motion-safe:animate-in motion-safe:fade-in-0 motion-safe:slide-in-from-top-1"
              >
                {translateError(i18n.t, error.error)}
              </p>
            )}
          </div>

          <div className="sticky bottom-0 flex justify-end border-t bg-background/85 px-6 py-4 backdrop-blur supports-[backdrop-filter]:bg-background/70">
            <Button
              type="submit"
              size="pos"
              className="rounded-xl px-8 shadow-sm transition-all active:scale-[0.98]"
              disabled={pending || limitBp === undefined}
            >
              {member ? t("form.save") : t("form.create")}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function PinDialog({ member, roles, onClose }: { member: StaffMember; roles: Role[]; onClose: () => void }) {
  const { t, i18n } = useTranslation("staff");
  const reset = useResetStaffPin();
  const [pin, setPin] = useState("");
  const submit = (e: FormEvent) => {
    e.preventDefault();
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
      <DialogContent className="gap-0 overflow-hidden rounded-2xl p-0">
        <DialogHeader className="border-b bg-muted/30 px-6 py-5">
          <div className="flex items-center gap-3">
            <span
              aria-hidden
              className="flex size-11 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary"
            >
              <KeyRound className="size-5" />
            </span>
            <DialogTitle className="text-xl tracking-tight">
              {t("pin.title", { name: member.display_name })}
            </DialogTitle>
          </div>
          <DialogDescription className="sr-only">{t("resetPin")}</DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="flex flex-col gap-4 px-6 py-6">
          <Label htmlFor="new-pin">{t("pin.label", { min: minPin(roles, member.role_id) })}</Label>
          <Input
            id="new-pin"
            className={cx(FIELD, "text-center text-2xl tracking-[0.5em]")}
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
          <Button
            type="submit"
            size="pos"
            className="rounded-xl shadow-sm transition-all active:scale-[0.98]"
            disabled={reset.isPending || pin.length === 0}
          >
            {t("pin.save")}
          </Button>
        </form>
      </DialogContent>
    </Dialog>
  );
}
