import { Link, useRouterState } from "@tanstack/react-router";
import {
  Banknote,
  BarChart3,
  Contact,
  Lock,
  Package,
  PanelLeft,
  ScrollText,
  Settings2,
  ShieldCheck,
  ShoppingCart,
  Store,
  UserRound,
  Users,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Kbd } from "@/components/common/Kbd";
import { Button } from "@/components/ui/button";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarSeparator,
  useSidebar,
} from "@/components/ui/sidebar";
import type { Perm } from "@/db/bindings";
import { hasPerm } from "@/lib/permissions";
import { useSession } from "../session";
import { WithTooltip } from "@/components/common/WithTooltip";

type NavTo =
  | "/"
  | "/pos"
  | "/orders"
  | "/cash"
  | "/customers"
  | "/products"
  | "/quick-sales"
  | "/reports"
  | "/audit"
  | "/staff"
  | "/settings";

interface NavItem {
  to: NavTo;
  /** `app:nav.*` key. */
  label: string;
  Icon: LucideIcon;
  /** Shown when the staff member holds any of these (hides only; Rust enforces). None ⇒ everyone. */
  perm?: Perm[];
}

interface NavGroup {
  label: string;
  items: NavItem[];
}

const GROUPS: NavGroup[] = [
  {
    label: "nav.groupSales",
    items: [
      { to: "/", label: "nav.home", Icon: Store },
      { to: "/pos", label: "nav.pos", Icon: ShoppingCart, perm: ["SALE_CREATE"] },
      { to: "/orders", label: "nav.orders", Icon: ScrollText, perm: ["REPORT_SALES"] },
      { to: "/cash", label: "nav.cash", Icon: Banknote },
      { to: "/customers", label: "nav.customers", Icon: Contact, perm: ["CUSTOMER_VIEW"] },
    ],
  },
  {
    label: "nav.groupInventory",
    items: [
      { to: "/products", label: "nav.products", Icon: Package },
      { to: "/quick-sales", label: "nav.quickSales", Icon: Zap, perm: ["QUICK_SALE_PROMOTE"] },
    ],
  },
  {
    label: "nav.groupManagement",
    items: [
      { to: "/reports", label: "nav.reports", Icon: BarChart3, perm: ["REPORT_SALES"] },
      { to: "/audit", label: "nav.audit", Icon: ShieldCheck, perm: ["AUDIT_VIEW"] },
    ],
  },
];

const SYSTEM: NavGroup = {
  label: "nav.groupSystem",
  items: [
    { to: "/staff", label: "nav.staff", Icon: Users, perm: ["USER_MANAGE"] },
    { to: "/settings", label: "nav.settings", Icon: Settings2, perm: ["SETTINGS_MANAGE", "BACKUP_CREATE"] },
  ],
};

/** 48 px rows open and collapsed (touch targets, doc §1.9); 20 px icons. */
const ROW =
  "h-12 text-base [&>svg]:size-5 group-data-[collapsible=icon]:size-12! group-data-[collapsible=icon]:p-3.5!";

function isActive(pathname: string, to: NavTo): boolean {
  return to === "/" ? pathname === "/" : pathname === to || pathname.startsWith(`${to}/`);
}

function NavItems({ items, pathname }: { items: NavItem[]; pathname: string }) {
  const { t } = useTranslation("app");
  return (
    <SidebarMenu>
      {items.map(({ to, label, Icon }) => (
        <SidebarMenuItem key={to}>
          <SidebarMenuButton asChild isActive={isActive(pathname, to)} tooltip={t(label)} className={ROW}>
            <Link to={to} aria-label={t(label)}>
              <Icon aria-hidden />
              <span>{t(label)}</span>
            </Link>
          </SidebarMenuButton>
        </SidebarMenuItem>
      ))}
    </SidebarMenu>
  );
}

/**
 * The back-office navigation (shadcn Sidebar, collapsible to icons; Ctrl+B or the header button toggles
 * it). Items are grouped by job; a group whose items the staff member can't open isn't shown.
 */
export function AppSidebar({ deviceCode, onLock }: { deviceCode: string | null; onLock: () => void }) {
  const { t } = useTranslation(["app", "auth"]);
  const { session } = useSession();
  const { toggleSidebar, state } = useSidebar();
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const allowed = (item: NavItem) => !item.perm || item.perm.some((p) => hasPerm(session, p));
  const groups = GROUPS.map((g) => ({ ...g, items: g.items.filter(allowed) })).filter(
    (g) => g.items.length > 0,
  );
  const system = SYSTEM.items.filter(allowed);
  const who = session ? `${session.display_name} · ${session.role_name}` : null;

  return (
    <Sidebar collapsible="icon" aria-label={t("nav.label")}>
      <SidebarHeader className="flex-row items-center justify-end p-2">
        <WithTooltip label={state === "expanded" ? t("nav.collapse") : t("nav.expand")}>
          <Button
            type="button"
            size="icon-pos"
            variant="ghost"
            aria-label={state === "expanded" ? t("nav.collapse") : t("nav.expand")}
            aria-expanded={state === "expanded"}
            onClick={toggleSidebar}
          >
            <PanelLeft aria-hidden />
          </Button>
        </WithTooltip>
      </SidebarHeader>

      <SidebarContent>
        {groups.map((g) => (
          <SidebarGroup key={g.label}>
            <SidebarGroupLabel className="text-sm">{t(g.label)}</SidebarGroupLabel>
            <SidebarGroupContent>
              <NavItems items={g.items} pathname={pathname} />
            </SidebarGroupContent>
          </SidebarGroup>
        ))}
      </SidebarContent>

      <SidebarFooter>
        {system.length > 0 && (
          <SidebarGroup className="p-0">
            <SidebarGroupLabel className="text-sm">{t("nav.groupSystem")}</SidebarGroupLabel>
            <SidebarGroupContent>
              <NavItems items={system} pathname={pathname} />
            </SidebarGroupContent>
          </SidebarGroup>
        )}
        <SidebarSeparator className="mx-0" />
        <SidebarMenu>
          {who && (
            <SidebarMenuItem>
              <div
                className="flex h-12 items-center gap-2 overflow-hidden rounded-md px-3.5 text-sm"
                title={deviceCode ? `${who} · ${deviceCode}` : who}
              >
                <UserRound aria-hidden className="size-5 shrink-0" />
                <span className="flex min-w-0 flex-1 flex-col group-data-[collapsible=icon]:hidden">
                  <span className="truncate font-medium">{session?.display_name}</span>
                  <span className="truncate text-muted-foreground">{session?.role_name}</span>
                </span>
                {deviceCode && (
                  <span className="rounded border border-input px-1.5 font-mono text-xs group-data-[collapsible=icon]:hidden">
                    {deviceCode}
                  </span>
                )}
              </div>
            </SidebarMenuItem>
          )}
          <SidebarMenuItem>
            <SidebarMenuButton tooltip={t("auth:lock")} onClick={onLock} className={ROW}>
              <Lock aria-hidden />
              <span>{t("auth:lock")}</span>
              <Kbd className="ml-auto group-data-[collapsible=icon]:hidden">{t("shortcuts.lock")}</Kbd>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  );
}
