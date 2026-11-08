// TanStack Query hooks over the generated commands. Only src/db and src/hardware call Rust (AGENT.md §2).
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type Perm, type SetupInput, type UiLanguage, type UiTheme } from "./bindings";
import { unwrap } from "./client";

export const queryKeys = {
  status: ["app", "status"] as const,
  devicePrefs: ["app", "device-prefs"] as const,
  appInfo: ["app", "info"] as const,
  integrity: ["app", "integrity"] as const,
  loginStaff: ["auth", "login-staff"] as const,
  audit: ["audit", "pages"] as const,
};

/** Polled so the shell notices an idle auto-lock done by Rust (doc §3.5). */
export function useAppStatus() {
  return useQuery({
    queryKey: queryKeys.status,
    queryFn: () => unwrap(commands.appStatus()),
    refetchInterval: 30_000,
  });
}

export function useDevicePrefs() {
  return useQuery({ queryKey: queryKeys.devicePrefs, queryFn: () => unwrap(commands.devicePreferences()) });
}

export function useAppInfo() {
  return useQuery({ queryKey: queryKeys.appInfo, queryFn: () => unwrap(commands.appInfo()) });
}

export function useIntegrity() {
  return useQuery({ queryKey: queryKeys.integrity, queryFn: () => unwrap(commands.integrityCheck()) });
}

export function useLoginStaff(enabled = true) {
  return useQuery({
    queryKey: queryKeys.loginStaff,
    queryFn: () => unwrap(commands.listLoginStaff()),
    enabled,
  });
}

const AUDIT_PAGE = 200;

export function useAuditPages() {
  return useInfiniteQuery({
    queryKey: queryKeys.audit,
    initialPageParam: null as { created_at: string; id: string } | null,
    queryFn: ({ pageParam }) => unwrap(commands.auditPage(pageParam, AUDIT_PAGE)),
    getNextPageParam: (last) => last.next_cursor ?? null,
  });
}

export function useAuditVerify() {
  return useMutation({ mutationFn: () => unwrap(commands.auditVerify()) });
}

export function useFirstRunSetup() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: SetupInput) => unwrap(commands.firstRunSetup(input)),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["app"] }),
  });
}

export function useLogin() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ staffId, pin }: { staffId: string; pin: string }) => unwrap(commands.login(staffId, pin)),
    onSuccess: () => qc.invalidateQueries({ queryKey: queryKeys.status }),
  });
}

export function useLogout() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => unwrap(commands.logout()),
    onSuccess: () => {
      qc.removeQueries({ queryKey: queryKeys.audit });
      return qc.invalidateQueries({ queryKey: queryKeys.status });
    },
  });
}

export function useRequestApproval() {
  return useMutation({
    mutationFn: (v: { perm: Perm; targetRef: string; approverId: string; pin: string }) =>
      unwrap(commands.requestApproval(v.perm, v.targetRef, v.approverId, v.pin)),
  });
}

export function useSetUiLanguage() {
  return useMutation({ mutationFn: (language: UiLanguage) => unwrap(commands.setUiLanguage(language)) });
}

export function useSetDeviceTheme() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (theme: UiTheme) => unwrap(commands.setDeviceTheme(theme)),
    onSuccess: () => qc.invalidateQueries({ queryKey: queryKeys.devicePrefs }),
  });
}
