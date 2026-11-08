//! RBAC permission bits (ARCHITECTURE §3.5). Bit numbers are FROZEN: append only, never renumber.

use serde::{Deserialize, Serialize};
use specta::Type;

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
            /// Administrative permissions are never delegable this way.
            pub const fn approvable(self) -> bool {
                match self { $( Perm::$key => $approvable, )* }
            }

            pub fn key(self) -> &'static str {
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

    /// Strip bits that don't correspond to a known permission (e.g. from a newer client).
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
    pub is_system: bool,
    pub min_pin_len: usize,
}

pub const OWNER_ROLE: &str = "Owner";

/// Owner-confirmed default matrix (§3.5). Cashier discount limit = 5% (owner rule).
pub fn default_roles() -> Vec<RoleSeed> {
    use Perm::*;
    let cashier = vec![SALE_CREATE, QUICK_SALE, LINE_DISCOUNT, ORDER_DISCOUNT, EXCHANGE_PROCESS, CASH_SESSION_MANAGE, REPRINT_RECEIPT];
    let mut supervisor = cashier.clone();
    supervisor.extend([
        PRICE_OVERRIDE, VOID_ORDER, EXCHANGE_LATE, DRAWER_NO_SALE, CASH_PAY_IN_OUT, CASH_SESSION_FORCE_CLOSE,
        STOCK_DAMAGE, LABEL_PRINT, REPORT_SALES,
    ]);
    let mut manager = supervisor.clone();
    manager.extend([
        STOCK_ADJUST, BATCH_MANAGE, PRODUCT_MANAGE, QUICK_SALE_PROMOTE, VIEW_COST, REPORT_FINANCIAL, EXPENSE_MANAGE,
        AUDIT_VIEW, DATA_EXPORT,
    ]);
    vec![
        RoleSeed { name: "Cashier", perms: PermMask::of(&cashier), max_discount_bp: 500, max_override_bp: 0, is_system: true, min_pin_len: 4 },
        RoleSeed { name: "Supervisor", perms: PermMask::of(&supervisor), max_discount_bp: 2_000, max_override_bp: 2_000, is_system: true, min_pin_len: 4 },
        RoleSeed { name: "Manager", perms: PermMask::of(&manager), max_discount_bp: 5_000, max_override_bp: 5_000, is_system: true, min_pin_len: 6 },
        RoleSeed { name: OWNER_ROLE, perms: PermMask::all(), max_discount_bp: 10_000, max_override_bp: 10_000, is_system: true, min_pin_len: 6 },
    ]
}

/// Minimum PIN length for a role name (Manager/Owner need 6+).
pub fn min_pin_len(role_name: &str) -> usize {
    default_roles().into_iter().find(|r| r.name == role_name).map(|r| r.min_pin_len).unwrap_or(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_are_frozen_and_safe_for_js() {
        assert_eq!(Perm::SALE_CREATE.bit(), 0);
        assert_eq!(Perm::EXCHANGE_PROCESS.bit(), 6);
        assert_eq!(Perm::EXCHANGE_LATE.bit(), 7);
        assert_eq!(Perm::DATA_EXPORT.bit(), 28);
        let bits: Vec<u32> = Perm::ALL.iter().map(|p| p.bit()).collect();
        assert_eq!(bits, (0..bits.len() as u32).collect::<Vec<_>>(), "bits must be dense and in order");
        assert!(PermMask::all().0 < (1i64 << 53));
    }

    #[test]
    fn effective_permissions() {
        let role = PermMask::of(&[Perm::SALE_CREATE, Perm::VOID_ORDER]);
        let grant = PermMask::of(&[Perm::DRAWER_NO_SALE]);
        let deny = PermMask::of(&[Perm::VOID_ORDER]);
        let e = effective(role, grant, deny, false);
        assert!(e.has(Perm::SALE_CREATE) && e.has(Perm::DRAWER_NO_SALE) && !e.has(Perm::VOID_ORDER));
        // Owner cannot be reduced.
        assert_eq!(effective(PermMask::EMPTY, PermMask::EMPTY, PermMask::all(), true), PermMask::all());
        // Unknown high bits are dropped.
        assert_eq!(effective(PermMask(1 << 50), PermMask::EMPTY, PermMask::EMPTY, false), PermMask::EMPTY);
    }

    #[test]
    fn default_matrix_matches_owner_rules() {
        let roles = default_roles();
        let cashier = &roles[0];
        assert_eq!(cashier.max_discount_bp, 500);
        assert!(cashier.perms.has(Perm::LINE_DISCOUNT) && cashier.perms.has(Perm::ORDER_DISCOUNT));
        assert!(cashier.perms.has(Perm::EXCHANGE_PROCESS));
        assert!(!cashier.perms.has(Perm::EXCHANGE_LATE));
        assert!(!cashier.perms.has(Perm::PRICE_OVERRIDE));
        assert!(!cashier.perms.has(Perm::DRAWER_NO_SALE));
        let supervisor = &roles[1];
        assert!(supervisor.perms.has(Perm::EXCHANGE_LATE) && supervisor.perms.has(Perm::VOID_ORDER));
        assert!(!roles[2].perms.has(Perm::USER_MANAGE));
        assert_eq!(roles[3].perms, PermMask::all());
        // Admin powers can't be borrowed with a supervisor PIN.
        assert!(!Perm::USER_MANAGE.approvable() && !Perm::SETTINGS_MANAGE.approvable());
        assert!(Perm::EXCHANGE_LATE.approvable() && Perm::ORDER_DISCOUNT.approvable());
    }
}
