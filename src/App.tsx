import { RouterProvider } from "@tanstack/react-router";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ExplicitLockContext } from "@/app/lock-context";
import { createAppRouter } from "@/app/router";
import { useSession } from "@/app/session";
import { useApplyTheme } from "@/components/common/ThemeToggle";
import { CommandError } from "@/db/client";
import { useAppStatus, useDevicePrefs } from "@/db/hooks";
import { FirstRunWizard } from "@/features/auth/FirstRunWizard";
import { BootstrapResume } from "@/features/auth/JoinStore";
import { LoginScreen } from "@/features/auth/LoginScreen";
import { isUiLanguage, translateError } from "@/i18n";

/** Routes the app: first-run wizard (or joining a store and pulling its history) → login (PIN) → logged-in
 * shell. Rust is the source of truth. */
function App() {
  const { t, i18n } = useTranslation();
  const status = useAppStatus();
  const prefs = useDevicePrefs();
  const { setSession } = useSession();
  const [router] = useState(createAppRouter);

  const rustSession = status.data?.session ?? null;
  const sessionKey = rustSession?.staff_id ?? null;

  // A session that ends without an explicit lock/logout was ended by Rust's idle auto-lock (doc §3.5).
  // Derived while rendering (React "adjust state when a value changes" pattern), not in an effect.
  const [prevKey, setPrevKey] = useState<string | null>(sessionKey);
  const [explicitLock, setExplicitLock] = useState(false);
  const [wasLocked, setWasLocked] = useState(false);
  if (sessionKey !== prevKey) {
    setPrevKey(sessionKey);
    setWasLocked(prevKey !== null && sessionKey === null && !explicitLock);
    setExplicitLock(false);
  }
  const markExplicitLock = useCallback(() => setExplicitLock(true), []);

  // Keep the read-only session mirror (used by <Can>) in step with Rust.
  useEffect(() => setSession(rustSession), [rustSession, setSession]);

  // Apply the saved language only when the staff member (or their saved language) changes. Re-applying on
  // every effect run would undo a live EN ↔ සිං switch, because the session copy stays stale until re-login.
  const language = rustSession?.ui_language ?? prefs.data?.default_language;
  const languageKey = `${sessionKey ?? ""}:${language ?? ""}`;
  const appliedLanguageKey = useRef<string | null>(null);
  useEffect(() => {
    if (!language || !isUiLanguage(language) || appliedLanguageKey.current === languageKey) return;
    appliedLanguageKey.current = languageKey;
    if (i18n.resolvedLanguage !== language) void i18n.changeLanguage(language);
  }, [language, languageKey, i18n]);

  useApplyTheme(prefs.data?.theme ?? "light");

  if (status.error) {
    const message =
      status.error instanceof CommandError
        ? translateError(i18n.t, status.error.error)
        : t("errors:INTERNAL");
    return (
      <main role="alert" className="flex min-h-screen items-center justify-center bg-background p-6 text-lg">
        {message}
      </main>
    );
  }
  if (!status.data) {
    return (
      <main className="flex min-h-screen items-center justify-center bg-background text-muted-foreground">
        {t("table.loading")}
      </main>
    );
  }
  if (!status.data.initialized) return <FirstRunWizard />;
  if (status.data.bootstrap_pending) return <BootstrapResume />;
  if (!rustSession) return <LoginScreen storeName={status.data.store_name ?? null} wasLocked={wasLocked} />;
  return (
    <ExplicitLockContext.Provider value={markExplicitLock}>
      <RouterProvider router={router} />
    </ExplicitLockContext.Provider>
  );
}

export default App;
