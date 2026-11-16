const TEMPLATE_KEY = "gayanshop.label.template";
const FALLBACK = "two_up_50x25";

/** The sticker roll this counter used last (or chose in Settings); both Settings and the print dialog read it. */
export function lastTemplate(): string {
  try {
    return localStorage.getItem(TEMPLATE_KEY) ?? FALLBACK;
  } catch {
    return FALLBACK;
  }
}

export function rememberTemplate(id: string) {
  try {
    localStorage.setItem(TEMPLATE_KEY, id);
  } catch {
    // A private window or blocked storage: the choice simply isn't remembered.
  }
}
