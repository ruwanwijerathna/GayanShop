import { Printer } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Can } from "@/components/auth/Can";
import { Button } from "@/components/ui/button";
import { CommandError } from "@/db/client";
import { useReprintZReport } from "@/db/hooks";
import { translateError } from "@/i18n";
import { playCue } from "@/lib/audio";

/** "Print again": queues the Z-report of a closed shift on this counter's receipt printer (marked REPRINT). */
export function PrintZButton({ sessionId }: { sessionId: string }) {
  const { t, i18n } = useTranslation("cash");
  const reprint = useReprintZReport();
  return (
    <Can perm="REPRINT_RECEIPT">
      <Button
        type="button"
        size="pos"
        variant="outline"
        disabled={reprint.isPending}
        onClick={() =>
          reprint.mutate(sessionId, {
            onSuccess: () => {
              playCue("tick");
              toast.success(t("z.printed"));
            },
            onError: (e) => {
              playCue("error");
              if (e instanceof CommandError) toast.error(translateError(i18n.t, e.error));
            },
          })
        }
      >
        <Printer aria-hidden />
        {t("z.print")}
      </Button>
    </Can>
  );
}
