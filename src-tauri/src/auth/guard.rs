//! `require()` — the RBAC gate every mutating command passes first (ARCHITECTURE §3.5).

use super::Auth;
use crate::clock::Clock;
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};

/// Who is performing an action, and who approved it if step-up auth was used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub staff_id: String,
    pub approved_by: Option<String>,
}

/// Allow the action if the logged-in staff member holds `perm`, or if `approval_id` is a valid,
/// unexpired, single-use approval for exactly (`perm`, `target_ref`, this staff member).
///
/// Without either: `ApprovalRequired` when a supervisor PIN may grant it, otherwise `Forbidden`.
/// Every call counts as activity for the idle auto-lock.
pub fn require(
    auth: &Auth,
    clock: &dyn Clock,
    perm: Perm,
    target_ref: &str,
    approval_id: Option<&str>,
) -> AppResult<Actor> {
    let now = clock.now();
    let session = auth.sessions.touch(now, auth.idle_lock)?;
    if session.perms.has(perm) {
        return Ok(Actor { staff_id: session.staff_id, approved_by: None });
    }
    if let Some(id) = approval_id {
        return match auth.approvals.consume(id, perm, target_ref, &session.staff_id, now) {
            Some(approver) => Ok(Actor { staff_id: session.staff_id, approved_by: Some(approver) }),
            None => Err(AppError::InvalidApproval),
        };
    }
    if perm.approvable() { Err(AppError::ApprovalRequired { perm }) } else { Err(AppError::Forbidden { perm }) }
}
