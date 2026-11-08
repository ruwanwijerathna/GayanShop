import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { UiLanguage } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useFirstRunSetup } from "@/db/hooks";
import { LANGUAGES, translateError } from "@/i18n";

const STEPS = 3;

/** First-run setup (doc §8 Phase 0): language → store + counter → owner account. One Rust transaction. */
export function FirstRunWizard() {
  const { t, i18n } = useTranslation(["auth", "common"]);
  const setup = useFirstRunSetup();
  const [step, setStep] = useState(1);
  const [language, setLanguage] = useState<UiLanguage>("en");
  const [storeName, setStoreName] = useState("GayanShop");
  const [deviceCode, setDeviceCode] = useState("T01");
  const [ownerName, setOwnerName] = useState("");
  const [pin, setPin] = useState("");
  const [pinConfirm, setPinConfirm] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);

  const error =
    localError ?? (setup.error instanceof CommandError ? translateError(i18n.t, setup.error.error) : null);

  const chooseLanguage = (lng: UiLanguage) => {
    setLanguage(lng);
    void i18n.changeLanguage(lng);
  };

  const finish = () => {
    if (pin !== pinConfirm) {
      setLocalError(t("wizard.pinMismatch"));
      return;
    }
    setLocalError(null);
    setup.mutate({
      store_name: storeName,
      owner_name: ownerName,
      owner_pin: pin,
      device_code: deviceCode,
      ui_language: language,
    });
  };

  return (
    <main className="flex min-h-screen items-center justify-center bg-background p-6">
      <section className="flex w-full max-w-xl flex-col gap-6 rounded-xl border border-border bg-card p-8">
        <header>
          <h1 className="text-2xl font-semibold">{t("wizard.title")}</h1>
          <p className="text-sm text-muted-foreground">{t("wizard.step", { current: step, total: STEPS })}</p>
        </header>

        {step === 1 && (
          <fieldset className="flex flex-col gap-3">
            <legend className="mb-2 text-lg font-semibold">{t("wizard.languageTitle")}</legend>
            <div className="flex gap-3">
              {LANGUAGES.map((lng) => (
                <Button
                  key={lng}
                  type="button"
                  size="pos-lg"
                  lang={lng}
                  variant={language === lng ? "default" : "outline"}
                  aria-pressed={language === lng}
                  className="flex-1"
                  onClick={() => chooseLanguage(lng)}
                >
                  {t(`common:language.${lng}`)}
                </Button>
              ))}
            </div>
          </fieldset>
        )}

        {step === 2 && (
          <fieldset className="flex flex-col gap-4">
            <legend className="mb-2 text-lg font-semibold">{t("wizard.storeTitle")}</legend>
            <div className="flex flex-col gap-2">
              <Label htmlFor="store-name">{t("wizard.storeName")}</Label>
              <Input
                id="store-name"
                className="h-12 text-base"
                value={storeName}
                maxLength={80}
                onChange={(e) => setStoreName(e.target.value)}
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="device-code">{t("wizard.deviceCode")}</Label>
              <Input
                id="device-code"
                className="h-12 font-mono text-base uppercase"
                value={deviceCode}
                maxLength={3}
                aria-describedby="device-code-hint"
                onChange={(e) => setDeviceCode(e.target.value.toUpperCase())}
              />
              <p id="device-code-hint" className="text-sm text-muted-foreground">
                {t("wizard.deviceCodeHint")}
              </p>
            </div>
          </fieldset>
        )}

        {step === 3 && (
          <fieldset className="flex flex-col gap-4">
            <legend className="mb-2 text-lg font-semibold">{t("wizard.ownerTitle")}</legend>
            <div className="flex flex-col gap-2">
              <Label htmlFor="owner-name">{t("wizard.ownerName")}</Label>
              <Input
                id="owner-name"
                className="h-12 text-base"
                value={ownerName}
                maxLength={60}
                onChange={(e) => setOwnerName(e.target.value)}
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="owner-pin">{t("wizard.ownerPin")}</Label>
              <Input
                id="owner-pin"
                type="password"
                inputMode="numeric"
                className="h-12 font-mono text-base"
                value={pin}
                maxLength={8}
                onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="owner-pin-confirm">{t("wizard.ownerPinConfirm")}</Label>
              <Input
                id="owner-pin-confirm"
                type="password"
                inputMode="numeric"
                className="h-12 font-mono text-base"
                value={pinConfirm}
                maxLength={8}
                onChange={(e) => setPinConfirm(e.target.value.replace(/\D/g, ""))}
              />
            </div>
          </fieldset>
        )}

        {error && (
          <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
            {error}
          </p>
        )}

        <footer className="flex justify-between gap-3">
          <Button
            type="button"
            size="pos"
            variant="outline"
            disabled={step === 1 || setup.isPending}
            onClick={() => setStep((s) => s - 1)}
          >
            {t("common:actions.back")}
          </Button>
          {step < STEPS ? (
            <Button type="button" size="pos" onClick={() => setStep((s) => s + 1)}>
              {t("common:actions.continue")}
            </Button>
          ) : (
            <Button type="button" size="pos" disabled={setup.isPending} onClick={finish}>
              {setup.isPending ? t("wizard.working") : t("wizard.finish")}
            </Button>
          )}
        </footer>
      </section>
    </main>
  );
}
