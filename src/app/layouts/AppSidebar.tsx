import { Link, useNavigate, useRouterState } from "@tanstack/react-router";
import {
  Banknote,
  BarChart3,
  ChevronsUpDown,
  Contact,
  Lock,
  LogOut,
  Package,
  PanelLeft,
  ScrollText,
  Settings2,
  ShieldCheck,
  ShoppingCart,
  Store,
  Users,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
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
      { to: "/staff", label: "nav.staff", Icon: Users, perm: ["USER_MANAGE"] },
      { to: "/audit", label: "nav.audit", Icon: ShieldCheck, perm: ["AUDIT_VIEW"] },
    ],
  },
];

const SYSTEM: NavGroup = {
  label: "nav.groupSystem",
  items: [
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
  const { toggleSidebar, state, isMobile } = useSidebar();
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const allowed = (item: NavItem) => !item.perm || item.perm.some((p) => hasPerm(session, p));
  const groups = GROUPS.map((g) => ({ ...g, items: g.items.filter(allowed) })).filter(
    (g) => g.items.length > 0,
  );
  const system = SYSTEM.items.filter(allowed);
  const initials =
    (session?.display_name ?? "")
      .split(" ")
      .filter(Boolean)
      .map((n) => n[0])
      .join("")
      .slice(0, 2)
      .toUpperCase() || "U";

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
          {session && (
            <SidebarMenuItem>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <SidebarMenuButton
                    size="lg"
                    className="data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground h-12 text-base group-data-[collapsible=icon]:size-12! group-data-[collapsible=icon]:p-2!"
                    aria-label={session.display_name}
                  >
                    <Avatar className="size-8 rounded-lg">
                      <AvatarFallback className="rounded-lg bg-primary/10 text-primary font-semibold">
                        {initials}
                      </AvatarFallback>
                    </Avatar>
                    <div className="grid flex-1 text-left text-sm leading-tight group-data-[collapsible=icon]:hidden">
                      <span className="truncate font-semibold">{session.display_name}</span>
                      <span className="truncate text-xs text-muted-foreground">{session.role_name}</span>
                    </div>
                    {deviceCode && (
                      <span className="rounded border border-input px-1.5 font-mono text-xs group-data-[collapsible=icon]:hidden">
                        {deviceCode}
                      </span>
                    )}
                    <ChevronsUpDown className="ml-auto size-4 group-data-[collapsible=icon]:hidden text-muted-foreground" />
                  </SidebarMenuButton>
                </DropdownMenuTrigger>
                <DropdownMenuContent
                  className="w-(--radix-dropdown-menu-trigger-width) min-w-56 rounded-lg"
                  side={isMobile ? "bottom" : "right"}
                  align="end"
                  sideOffset={8}
                >
                  <DropdownMenuLabel className="p-0 font-normal">
                    <div className="flex items-center gap-2 px-2 py-1.5 text-left text-sm">
                      <Avatar className="size-8 rounded-lg">
                        <AvatarFallback className="rounded-lg bg-primary/10 text-primary font-semibold">
                          {initials}
                        </AvatarFallback>
                      </Avatar>
                      <div className="grid flex-1 text-left text-sm leading-tight">
                        <span className="truncate font-semibold">{session.display_name}</span>
                        <span className="truncate text-xs text-muted-foreground">{session.role_name}</span>
                      </div>
                      {deviceCode && (
                        <span className="rounded border border-input px-1.5 font-mono text-xs">
                          {deviceCode}
                        </span>
                      )}
                    </div>
                  </DropdownMenuLabel>
                  <DropdownMenuSeparator />
                  {system.length > 0 && (
                    <>
                      <DropdownMenuGroup>
                        <DropdownMenuItem onClick={() => navigate({ to: "/settings" })}>
                          <Settings2 className="mr-2 size-4" />
                          <span>{t("nav.settings")}</span>
                        </DropdownMenuItem>
                      </DropdownMenuGroup>
                      <DropdownMenuSeparator />
                    </>
                  )}
                  <DropdownMenuItem onClick={onLock}>
                    <Lock className="mr-2 size-4" />
                    <span>{t("auth:lock")}</span>
                    <DropdownMenuShortcut>{t("shortcuts.lock")}</DropdownMenuShortcut>
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={onLock} variant="destructive">
                    <LogOut className="mr-2 size-4" />
                    <span>{t("auth:logout")}</span>
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </SidebarMenuItem>
          )}
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  );
}
