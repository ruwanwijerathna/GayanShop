import { KeyRound, Pencil, UserPlus } from "lucide-react";
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

const FIELD = "h-12 text-base";

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

/** Staff management (doc §3.5; `USER_MANAGE`). The owner invariant is shown, and enforced by Rust. */
export function StaffScreen() {
  const { t } = useTranslation("staff");
  const staff = useStaffList();
  const roles = useRoles();
  const [editing, setEditing] = useState<StaffMember | "new" | null>(null);
  const [pinFor, setPinFor] = useState<StaffMember | null>(null);
  const list = staff.data ?? [];

  return (
    <section className="mx-auto flex max-w-5xl flex-col gap-4 p-6">
      <div className="flex items-center gap-3">
        <h1 className="flex-1 text-2xl font-semibold">{t("title")}</h1>
        <Button size="pos" onClick={() => setEditing("new")} disabled={!roles.data}>
          <UserPlus aria-hidden />
          {t("add")}
        </Button>
      </div>
      <table className="w-full text-base">
        <thead>
          <tr className="border-b text-left text-sm text-muted-foreground">
            <th className="py-2">{t("columns.name")}</th>
            <th>{t("columns.role")}</th>
            <th>{t("columns.language")}</th>
            <th>{t("columns.status")}</th>
            <th className="text-right">{t("columns.actions")}</th>
          </tr>
        </thead>
        <tbody>
          {list.map((m) => (
            <tr key={m.id} className="h-12 border-b">
              <td className="font-medium">{m.display_name}</td>
              <td>{m.role_name}</td>
              <td>{t(`languages.${m.ui_language === "si" ? "si" : "en"}`)}</td>
              <td>{m.is_active ? t("active") : t("inactive")}</td>
              <td>
                <div className="flex justify-end gap-2">
                  <Button
                    size="pos"
                    variant="outline"
                    onClick={() => setEditing(m)}
                    aria-label={`${t("edit")} ${m.display_name}`}
                  >
                    <Pencil aria-hidden />
                    {t("edit")}
                  </Button>
                  <Button
                    size="pos"
                    variant="outline"
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
      <DialogContent className="max-h-[90vh] max-w-2xl overflow-y-auto">
        <DialogHeader>
          <DialogTitle>
            {member ? t("form.editTitle", { name: member.display_name }) : t("form.createTitle")}
          </DialogTitle>
          <DialogDescription className="sr-only">{t("title")}</DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="flex flex-col gap-4">
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
                  className={FIELD}
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
            <label className="flex min-h-12 items-center gap-3 text-base sm:col-span-2">
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
            <p id="staff-locked" className="text-base text-muted-foreground">
              {t("form.lastOwner")}
            </p>
          )}

          <fieldset className="flex flex-col gap-2">
            <legend className="text-base font-medium">{t("form.overrides")}</legend>
            {isOwnerRole ? (
              <p className="text-sm text-muted-foreground">{t("form.ownerAll")}</p>
            ) : (
              <>
                <p className="text-sm text-muted-foreground">{t("form.overridesHint")}</p>
                <ul className="grid gap-1 sm:grid-cols-2">
                  {ALL_PERMS.map((p) => {
                    const fromRole = role?.permissions.includes(p) ?? false;
                    return (
                      <li key={p} className="flex min-h-12 items-center gap-2 rounded-md border px-2">
                        <span className="flex-1 text-sm">
                          {t(`permissions.${p}`)}
                          {fromRole && <span className="text-muted-foreground"> · {t("form.fromRole")}</span>}
                        </span>
                        {fromRole ? (
                          <label className="flex min-h-12 items-center gap-1 text-sm">
                            <Checkbox
                              className="size-6"
                              checked={denies.includes(p)}
                              onCheckedChange={() => toggle(p, denies, setDenies, grants, setGrants)}
                            />
                            {t("form.deny")}
                          </label>
                        ) : (
                          <label className="flex min-h-12 items-center gap-1 text-sm">
                            <Checkbox
                              className="size-6"
                              checked={grants.includes(p)}
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
            <p role="alert" className="text-base text-destructive">
              {translateError(i18n.t, error.error)}
            </p>
          )}
          <Button type="submit" size="pos" className="self-end" disabled={pending || limitBp === undefined}>
            {member ? t("form.save") : t("form.create")}
          </Button>
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
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("pin.title", { name: member.display_name })}</DialogTitle>
          <DialogDescription className="sr-only">{t("resetPin")}</DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <Label htmlFor="new-pin">{t("pin.label", { min: minPin(roles, member.role_id) })}</Label>
          <Input
            id="new-pin"
            className={FIELD}
            type="password"
            inputMode="numeric"
            autoComplete="new-password"
            maxLength={8}
            value={pin}
            onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
          />
          {reset.error instanceof CommandError && (
            <p role="alert" className="text-base text-destructive">
              {translateError(i18n.t, reset.error.error)}
            </p>
          )}
          <Button type="submit" size="pos" disabled={reset.isPending || pin.length === 0}>
            {t("pin.save")}
          </Button>
        </form>
      </DialogContent>
    </Dialog>
  );
}
