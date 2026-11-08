//! Supervisor approval tokens (step-up auth, ARCHITECTURE §3.5): in memory only,
//! single-use, short-lived, and bound to (permission, target, requester).

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};

use crate::clock::new_id;
use crate::domain::permissions::Perm;

pub const APPROVAL_TTL_SECONDS: i64 = 60;

#[derive(Debug, Clone)]
struct Approval {
    perm: Perm,
    target_ref: String,
    requester_id: String,
    approver_id: String,
    expires_at: DateTime<Utc>,
}

#[derive(Default)]
pub struct ApprovalStore(Mutex<HashMap<String, Approval>>);

impl ApprovalStore {
    pub fn issue(&self, perm: Perm, target_ref: &str, requester_id: &str, approver_id: &str, now: DateTime<Utc>) -> (String, DateTime<Utc>) {
        let id = new_id();
        let expires_at = now + Duration::seconds(APPROVAL_TTL_SECONDS);
        let mut map = self.0.lock().expect("approvals lock");
        map.retain(|_, a| a.expires_at > now);
        map.insert(
            id.clone(),
            Approval {
                perm,
                target_ref: target_ref.to_owned(),
                requester_id: requester_id.to_owned(),
                approver_id: approver_id.to_owned(),
                expires_at,
            },
        );
        (id, expires_at)
    }

    /// Consume a token. Returns the approver's staff id if it matches and hasn't expired.
    /// The token is removed whether or not it matches, so a guessed/misused id can't be retried.
    pub fn consume(&self, id: &str, perm: Perm, target_ref: &str, requester_id: &str, now: DateTime<Utc>) -> Option<String> {
        let approval = self.0.lock().expect("approvals lock").remove(id)?;
        let valid = approval.perm == perm
            && approval.target_ref == target_ref
            && approval.requester_id == requester_id
            && now < approval.expires_at;
        valid.then_some(approval.approver_id)
    }

    /// Drop everything (e.g. on logout).
    pub fn clear(&self) {
        self.0.lock().expect("approvals lock").clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 8, 9, 0, 0).unwrap()
    }

    #[test]
    fn single_use() {
        let s = ApprovalStore::default();
        let (id, _) = s.issue(Perm::VOID_ORDER, "order-1", "cashier", "sup", t0());
        assert_eq!(s.consume(&id, Perm::VOID_ORDER, "order-1", "cashier", t0()), Some("sup".into()));
        assert_eq!(s.consume(&id, Perm::VOID_ORDER, "order-1", "cashier", t0()), None);
    }

    #[test]
    fn expires_after_ttl() {
        let s = ApprovalStore::default();
        let (id, _) = s.issue(Perm::VOID_ORDER, "o", "c", "s", t0());
        let late = t0() + Duration::seconds(APPROVAL_TTL_SECONDS);
        assert_eq!(s.consume(&id, Perm::VOID_ORDER, "o", "c", late), None);
        let (id, _) = s.issue(Perm::VOID_ORDER, "o", "c", "s", t0());
        assert!(s.consume(&id, Perm::VOID_ORDER, "o", "c", late - Duration::seconds(1)).is_some());
    }

    #[test]
    fn bound_to_perm_target_and_requester() {
        let s = ApprovalStore::default();
        for (perm, target, requester) in [
            (Perm::EXCHANGE_LATE, "o", "c"),
            (Perm::VOID_ORDER, "other", "c"),
            (Perm::VOID_ORDER, "o", "someone-else"),
        ] {
            let (id, _) = s.issue(Perm::VOID_ORDER, "o", "c", "s", t0());
            assert_eq!(s.consume(&id, perm, target, requester, t0()), None);
            // misuse burns the token
            assert_eq!(s.consume(&id, Perm::VOID_ORDER, "o", "c", t0()), None);
        }
    }
}
