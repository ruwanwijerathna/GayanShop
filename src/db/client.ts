// The only bridge between the UI and Rust commands (AGENT.md §2). Unwraps tauri-specta's
// `{ status, data | error }` results and turns `AppError` into a typed, translatable exception.
import type { AppError } from "./bindings";

export type CommandResult<T> = { status: "ok"; data: T } | { status: "error"; error: AppError };

/** The i18n key for an error: `errors.<KIND>` or `errors.<KIND>.<code>` (mirrors Rust `AppError::i18n_key`). */
export function errorKey(error: AppError): string {
  if (error.kind === "VALIDATION" || error.kind === "INVARIANT") return `errors.${error.kind}.${error.code}`;
  return `errors.${error.kind}`;
}

/** Thrown by `unwrap` when a command returns an `AppError`. */
export class CommandError extends Error {
  readonly error: AppError;
  readonly i18nKey: string;

  constructor(error: AppError) {
    super(error.kind);
    this.name = "CommandError";
    this.error = error;
    this.i18nKey = errorKey(error);
  }

  /** True when a supervisor approval would let the action through (opens the ApprovalDialog). */
  get needsApproval(): boolean {
    return this.error.kind === "APPROVAL_REQUIRED";
  }
}

/** Await a generated command and return its data, or throw `CommandError`. */
export async function unwrap<T>(pending: Promise<CommandResult<T>>): Promise<T> {
  const result = await pending;
  if (result.status === "ok") return result.data;
  throw new CommandError(result.error);
}
