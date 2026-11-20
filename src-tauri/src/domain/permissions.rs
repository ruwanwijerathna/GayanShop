//! RBAC permission bits (ARCHITECTURE §3.5). Bit numbers are FROZEN once Phase 0 ships: append only,
//! never renumber. Bits 6 and 7 were reserved for exchanges and are now `EXCHANGE_PROCESS` / `EXCHANGE_LATE`.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Bits that were reserved before they had a meaning; they now belong to the exchange permissions only.
pub const EXCHANGE_BITS: [u32; 2] = [6, 7];

macro_rules! permissions {
    ($( $bit:literal => $key:ident, approvable: $approvable:literal; )*) => {
        /// Permission key as seen by the UI and stored in audit metadata.
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
        pub enum Perm { $( $key, )* }

        impl Perm {
            pub const ALL: &'static [Perm] = &[ $( Perm::$key, )* ];

            pub const fn bit(self) -> u32 {
                match self { $( Perm::$key => $bit, )* }
            }

            /// Whether a supervisor's PIN can authorise this for someone lacking it.
            /// Administrative and data-access permissions are never delegable this way.
            pub const fn approvable(self) -> bool {
                match self { $( Perm::$key => $approvable, )* }
            }

            pub const fn key(self) -> &'static str {
                match self { $( Perm::$key => stringify!($key), )* }
            }
        }
    };
}

permissions! {
    0  => SALE_CREATE,              approvable: false;
    1  => QUICK_SALE,               approvable: true;
    2  => PRICE_OVERRIDE,           approvable: true;
    3  => LINE_DISCOUNT,            approvable: true;
    4  => ORDER_DISCOUNT,           approvable: true;
    5  => VOID_ORDER,               approvable: true;
    6  => EXCHANGE_PROCESS,         approvable: true;
    7  => EXCHANGE_LATE,            approvable: true;
    8  => DRAWER_NO_SALE,           approvable: true;
    9  => CASH_PAY_IN_OUT,          approvable: true;
    10 => CASH_SESSION_MANAGE,      approvable: false;
    11 => CASH_SESSION_FORCE_CLOSE, approvable: true;
    12 => REPRINT_RECEIPT,          approvable: true;
    13 => STOCK_ADJUST,             approvable: true;
    14 => STOCK_DAMAGE,             approvable: true;
    15 => BATCH_MANAGE,             approvable: false;
    16 => PRODUCT_MANAGE,           approvable: false;
    17 => QUICK_SALE_PROMOTE,       approvable: false;
    18 => LABEL_PRINT,              approvable: true;
    19 => VIEW_COST,                approvable: false;
    20 => REPORT_SALES,             approvable: false;
    21 => REPORT_FINANCIAL,         approvable: false;
    22 => EXPENSE_MANAGE,           approvable: false;
    23 => SETTINGS_MANAGE,          approvable: false;
    24 => USER_MANAGE,              approvable: false;
    25 => AUDIT_VIEW,               approvable: false;
    26 => SYNC_MANAGE,              approvable: false;
    27 => BACKUP_RESTORE,           approvable: false;
    28 => DATA_EXPORT,              approvable: false;
    29 => CUSTOMER_VIEW,            approvable: false;
    30 => BACKUP_CREATE,            approvable: false;
}

/// A set of permissions, stored as an INTEGER bitmask (all bits < 53, safe as a JS number).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PermMask(pub i64);

impl PermMask {
    pub const EMPTY: PermMask = PermMask(0);

    pub fn all() -> Self {
        Self::of(Perm::ALL)
    }

    pub fn of(perms: &[Perm]) -> Self {
        PermMask(perms.iter().fold(0, |m, p| m | (1i64 << p.bit())))
    }

    pub fn has(self, p: Perm) -> bool {
        self.0 & (1i64 << p.bit()) != 0
    }

    pub fn list(self) -> Vec<Perm> {
        Perm::ALL.iter().copied().filter(|p| self.has(*p)).collect()
    }

    /// Strip bits that don't correspond to a known permission (reserved or from a newer client).
    pub fn known(self) -> Self {
        PermMask(self.0 & Self::all().0)
    }
}

/// `(role | grant) & ~deny`; the Owner system role is always everything.
pub fn effective(role: PermMask, grant: PermMask, deny: PermMask, is_owner_role: bool) -> PermMask {
    if is_owner_role {
        return PermMask::all();
    }
    PermMask((role.0 | grant.0) & !deny.0).known()
}

pub struct RoleSeed {
    pub name: &'static str,
    pub perms: PermMask,
    pub max_discount_bp: i64,
    pub max_override_bp: i64,
    pub min_pin_len: usize,
}

pub const OWNER_ROLE: &str = "Owner";

/// Owner-confirmed default matrix (§3.5). Cashier discount limit = 5% (owner rule).
pub fn default_roles() -> Vec<RoleSeed> {
    use Perm::*;
    let cashier = vec![
        SALE_CREATE,
        QUICK_SALE,
        LINE_DISCOUNT,
        ORDER_DISCOUNT,
        EXCHANGE_PROCESS,
        CASH_SESSION_MANAGE,
        REPRINT_RECEIPT,
    ];
    let mut supervisor = cashier.clone();
    supervisor.extend([
        PRICE_OVERRIDE,
        VOID_ORDER,
        EXCHANGE_LATE,
        DRAWER_NO_SALE,
        CASH_PAY_IN_OUT,
        CASH_SESSION_FORCE_CLOSE,
        STOCK_DAMAGE,
        LABEL_PRINT,
        REPORT_SALES,
    ]);
    let mut manager = supervisor.clone();
    manager.extend([
        STOCK_ADJUST,
        BATCH_MANAGE,
        PRODUCT_MANAGE,
        QUICK_SALE_PROMOTE,
        VIEW_COST,
        REPORT_FINANCIAL,
        EXPENSE_MANAGE,
        AUDIT_VIEW,
        DATA_EXPORT,
        CUSTOMER_VIEW,
        BACKUP_CREATE,
    ]);
    vec![
        RoleSeed {
            name: "Cashier",
            perms: PermMask::of(&cashier),
            max_discount_bp: 500,
            max_override_bp: 0,
            min_pin_len: 4,
        },
        RoleSeed {
            name: "Supervisor",
            perms: PermMask::of(&supervisor),
            max_discount_bp: 2_000,
            max_override_bp: 2_000,
            min_pin_len: 4,
        },
        RoleSeed {
            name: "Manager",
            perms: PermMask::of(&manager),
            max_discount_bp: 5_000,
            max_override_bp: 5_000,
            min_pin_len: 6,
        },
        RoleSeed {
            name: OWNER_ROLE,
            perms: PermMask::all(),
            max_discount_bp: 10_000,
            max_override_bp: 10_000,
            min_pin_len: 6,
        },
    ]
}

/// Minimum PIN length for a role name (Manager/Owner need 6+; custom roles 4).
pub fn min_pin_len(role_name: &str) -> usize {
    default_roles().into_iter().find(|r| r.name == role_name).map(|r| r.min_pin_len).unwrap_or(4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn bits_are_frozen_unique_reserved_and_js_safe() {
        assert_eq!(Perm::SALE_CREATE.bit(), 0);
        assert_eq!(Perm::VOID_ORDER.bit(), 5);
        assert_eq!(Perm::DRAWER_NO_SALE.bit(), 8);
        assert_eq!(Perm::DATA_EXPORT.bit(), 28);
        assert_eq!(Perm::CUSTOMER_VIEW.bit(), 29);
        assert_eq!(Perm::BACKUP_CREATE.bit(), 30);
        let bits: Vec<u32> = Perm::ALL.iter().map(|p| p.bit()).collect();
        assert_eq!(bits.iter().collect::<HashSet<_>>().len(), bits.len(), "bits must be unique");
        assert_eq!(Perm::EXCHANGE_PROCESS.bit(), EXCHANGE_BITS[0]);
        assert_eq!(Perm::EXCHANGE_LATE.bit(), EXCHANGE_BITS[1]);
        assert!(PermMask::all().0 < (1i64 << 53));
        // The SQL policy triggers hard-code these masks (§4.3): PRICE_OVERRIDE = 4, VOID_ORDER = 32.
        assert_eq!(PermMask::of(&[Perm::PRICE_OVERRIDE]).0, 4);
        assert_eq!(PermMask::of(&[Perm::VOID_ORDER]).0, 32);
    }

    #[test]
    fn effective_permissions() {
        let role = PermMask::of(&[Perm::SALE_CREATE, Perm::VOID_ORDER]);
        let grant = PermMask::of(&[Perm::DRAWER_NO_SALE]);
        let deny = PermMask::of(&[Perm::VOID_ORDER]);
        let e = effective(role, grant, deny, false);
        assert!(e.has(Perm::SALE_CREATE) && e.has(Perm::DRAWER_NO_SALE) && !e.has(Perm::VOID_ORDER));
        assert_eq!(effective(PermMask::EMPTY, PermMask::EMPTY, PermMask::all(), true), PermMask::all());
        assert_eq!(effective(PermMask(1 << 50), PermMask::EMPTY, PermMask::EMPTY, false), PermMask::EMPTY);
    }

    #[test]
    fn default_matrix_matches_owner_rules() {
        let roles = default_roles();
        let [cashier, supervisor, manager, owner] = [&roles[0], &roles[1], &roles[2], &roles[3]];
        assert_eq!((cashier.name, cashier.max_discount_bp), ("Cashier", 500));
        assert!(cashier.perms.has(Perm::LINE_DISCOUNT) && cashier.perms.has(Perm::ORDER_DISCOUNT));
        assert!(!cashier.perms.has(Perm::VOID_ORDER) && !cashier.perms.has(Perm::PRICE_OVERRIDE));
        assert!(!cashier.perms.has(Perm::DRAWER_NO_SALE));
        assert!(supervisor.perms.has(Perm::VOID_ORDER));
        // Any cashier may process an exchange inside the window; only a supervisor-or-above PIN lifts it.
        assert!(cashier.perms.has(Perm::EXCHANGE_PROCESS) && !cashier.perms.has(Perm::EXCHANGE_LATE));
        assert!(supervisor.perms.has(Perm::EXCHANGE_PROCESS) && supervisor.perms.has(Perm::EXCHANGE_LATE));
        assert!(manager.perms.has(Perm::EXCHANGE_LATE) && Perm::EXCHANGE_LATE.approvable());
        assert!(manager.perms.has(Perm::CUSTOMER_VIEW) && manager.perms.has(Perm::BACKUP_CREATE));
        assert!(!manager.perms.has(Perm::USER_MANAGE) && !manager.perms.has(Perm::BACKUP_RESTORE));
        assert_eq!(owner.perms, PermMask::all());
        assert_eq!(
            (min_pin_len("Owner"), min_pin_len("Manager"), min_pin_len("Cashier"), min_pin_len("Custom")),
            (6, 6, 4, 4)
        );
        assert!(
            !Perm::USER_MANAGE.approvable() && !Perm::CUSTOMER_VIEW.approvable() && !Perm::BACKUP_CREATE.approvable()
        );
        assert!(Perm::VOID_ORDER.approvable() && Perm::ORDER_DISCOUNT.approvable());
    }

    #[test]
    fn mask_list_roundtrip() {
        let m = PermMask::of(&[Perm::VOID_ORDER, Perm::AUDIT_VIEW]);
        assert_eq!(m.list(), vec![Perm::VOID_ORDER, Perm::AUDIT_VIEW]);
        assert_eq!(Perm::AUDIT_VIEW.key(), "AUDIT_VIEW");
    }
}
