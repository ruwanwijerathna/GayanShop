import { CircleAlert, LogIn } from "lucide-react";
import { useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { PinKeys } from "@/components/common/PinPad";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { CommandError } from "@/db/client";
import { useLogin, useLoginStaff, useTouchMode } from "@/db/hooks";
import { translateError } from "@/i18n";
import { findStaff } from "./username";

const PASSWORD_MAX = 8;

type Problem = "notFound" | "ambiguous";

/**
 * The sign-in form: username + password on a centred card, always in English. Enter in Username moves
 * to Password; Enter in Password signs in. A lone staff member's name is prefilled (still editable).
 * Touch screen mode adds the on-screen number pad. Lockout messages come from Rust error codes.
 */
export function LoginScreen({
  storeName,
  wasLocked = false,
}: {
  storeName: string | null;
  wasLocked?: boolean;
}) {
  // The sign-in form is English-only, whatever this counter's display language is.
  const { t } = useTranslation(["auth", "common", "errors"], { lng: "en" });
  const staff = useLoginStaff();
  const login = useLogin();
  const touchMode = useTouchMode();
  const ids = useId();
  const userRef = useRef<HTMLInputElement>(null);
  const passRef = useRef<HTMLInputElement>(null);
  const [typedUser, setTypedUser] = useState<string | null>(null);
  const [password, setPassword] = useState("");
  const [problem, setProblem] = useState<Problem | null>(null);

  const only = staff.data?.length === 1 ? staff.data[0] : undefined;
  const username = typedUser ?? only?.display_name ?? "";

  useEffect(() => userRef.current?.focus(), []);
  // After a refused sign-in, the (re-enabled) password field takes focus for the next try.
  useEffect(() => {
    if (login.isError) passRef.current?.focus();
  }, [login.isError]);

  const err = login.error instanceof CommandError ? login.error.error : null;
  const loginError = err
    ? err.kind === "INVALID_PIN"
      ? t("login.wrongPassword", { attempts: err.attempts_left })
      : translateError(t, err)
    : null;
  const message =
    problem === "notFound"
      ? t("login.userNotFound")
      : problem === "ambiguous"
        ? t("login.userAmbiguous")
        : loginError;
  const canSubmit = username.trim().length > 0 && password.length > 0 && !login.isPending && !!staff.data;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!canSubmit || !staff.data) return;
    const found = findStaff(staff.data, username);
    if ("problem" in found) {
      login.reset();
      setProblem(found.problem);
      userRef.current?.focus();
      return;
    }
    setProblem(null);
    login.mutate({ staffId: found.staff.id, pin: password }, { onSettled: () => setPassword("") });
  };

  const userKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key !== "Enter") return;
    e.preventDefault();
    passRef.current?.focus();
  };

  return (
    <main lang="en" className="flex min-h-screen items-center justify-center bg-muted/40 p-6">
      <Card className="w-full max-w-md gap-6">
        <CardHeader className="text-center">
          <CardTitle>
            <h1 className="text-3xl font-semibold">{storeName ?? t("common:app.name")}</h1>
          </CardTitle>
          <CardDescription className="text-base">{t("login.subtitle")}</CardDescription>
        </CardHeader>
        <CardContent>
          <form className="flex flex-col items-center gap-4" onSubmit={submit} noValidate>
            {wasLocked && (
              <p role="status" className="w-full rounded-md bg-warning px-4 py-2 text-warning-foreground">
                {t("login.locked")}
              </p>
            )}
            {staff.data && staff.data.length === 0 && <p className="w-full">{t("login.noStaff")}</p>}

            <div className="flex w-full flex-col gap-2">
              <Label htmlFor={`${ids}-user`} className="text-base">
                {t("login.username")}
              </Label>
              <Input
                id={`${ids}-user`}
                ref={userRef}
                type="text"
                autoComplete="username"
                autoCapitalize="none"
                spellCheck={false}
                placeholder={t("login.usernamePlaceholder")}
                value={username}
                disabled={login.isPending}
                aria-invalid={problem !== null}
                onChange={(e) => {
                  setTypedUser(e.target.value);
                  setProblem(null);
                  login.reset();
                }}
                onKeyDown={userKeyDown}
                className="h-12 text-base"
              />
            </div>

            <div className="flex w-full flex-col gap-2">
              <Label htmlFor={`${ids}-pass`} className="text-base">
                {t("login.password")}
              </Label>
              <Input
                id={`${ids}-pass`}
                ref={passRef}
                type="password"
                autoComplete="current-password"
                placeholder={t("login.passwordPlaceholder")}
                value={password}
                maxLength={PASSWORD_MAX}
                disabled={login.isPending}
                aria-invalid={loginError !== null}
                onChange={(e) => {
                  setPassword(e.target.value.slice(0, PASSWORD_MAX));
                  login.reset();
                }}
                className="h-12 text-base"
              />
            </div>

            {touchMode && (
              <div className="w-72">
                <PinKeys
                  value={password}
                  onChange={setPassword}
                  maxLength={PASSWORD_MAX}
                  disabled={login.isPending}
                  lng="en"
                />
              </div>
            )}

            {message && (
              <Alert variant="destructive" className="w-full border-destructive">
                <CircleAlert aria-hidden />
                <AlertDescription className="text-base">{message}</AlertDescription>
              </Alert>
            )}

            <Button type="submit" size="pos" className="w-full" disabled={!canSubmit}>
              <LogIn aria-hidden className="mr-2 h-4 w-4" />
              {login.isPending ? t("login.signingIn") : t("login.submit")}
            </Button>
          </form>
        </CardContent>
      </Card>
    </main>
  );
}
