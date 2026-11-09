//! An in-process stand-in for the sync server, for the client-side sync tests (Blocks 3.4, 3.5). It keeps
//! the protocol's contract: op ids are idempotent, insert-only rows are inserted once, an order may only
//! move completed → voided, mutable rows merge by last-write-wins, a row whose parent hasn't arrived is
//! answered `retry`, a device never receives its own writes, and every page carries its rows' ancestors
//! (parent closure, following the terminal's own foreign keys). The real server's rules (RLS, invariants,
//! the advisory lock) are proven by pgTAP and, end to end, by the chaos suite.
//!
//! Faults can be injected: refuse the next pushes or pulls (network down), or apply a push and lose its
//! answer (the network dropped mid-push).

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use gayanshop_lib::sync::api::{BoxFut, SyncApi, SyncError};
use gayanshop_lib::sync::apply::{Kind, TABLES};
use gayanshop_lib::sync::wire::{Health, OpResult, OpStatus, PullPage, PulledRow, PushOp};
use serde_json::Value;

pub const SYSTEM_DEVICE: &str = "00000000-0000-0000-0000-000000000000";

/// A foreign key of the terminal schema: `child.column → parent.id` (or `parent.key`).
#[derive(Debug, Clone)]
pub struct Fk {
    pub child: String,
    pub column: String,
    pub parent: String,
}

/// Every foreign key between synced tables, read from a terminal database's own schema.
pub async fn foreign_keys(pool: &sqlx::SqlitePool) -> Vec<Fk> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT m.name, p.\"from\", p.\"table\" FROM sqlite_master m, pragma_foreign_key_list(m.name) p
         WHERE m.type = 'table'",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let synced: HashSet<&str> = TABLES.iter().map(|(t, _, _)| *t).collect();
    rows.into_iter()
        .filter(|(c, _, p)| synced.contains(c.as_str()) && synced.contains(p.as_str()))
        .map(|(child, column, parent)| Fk { child, column, parent })
        .collect()
}

#[derive(Debug, Clone)]
struct Stored {
    seq: i64,
    row: Value,
}

#[derive(Default)]
struct State {
    seq: i64,
    rows: HashMap<(String, String), Stored>,
    processed: HashMap<String, OpStatus>,
    pushes: usize,
    pulls: usize,
}

pub struct Hub {
    fks: Vec<Fk>,
    state: Mutex<State>,
    /// Refuse this many coming pushes before touching anything (network down).
    pub fail_pushes: AtomicUsize,
    /// Apply this many coming pushes, then lose the answer (the network dropped mid-push).
    pub lose_push_answers: AtomicUsize,
    pub fail_pulls: AtomicUsize,
    /// Reject every op on these tables (to exercise dead ops).
    pub reject_tables: Mutex<HashSet<String>>,
    /// Leave out the parent closure (to prove a test needs it).
    pub no_closure: std::sync::atomic::AtomicBool,
}

fn kind_of(table: &str) -> Option<(Kind, &'static str)> {
    TABLES.iter().find(|(t, _, _)| *t == table).map(|(_, k, pk)| (*k, *pk))
}

fn s<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}

fn own(table: &str, row: &Value, me: &str) -> bool {
    match kind_of(table) {
        Some((Kind::InsertOnly, _)) => s(row, "device_id") == me,
        _ => s(row, "updated_by_device") == me,
    }
}

impl Hub {
    pub fn new(fks: Vec<Fk>) -> Self {
        Self {
            fks,
            state: Mutex::new(State::default()),
            fail_pushes: AtomicUsize::new(0),
            lose_push_answers: AtomicUsize::new(0),
            fail_pulls: AtomicUsize::new(0),
            reject_tables: Mutex::new(HashSet::new()),
            no_closure: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn client(self: &std::sync::Arc<Self>, device_id: &str) -> HubClient {
        HubClient { hub: self.clone(), device_id: device_id.to_owned() }
    }

    /// The cloud's copy of a row.
    pub fn row(&self, table: &str, id: &str) -> Option<Value> {
        self.state.lock().unwrap().rows.get(&(table.to_owned(), id.to_owned())).map(|r| r.row.clone())
    }

    pub fn count(&self, table: &str) -> usize {
        self.state.lock().unwrap().rows.keys().filter(|(t, _)| t == table).count()
    }

    pub fn pushes(&self) -> usize {
        self.state.lock().unwrap().pushes
    }

    pub fn pulls(&self) -> usize {
        self.state.lock().unwrap().pulls
    }

    /// Change a row in the cloud as another writer would (gets a new seq).
    pub fn put(&self, table: &str, row: Value) {
        let (_, pk) = kind_of(table).unwrap();
        let mut st = self.state.lock().unwrap();
        st.seq += 1;
        let seq = st.seq;
        st.rows.insert((table.to_owned(), s(&row, pk).to_owned()), Stored { seq, row });
    }

    fn apply(&self, st: &mut State, op: &PushOp) -> OpStatus {
        let Some((kind, pk)) = kind_of(&op.table) else { return OpStatus::Rejected };
        if self.reject_tables.lock().unwrap().contains(&op.table) {
            // Like the server: the refused row's cloud copy is re-stamped so its origin pulls it back.
            let key = (op.table.clone(), s(&op.row, pk).to_owned());
            if kind == Kind::Lww && st.rows.contains_key(&key) {
                st.seq += 1;
                let seq = st.seq;
                if let Some(r) = st.rows.get_mut(&key) {
                    r.seq = seq;
                }
            }
            return OpStatus::Rejected;
        }
        // A parent that hasn't arrived: retry later, not recorded.
        for fk in self.fks.iter().filter(|f| f.child == op.table) {
            let parent_id = s(&op.row, &fk.column);
            if !parent_id.is_empty() && !st.rows.contains_key(&(fk.parent.clone(), parent_id.to_owned())) {
                return OpStatus::Retry;
            }
        }
        let key = (op.table.clone(), s(&op.row, pk).to_owned());
        let store = match (kind, st.rows.get(&key)) {
            (_, None) => true,
            (Kind::InsertOnly, Some(_)) => false,
            (Kind::Order, Some(cur)) => s(&cur.row, "status") == "completed" && s(&op.row, "status") == "voided",
            // Customers: an erasure wins over any concurrent edit, whichever is newer (§6.3).
            (Kind::Lww, Some(cur)) if op.table == "customers" && !cur.row["erased_at"].is_null() => false,
            (Kind::Lww, Some(cur)) if op.table == "customers" && !op.row["erased_at"].is_null() => {
                cur.row["erased_at"].is_null()
            }
            (Kind::Lww, Some(cur)) => {
                let (at, dev) = (s(&op.row, "updated_at"), s(&op.row, "updated_by_device"));
                let (cat, cdev) = (s(&cur.row, "updated_at"), s(&cur.row, "updated_by_device"));
                at > cat || (at == cat && dev >= cdev)
            }
        };
        if store {
            let mut row = op.row.clone();
            // Like the server (§6.3): a product taking a barcode another live product holds is parked
            // inactive + barcode_conflict, stamped SYSTEM, so it returns to its origin too.
            if op.table == "products"
                && row["is_active"] == 1
                && row["deleted_at"].is_null()
                && let Some(barcode) = row["barcode"].as_str()
                && st.rows.iter().any(|((t, id), r)| {
                    t == "products"
                        && id != &key.1
                        && r.row["barcode"].as_str() == Some(barcode)
                        && r.row["is_active"] == 1
                        && r.row["deleted_at"].is_null()
                })
            {
                row["is_active"] = Value::from(0);
                row["barcode_conflict"] = Value::from(1);
                row["updated_by_device"] = Value::from(SYSTEM_DEVICE);
                let at = gayanshop_lib::clock::parse_iso(s(&op.row, "updated_at")).unwrap();
                row["updated_at"] = Value::from(gayanshop_lib::clock::iso(at + chrono::Duration::milliseconds(1)));
            }
            st.seq += 1;
            let seq = st.seq;
            st.rows.insert(key, Stored { seq, row });
        }
        OpStatus::Ok
    }

    fn push(&self, ops: &[PushOp]) -> Result<Vec<OpResult>, SyncError> {
        if take(&self.fail_pushes) {
            return Err(SyncError::Offline("injected: network down".into()));
        }
        let mut st = self.state.lock().unwrap();
        st.pushes += 1;
        let mut out = Vec::with_capacity(ops.len());
        for op in ops {
            let status = match st.processed.get(&op.op_id) {
                Some(prior) => *prior,
                None => {
                    let status = self.apply(&mut st, op);
                    if status != OpStatus::Retry {
                        st.processed.insert(op.op_id.clone(), status);
                    }
                    status
                }
            };
            out.push(OpResult { op_id: Value::String(op.op_id.clone()), status, reason: None });
        }
        drop(st);
        if take(&self.lose_push_answers) {
            return Err(SyncError::Offline("injected: answer lost".into()));
        }
        Ok(out)
    }

    fn pull(&self, me: &str, cursor: i64, max_rows: i64, include_own: bool) -> Result<PullPage, SyncError> {
        if take(&self.fail_pulls) {
            return Err(SyncError::Offline("injected: network down".into()));
        }
        let mut st = self.state.lock().unwrap();
        st.pulls += 1;
        let watermark = st.rows.values().map(|r| r.seq).max().unwrap_or(0);
        let mut primary: Vec<(&(String, String), &Stored)> =
            st.rows.iter().filter(|((t, _), r)| r.seq > cursor && (include_own || !own(t, &r.row, me))).collect();
        primary.sort_by_key(|(_, r)| r.seq);
        let limit = usize::try_from(max_rows).unwrap();
        let has_more = primary.len() > limit;
        primary.truncate(limit);
        let next = if has_more { primary.last().map(|(_, r)| r.seq).unwrap_or(cursor) } else { cursor.max(watermark) };

        // Parent closure: every ancestor newer than the cursor, until nothing new turns up.
        let mut page: HashMap<(String, String), i64> = primary.iter().map(|(k, r)| ((*k).clone(), r.seq)).collect();
        while !self.no_closure.load(Ordering::SeqCst) {
            let mut found = Vec::new();
            for (t, id) in page.keys() {
                let row = &st.rows[&(t.clone(), id.clone())].row;
                for fk in self.fks.iter().filter(|f| &f.child == t) {
                    let pid = s(row, &fk.column);
                    if pid.is_empty() {
                        continue;
                    }
                    let k = (fk.parent.clone(), pid.to_owned());
                    if let Some(p) = st.rows.get(&k)
                        && p.seq > cursor
                        && (include_own || !own(&fk.parent, &p.row, me))
                        && !page.contains_key(&k)
                    {
                        found.push((k, p.seq));
                    }
                }
            }
            if found.is_empty() {
                break;
            }
            page.extend(found);
        }
        let mut rows: Vec<PulledRow> = page
            .iter()
            .map(|((t, id), seq)| PulledRow {
                seq: *seq,
                table: t.clone(),
                row: st.rows[&(t.clone(), id.clone())].row.clone(),
            })
            .collect();
        rows.sort_by_key(|r| r.seq);
        Ok(PullPage { rows, cursor: next, has_more, watermark })
    }
}

fn take(counter: &AtomicUsize) -> bool {
    counter.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok()
}

/// One terminal's connection to the hub.
pub struct HubClient {
    hub: std::sync::Arc<Hub>,
    device_id: String,
}

impl SyncApi for HubClient {
    fn health(&self) -> BoxFut<'_, Result<Health, SyncError>> {
        Box::pin(async move {
            Ok(Health {
                ok: true,
                server_time: "2026-11-09T00:00:00.000Z".into(),
                device_code: String::new(),
                chain_broken: vec![],
            })
        })
    }

    fn push<'a>(&'a self, device_id: &'a str, ops: &'a [PushOp]) -> BoxFut<'a, Result<Vec<OpResult>, SyncError>> {
        Box::pin(async move {
            assert_eq!(device_id, self.device_id, "a device pushes as itself");
            self.hub.push(ops)
        })
    }

    fn pull(&self, cursor: i64, max_rows: i64, include_own: bool) -> BoxFut<'_, Result<PullPage, SyncError>> {
        Box::pin(async move { self.hub.pull(&self.device_id, cursor, max_rows, include_own) })
    }
}
