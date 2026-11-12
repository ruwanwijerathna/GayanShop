import {
  BarChart3,
  CircleAlert,
  Eye,
  EyeOff,
  Layers,
  Loader2,
  Lock,
  LogIn,
  ShieldCheck,
  User,
  Zap,
} from "lucide-react";
import { useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { PinKeys } from "@/components/common/PinPad";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { CommandError } from "@/db/client";
import { useLogin, useLoginStaff, useTouchMode } from "@/db/hooks";
import { translateError } from "@/i18n";
import { findStaff } from "./username";

const PASSWORD_MAX = 8;

type Problem = "notFound" | "ambiguous";

/**
 * Modern split-screen sign-in screen: branded POS panel on the right (desktop),
 * credentials form on the left with icon-enhanced inputs, password visibility toggle,
 * touch mode on-screen PIN keypad, and role/permission security.
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
  const [remember, setRemember] = useState(true);
  const [showPassword, setShowPassword] = useState(false);
  const [problem, setProblem] = useState<Problem | null>(null);
  const [infoMessage, setInfoMessage] = useState<string | null>(null);

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

  const displayName = storeName ?? t("common:app.name");

  return (
    <main lang="en" className="grid min-h-svh bg-background lg:grid-cols-2">
      {/* Form side */}
      <div className="flex flex-col justify-between overflow-y-auto px-6 py-8 sm:px-12">
        <div className="flex items-center gap-2 font-semibold">
          <span className="flex size-9 items-center justify-center rounded-lg bg-primary text-primary-foreground shadow-sm">
            <Layers className="size-5" />
          </span>
          <h1 className="text-xl font-bold tracking-tight">{displayName}</h1>
        </div>

        <div className="my-auto flex flex-1 items-center justify-center py-8">
          <div className="w-full max-w-sm">
            <div className="mb-8 space-y-2">
              <h2 className="text-3xl font-semibold tracking-tight">Welcome back</h2>
              <p className="text-sm text-muted-foreground">{t("login.subtitle")}</p>
            </div>

            <form onSubmit={submit} noValidate className="space-y-5">
              {wasLocked && (
                <p
                  role="status"
                  className="rounded-md border border-warning/30 bg-warning/15 px-3.5 py-2.5 text-sm text-warning-foreground"
                >
                  {t("login.locked")}
                </p>
              )}

              {staff.data && staff.data.length === 0 && (
                <p className="rounded-md border border-muted bg-muted/50 px-3.5 py-2 text-sm text-muted-foreground">
                  {t("login.noStaff")}
                </p>
              )}

              {infoMessage && (
                <div
                  role="status"
                  className="flex items-start justify-between rounded-md border border-primary/20 bg-primary/10 px-3.5 py-2.5 text-sm text-primary"
                >
                  <span>{infoMessage}</span>
                  <button
                    type="button"
                    onClick={() => setInfoMessage(null)}
                    className="ml-2 font-semibold hover:underline"
                    aria-label="Dismiss info"
                  >
                    ×
                  </button>
                </div>
              )}

              {message && (
                <Alert variant="destructive" className="border-destructive">
                  <CircleAlert aria-hidden />
                  <AlertDescription className="text-sm">{message}</AlertDescription>
                </Alert>
              )}

              <div className="space-y-2">
                <Label htmlFor={`${ids}-user`}>{t("login.username")}</Label>
                <div className="relative">
                  <User className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    id={`${ids}-user`}
                    ref={userRef}
                    type="text"
                    autoComplete="username"
                    autoCapitalize="none"
                    spellCheck={false}
                    placeholder={t("login.usernamePlaceholder")}
                    className="h-12 pl-9 text-base"
                    value={username}
                    disabled={login.isPending}
                    aria-invalid={problem !== null}
                    aria-describedby={problem ? `${ids}-user-error` : undefined}
                    onChange={(e) => {
                      setTypedUser(e.target.value);
                      setProblem(null);
                      login.reset();
                    }}
                    onKeyDown={userKeyDown}
                  />
                </div>
              </div>

              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <Label htmlFor={`${ids}-pass`}>{t("login.password")}</Label>
                  <button
                    type="button"
                    onClick={() =>
                      setInfoMessage(
                        "Forgot your password or PIN? Please ask the store owner or manager to reset it in the Staff screen.",
                      )
                    }
                    className="text-xs font-medium text-primary hover:underline"
                  >
                    Forgot password?
                  </button>
                </div>
                <div className="relative">
                  <Lock className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    id={`${ids}-pass`}
                    ref={passRef}
                    type={showPassword ? "text" : "password"}
                    autoComplete="current-password"
                    placeholder={t("login.passwordPlaceholder")}
                    className="h-12 pl-9 pr-10 text-base"
                    value={password}
                    maxLength={PASSWORD_MAX}
                    disabled={login.isPending}
                    aria-invalid={loginError !== null}
                    aria-describedby={loginError ? `${ids}-pass-error` : undefined}
                    onChange={(e) => {
                      setPassword(e.target.value.slice(0, PASSWORD_MAX));
                      login.reset();
                    }}
                  />
                  <button
                    type="button"
                    onClick={() => setShowPassword((s) => !s)}
                    aria-label={showPassword ? "Hide password" : "Show password"}
                    className="absolute right-2.5 top-1/2 -translate-y-1/2 rounded p-1 text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    {showPassword ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
                  </button>
                </div>
              </div>

              {touchMode && (
                <div className="flex justify-center pt-2">
                  <div className="w-72">
                    <PinKeys
                      value={password}
                      onChange={setPassword}
                      maxLength={PASSWORD_MAX}
                      disabled={login.isPending}
                      lng="en"
                    />
                  </div>
                </div>
              )}

              <div className="flex items-center gap-2">
                <Checkbox
                  id={`${ids}-remember`}
                  checked={remember}
                  onCheckedChange={(v) => setRemember(v === true)}
                />
                <Label htmlFor={`${ids}-remember`} className="font-normal text-muted-foreground">
                  Keep me signed in
                </Label>
              </div>

              <Button type="submit" size="pos" className="w-full" disabled={!canSubmit}>
                {login.isPending ? (
                  <>
                    <Loader2 className="mr-2 size-4 animate-spin" />
                    {t("login.signingIn")}
                  </>
                ) : (
                  <>
                    <LogIn className="mr-2 size-4" />
                    {t("login.submit")}
                  </>
                )}
              </Button>
            </form>

            <p className="mt-8 text-center text-sm text-muted-foreground">
              Don&apos;t have an account?{" "}
              <button
                type="button"
                onClick={() =>
                  setInfoMessage(
                    "New staff accounts can be created by the store owner or manager in Settings → Staff.",
                  )
                }
                className="font-medium text-primary hover:underline"
              >
                Create one
              </button>
            </p>
          </div>
        </div>

        <p className="text-center text-xs text-muted-foreground">
          © {new Date().getFullYear()} {displayName}. All rights reserved.
        </p>
      </div>

      {/* Brand side */}
      <div className="relative hidden overflow-hidden bg-zinc-950 lg:flex lg:flex-col lg:justify-between lg:p-12">
        <div className="pointer-events-none absolute -left-24 -top-24 size-[28rem] rounded-full bg-primary/40 blur-3xl" />
        <div className="pointer-events-none absolute -bottom-32 -right-24 size-[30rem] rounded-full bg-fuchsia-500/20 blur-3xl" />
        <div
          className="pointer-events-none absolute inset-0 opacity-[0.07]"
          style={{
            backgroundImage:
              "linear-gradient(to right, white 1px, transparent 1px), linear-gradient(to bottom, white 1px, transparent 1px)",
            backgroundSize: "44px 44px",
          }}
        />

        <div className="relative" />

        <div className="relative space-y-8">
          <blockquote className="space-y-4">
            <p className="text-2xl font-medium leading-snug text-white">
              &ldquo;{displayName} — fast counter checkout, live stock tracking, and offline-first
              reliability.&rdquo;
            </p>
            <footer className="text-sm text-zinc-400">Retail Point of Sale & Inventory</footer>
          </blockquote>

          <ul className="grid gap-4 text-sm text-zinc-300">
            {[
              { icon: BarChart3, text: "Real-time stock tracking and sales reporting" },
              { icon: ShieldCheck, text: "Role-based counter security and staff PINs" },
              { icon: Zap, text: "Offline-first counter speed with reliable local ledger" },
            ].map(({ icon: Icon, text }) => (
              <li key={text} className="flex items-center gap-3">
                <span className="flex size-8 items-center justify-center rounded-lg bg-white/10 ring-1 ring-white/10">
                  <Icon className="size-4 text-white" />
                </span>
                {text}
              </li>
            ))}
          </ul>
        </div>
      </div>
    </main>
  );
}
