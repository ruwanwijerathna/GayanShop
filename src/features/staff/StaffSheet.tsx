import * as React from "react";
import { ShieldCheck, UserPlus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { SelectField } from "@/components/common/SelectField";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Switch } from "@/components/ui/switch";
import type { Perm, Role, StaffMember_Serialize as StaffMember } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useCreateStaff, useUpdateStaff } from "@/db/hooks";
import { translateError } from "@/i18n";
import { cn } from "@/lib/utils";
import { initials, isOwnerRole, limitText, minPin, parseLimit, PERMISSION_GROUPS } from "./permissions";

const FIELD = "h-12 text-base";

/** Create / edit a staff member in a right-hand sheet (doc §3.5; `USER_MANAGE`). */
export function StaffSheet({
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
  const [name, setName] = React.useState(member?.display_name ?? "");
  const [roleId, setRoleId] = React.useState(member?.role_id ?? cashier?.id ?? "");
  const [language, setLanguage] = React.useState(member?.ui_language === "si" ? "si" : "en");
  const [active, setActive] = React.useState(member?.is_active ?? true);
  const [pin, setPin] = React.useState("");
  const [limit, setLimit] = React.useState(limitText(member?.max_discount_bp ?? null));
  const [grants, setGrants] = React.useState<Perm[]>(member?.grants ?? []);
  const [denies, setDenies] = React.useState<Perm[]>(member?.denies ?? []);

  const role = roles.find((r) => r.id === roleId);
  const ownerRole = isOwnerRole(role);
  const locked = member?.last_owner === true;
  const limitBp = parseLimit(limit);
  const pending = create.isPending || update.isPending;
  const error = [create.error, update.error].find((e) => e instanceof CommandError);
  const overrideCount = ownerRole ? 0 : grants.length + denies.length;

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

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (limitBp === undefined) return;
    const input = {
      display_name: name,
      role_id: roleId,
      ui_language: language,
      grants: ownerRole ? [] : grants,
      denies: ownerRole ? [] : denies,
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
    <Sheet open onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-full gap-0 p-0 sm:max-w-2xl">
        <SheetHeader className="border-b bg-gradient-to-b from-muted/50 to-transparent px-6 py-5">
          <div className="flex items-center gap-4 pr-8">
            <Avatar className="size-12 ring-1 ring-primary/15">
              <AvatarFallback className="bg-primary/10 text-base font-semibold text-primary">
                {member ? initials(name || member.display_name) : <UserPlus aria-hidden className="size-5" />}
              </AvatarFallback>
            </Avatar>
            <div className="min-w-0">
              <SheetTitle className="truncate text-xl tracking-tight">
                {member ? t("form.editTitle", { name: member.display_name }) : t("form.createTitle")}
              </SheetTitle>
              <SheetDescription className="truncate">{role?.name ?? t("title")}</SheetDescription>
            </div>
          </div>
        </SheetHeader>

        <form onSubmit={submit} className="flex min-h-0 flex-1 flex-col">
          <div className="flex min-h-0 flex-1 flex-col gap-8 overflow-y-auto px-6 py-6">
            <section className="flex flex-col gap-4" aria-labelledby="staff-details">
              <h3
                id="staff-details"
                className="text-xs font-semibold uppercase tracking-wider text-muted-foreground"
              >
                {t("form.details")}
              </h3>
              <div className="grid gap-4 sm:grid-cols-2">
                <div className="flex flex-col gap-2 sm:col-span-2">
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
                      className={cn(FIELD, "tracking-[0.3em]")}
                      type="password"
                      inputMode="numeric"
                      autoComplete="new-password"
                      maxLength={8}
                      value={pin}
                      onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
                    />
                  </div>
                )}
                {!ownerRole && (
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
              </div>

              <div
                className={cn(
                  "flex min-h-14 items-center justify-between gap-3 rounded-xl border px-4 transition-colors",
                  active ? "border-emerald-500/30 bg-emerald-500/5" : "bg-muted/30",
                  locked && "opacity-70",
                )}
              >
                <Label htmlFor="staff-active" className="flex-1 cursor-pointer text-base">
                  {t("form.activeLabel")}
                </Label>
                <Switch id="staff-active" checked={active} disabled={locked} onCheckedChange={setActive} />
              </div>
              {locked && (
                <p
                  id="staff-locked"
                  className="flex items-start gap-2 rounded-xl border border-amber-500/30 bg-amber-500/5 px-4 py-3 text-sm text-muted-foreground"
                >
                  <ShieldCheck aria-hidden className="mt-0.5 size-5 shrink-0 text-amber-500" />
                  {t("form.lastOwner")}
                </p>
              )}
            </section>

            <fieldset className="flex flex-col gap-4">
              <legend className="mb-1 flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                {t("form.overrides")}
                {overrideCount > 0 && (
                  <span className="rounded-full bg-primary/10 px-2 py-0.5 text-xs font-semibold tabular-nums text-primary normal-case tracking-normal">
                    {overrideCount}
                  </span>
                )}
              </legend>
              {ownerRole ? (
                <p className="rounded-xl border bg-muted/30 px-4 py-3 text-sm text-muted-foreground">
                  {t("form.ownerAll")}
                </p>
              ) : (
                <>
                  <p className="text-sm text-muted-foreground">{t("form.overridesHint")}</p>
                  {PERMISSION_GROUPS.map((group) => (
                    <div key={group.id} className="flex flex-col gap-2">
                      <h4 className="text-sm font-semibold">{t(`groups.${group.id}`)}</h4>
                      <ul className="grid gap-2 sm:grid-cols-2">
                        {group.perms.map((p) => {
                          const fromRole = role?.permissions.includes(p) ?? false;
                          const denied = denies.includes(p);
                          const granted = grants.includes(p);
                          return (
                            <li
                              key={p}
                              className={cn(
                                "flex min-h-12 items-center gap-2 rounded-xl border px-3 transition-colors",
                                denied && "border-destructive/40 bg-destructive/5",
                                granted && "border-primary/40 bg-primary/5",
                                !denied && !granted && fromRole && "bg-muted/40",
                              )}
                            >
                              <span
                                className={cn(
                                  "flex-1 text-sm",
                                  denied && "text-muted-foreground line-through",
                                )}
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
                    </div>
                  ))}
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

          <div className="flex justify-end gap-2 border-t bg-background/85 px-6 py-4 backdrop-blur supports-[backdrop-filter]:bg-background/70">
            <Button type="button" variant="ghost" size="pos" className="rounded-xl" onClick={onClose}>
              {t("form.cancel")}
            </Button>
            <Button
              type="submit"
              size="pos"
              className="rounded-xl px-8 shadow-sm"
              disabled={pending || limitBp === undefined}
            >
              {member ? t("form.save") : t("form.create")}
            </Button>
          </div>
        </form>
      </SheetContent>
    </Sheet>
  );
}
