import { describe, expect, it } from "vitest";
import type { AppError } from "./bindings";
import { CommandError, errorKey, unwrap } from "./client";

describe("errorKey", () => {
  it("maps kinds to errors.<KIND>", () => {
    expect(errorKey({ kind: "NOT_LOGGED_IN" })).toBe("errors.NOT_LOGGED_IN");
    expect(errorKey({ kind: "FORBIDDEN", perm: "USER_MANAGE" })).toBe("errors.FORBIDDEN");
  });

  it("adds the stable code for validation and invariant errors", () => {
    expect(errorKey({ kind: "VALIDATION", code: "PIN_TOO_SHORT", detail: "x" })).toBe(
      "errors.VALIDATION.PIN_TOO_SHORT",
    );
    expect(errorKey({ kind: "INVARIANT", code: "LAST_OWNER", detail: "x" })).toBe(
      "errors.INVARIANT.LAST_OWNER",
    );
  });
});

describe("unwrap", () => {
  it("returns data on success", async () => {
    await expect(unwrap(Promise.resolve({ status: "ok", data: 42 }))).resolves.toBe(42);
  });

  it("throws a CommandError carrying the AppError and its i18n key", async () => {
    const error: AppError = { kind: "APPROVAL_REQUIRED", perm: "VOID_ORDER" };
    const thrown = await unwrap(Promise.resolve({ status: "error", error })).catch((e: unknown) => e);
    expect(thrown).toBeInstanceOf(CommandError);
    const ce = thrown as CommandError;
    expect(ce.error).toEqual(error);
    expect(ce.i18nKey).toBe("errors.APPROVAL_REQUIRED");
    expect(ce.needsApproval).toBe(true);
  });
});
