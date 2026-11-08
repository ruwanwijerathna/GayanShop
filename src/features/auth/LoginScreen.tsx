import { UserRound } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { PinPad } from "@/components/common/PinPad";
import { Button } from "@/components/ui/button";
import type { LoginStaff } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useLogin, useLoginStaff } from "@/db/hooks";
import { translateError } from "@/i18n";

/** Staff picker (48 px+ tiles) → PIN pad. Lockout and wrong-PIN messages come from Rust error codes. */
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
  const [selected, setSelected] = useState<LoginStaff | null>(null);
  const [pin, setPin] = useState("");

  const error = login.error instanceof CommandError ? translateError(i18n.t, login.error.error) : null;

  const submit = () => {
    if (!selected) return;
    login.mutate({ staffId: selected.id, pin }, { onSettled: () => setPin("") });
  };

  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-6 bg-background p-6">
      <h1 className="text-3xl font-semibold">{storeName ?? t("common:app.name")}</h1>
      {wasLocked && (
        <p role="status" className="rounded-md bg-warning px-4 py-2 text-warning-foreground">
          {t("login.locked")}
        </p>
      )}

      {!selected ? (
        <section className="flex flex-col items-center gap-4">
          <h2 className="text-xl font-semibold">{t("login.title")}</h2>
          {staff.data && staff.data.length === 0 && <p>{t("login.noStaff")}</p>}
          <ul className="grid grid-cols-2 gap-3 lg:grid-cols-3">
            {staff.data?.map((s) => (
              <li key={s.id}>
                <Button
                  type="button"
                  size="pos-lg"
                  variant="outline"
                  className="h-20 w-56 flex-col items-start gap-0"
                  onClick={() => setSelected(s)}
                >
                  <span className="flex items-center gap-2 text-lg">
                    <UserRound aria-hidden /> {s.display_name}
                  </span>
                  <span className="text-sm font-normal text-muted-foreground">{s.role_name}</span>
                </Button>
              </li>
            ))}
          </ul>
        </section>
      ) : (
        <section className="flex flex-col items-center gap-4">
          <PinPad
            label={t("login.enterPin", { name: selected.display_name })}
            value={pin}
            onChange={setPin}
            onSubmit={submit}
            disabled={login.isPending}
          />
          {error && (
            <p
              role="alert"
              className="max-w-72 rounded-md bg-destructive px-3 py-2 text-destructive-foreground"
            >
              {error}
            </p>
          )}
          <div className="flex w-72 gap-2">
            <Button
              type="button"
              size="pos"
              variant="outline"
              className="flex-1"
              onClick={() => {
                setSelected(null);
                setPin("");
                login.reset();
              }}
            >
              {t("login.changeUser")}
            </Button>
            <Button
              type="button"
              size="pos"
              className="flex-1"
              disabled={pin.length === 0 || login.isPending}
              onClick={submit}
            >
              {t("login.submit")}
            </Button>
          </div>
        </section>
      )}
    </main>
  );
}
