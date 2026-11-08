import { QueryClientProvider } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { I18nextProvider } from "react-i18next";
import { Toaster } from "@/components/ui/sonner";
import { createI18n, type UiLanguage } from "@/i18n";
import { createQueryClient } from "./query-client";
import { SessionProvider } from "./session";

export function Providers({ children, language = "en" }: { children: ReactNode; language?: UiLanguage }) {
  const [queryClient] = useState(createQueryClient);
  const [i18n] = useState(() => createI18n(language));
  return (
    <QueryClientProvider client={queryClient}>
      <I18nextProvider i18n={i18n}>
        <SessionProvider>
          {children}
          <Toaster position="top-center" />
        </SessionProvider>
      </I18nextProvider>
    </QueryClientProvider>
  );
}
