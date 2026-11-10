import { CircleAlert, LogIn, UserRound } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { PinPad } from "@/components/common/PinPad";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import type { LoginStaff } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useLogin, useLoginStaff, useTouchMode } from "@/db/hooks";
import { translateError } from "@/i18n";
import { cn } from "@/lib/utils";

const PIN_MAX = 8;

/** True when a key event belongs to a control that handles typing or Enter itself. */
function ownsKeys(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT", "BUTTON", "A"].includes(target.tagName);
}

/**
 * One-screen sign in: the staff picker and the PIN field sit together on a centred card. A lone staff
 * member is picked automatically. Digits typed anywhere go into the PIN and Enter signs in; touch screen
 * mode adds the on-screen number pad. Lockout and wrong-PIN messages come from Rust error codes.
 */
export function LoginScreen({
  storeName,
  wasLocked = false,
}: {
  storeName: string | null;
  wasLocked?: boolean;
}) {
  const { t, i18n } = useTranslation(["auth", "common"]);
  const staff = useLoginStaff();
  const login = useLogin();
  const touchMode = useTouchMode();
  const [picked, setPicked] = useState<LoginStaff | null>(null);
  const [pin, setPin] = useState("");
  const only = staff.data?.length === 1 ? staff.data[0] : null;
  const selected = picked ?? only ?? null;

  const error = login.error instanceof CommandError ? translateError(i18n.t, login.error.error) : null;
  const canSubmit = selected !== null && pin.length > 0 && !login.isPending;

  const submit = () => {
    if (!selected || pin.length === 0 || login.isPending) return;
    login.mutate({ staffId: selected.id, pin }, { onSettled: () => setPin("") });
  };

  const choose = (s: LoginStaff) => {
    setPicked(s);
    setPin("");
    login.reset();
  };

  // Typing digits anywhere on the screen enters the PIN; Enter signs in (keyboard-first, doc §1.9).
  const keys = useRef({ selected, pending: login.isPending, submit });
  useEffect(() => {
    keys.current = { selected, pending: login.isPending, submit };
  });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.ctrlKey || e.altKey || e.metaKey || ownsKeys(e.target)) return;
      const { selected: who, pending, submit: go } = keys.current;
      if (!who || pending) return;
      if (/^[0-9]$/.test(e.key)) {
        e.preventDefault();
        setPin((p) => (p.length < PIN_MAX ? p + e.key : p));
      } else if (e.key === "Backspace") {
        e.preventDefault();
        setPin((p) => p.slice(0, -1));
      } else if (e.key === "Enter") {
        e.preventDefault();
        go();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <main className="flex min-h-screen items-center justify-center bg-muted/40 p-6">
      <Card className="w-full max-w-xl gap-5">
        <CardHeader className="text-center">
          <CardTitle className="text-3xl">{storeName ?? t("common:app.name")}</CardTitle>
          <CardDescription className="text-base">{t("login.subtitle")}</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-5">
          {wasLocked && (
            <p role="status" className="rounded-md bg-warning px-4 py-2 text-warning-foreground">
              {t("login.locked")}
            </p>
          )}

          <section aria-labelledby="login-staff" className="flex flex-col gap-3">
            <h2 id="login-staff" className="text-lg font-semibold">
              {t("login.title")}
            </h2>
            {staff.data && staff.data.length === 0 && <p>{t("login.noStaff")}</p>}
            <ul className="grid max-h-64 grid-cols-2 gap-2 overflow-y-auto sm:grid-cols-3">
              {staff.data?.map((s) => {
                const active = selected?.id === s.id;
                return (
                  <li key={s.id}>
                    <Button
                      type="button"
                      size="pos"
                      variant={active ? "default" : "outline"}
                      aria-pressed={active}
                      className={cn(
                        "h-16 w-full flex-col items-start gap-0 px-3",
                        active && "ring-2 ring-ring",
                      )}
                      onClick={() => choose(s)}
                    >
                      <span className="flex max-w-full items-center gap-2 truncate text-base">
                        <UserRound aria-hidden className="shrink-0" /> {s.display_name}
                      </span>
                      <span
                        className={cn(
                          "text-sm font-normal",
                          active ? "text-primary-foreground/80" : "text-muted-foreground",
                        )}
                      >
                        {s.role_name}
                      </span>
                    </Button>
                  </li>
                );
              })}
            </ul>
          </section>

          <section className="flex flex-col items-center gap-3 border-t pt-5">
            <PinPad
              key={selected?.id ?? "none"}
              label={selected ? t("login.enterPin", { name: selected.display_name }) : t("login.pin")}
              value={pin}
              onChange={setPin}
              onSubmit={submit}
              maxLength={PIN_MAX}
              disabled={!selected || login.isPending}
              keypad={touchMode}
              className={touchMode ? "w-72" : "w-full max-w-sm"}
            />
            {!selected && staff.data && staff.data.length > 0 && (
              <p className="text-sm text-muted-foreground">{t("login.pickFirst")}</p>
            )}
            {error && (
              <Alert variant="destructive" className="max-w-sm border-destructive">
                <CircleAlert aria-hidden />
                <AlertDescription className="text-base">{error}</AlertDescription>
              </Alert>
            )}
            <Button
              type="button"
              size="pos"
              className={touchMode ? "w-72" : "w-full max-w-sm"}
              disabled={!canSubmit}
              onClick={submit}
            >
              <LogIn aria-hidden />
              {login.isPending ? t("login.signingIn") : t("login.submit")}
            </Button>
          </section>
        </CardContent>
      </Card>
    </main>
  );
}
