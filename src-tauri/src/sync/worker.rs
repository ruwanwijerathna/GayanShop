//! The background sync worker (ARCHITECTURE §1.3): a tokio task that never blocks a sale.
//!
//! * Connectivity is probed, not guessed: every round starts with the `sync_health` RPC. Idle, a round runs
//!   every 15 s; right after a local write it runs at once (`SyncHandle::poke`, called with every
//!   `db://changed`).
//! * Failures back off exponentially, 5 s → 5 min, with jitter, so three counters coming back online don't
//!   reconnect in lockstep.
//! * A `sync://status` event drives the status pill: synced, pending N, offline, or a problem that needs a
//!   person (unregistered, identity revoked).
//!
//! The worker only ever takes the database's writer lock for short local transactions; network calls happen
//! outside it.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use rand_core::{OsRng, RngCore};
use serde::Serialize;
use specta::Type;
use tokio::sync::Notify;

use super::api::{HttpApi, SyncApi, SyncError};
use super::config::{self, ServerConfig};
use super::credentials::SecretStore;
use super::engine::{self, RoundError};
use super::registration::{self, BOOTSTRAP_PENDING_KEY};
use super::wire::ChainBreak;
use crate::clock::{Clock, iso};
use crate::db::Db;
use crate::db::settings::{self, keys};
use crate::error::AppResult;

pub const IDLE_PROBE: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    /// No server or device identity yet (Settings → Sync → Register).
    Unregistered,
    /// A joined counter that hasn't pulled the store's history yet.
    BootstrapPending,
    /// A restored backup awaits recovery: nothing is sent or applied until the Owner finishes it.
    RestorePending,
    Syncing,
    /// Everything this counter wrote is in the cloud and it has everything the cloud has.
    Synced,
    /// Local writes are still queued (the server asked for a retry, or ops are dead).
    Pending,
    /// The server can't be reached; sales carry on and queue.
    Offline,
    /// The device identity was refused: the owner must re-register this counter.
    Unauthorized,
    /// Something unexpected; see `last_error`.
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct SyncStatus {
    pub state: SyncState,
    /// Outbox ops not yet accepted by the cloud (pending, retrying or in flight).
    #[specta(type = specta_typescript::Number)]
    pub pending: i64,
    /// Ops the cloud rejected (Settings → Sync).
    #[specta(type = specta_typescript::Number)]
    pub dead: i64,
    pub last_success_at: Option<String>,
    pub last_error: Option<String>,
    /// Devices whose audit chain the server flagged.
    pub chain_broken: Vec<ChainBreakView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ChainBreakView {
    pub device_code: String,
    pub at: Option<String>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub seq: Option<i64>,
}

impl From<&ChainBreak> for ChainBreakView {
    fn from(c: &ChainBreak) -> Self {
        Self { device_code: c.device_code.clone(), at: c.at.clone(), seq: c.seq }
    }
}

impl SyncStatus {
    fn initial() -> Self {
        Self {
            state: SyncState::Unregistered,
            pending: 0,
            dead: 0,
            last_success_at: None,
            last_error: None,
            chain_broken: vec![],
        }
    }
}

/// What the worker tells the app: status changes, and the tables a pull changed.
pub trait SyncEvents: Send + Sync {
    fn status(&self, status: &SyncStatus);
    fn applied(&self, tables: &[&'static str]);
}

/// Shared between the worker and the commands.
#[derive(Clone)]
pub struct SyncHandle {
    notify: Arc<Notify>,
    status: Arc<RwLock<SyncStatus>>,
}

impl Default for SyncHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncHandle {
    pub fn new() -> Self {
        Self { notify: Arc::new(Notify::new()), status: Arc::new(RwLock::new(SyncStatus::initial())) }
    }

    /// Run a round now (after a local write, or "Sync now").
    pub fn poke(&self) {
        self.notify.notify_one();
    }

    pub fn status(&self) -> SyncStatus {
        self.status.read().map(|s| s.clone()).unwrap_or_else(|p| p.into_inner().clone())
    }

    fn set(&self, status: SyncStatus) {
        match self.status.write() {
            Ok(mut s) => *s = status,
            Err(p) => *p.into_inner() = status,
        }
    }
}

/// Outbox counts for the status: (not yet accepted, dead).
pub async fn outbox_counts(db: &Db) -> AppResult<(i64, i64)> {
    let (pending, dead): (Option<i64>, Option<i64>) =
        sqlx::query_as("SELECT SUM(status IN ('pending','failed','in_flight')), SUM(status = 'dead') FROM sync_outbox")
            .fetch_one(&db.pool)
            .await?;
    Ok((pending.unwrap_or(0), dead.unwrap_or(0)))
}

/// The delay before the next attempt after `failures` consecutive failures: 5 s doubling to 5 min, plus up
/// to 20 % jitter.
pub fn retry_delay(failures: u32) -> Duration {
    let base = 5u64.saturating_mul(1u64 << failures.min(6)).min(300);
    let jitter_ms = u64::from(OsRng.next_u32()) % (base * 200).max(1);
    Duration::from_secs(base) + Duration::from_millis(jitter_ms)
}

/// Opens the connection to the server for a configuration and device. The app uses [`http_connector`];
/// tests connect the worker to an in-process fake.
pub type Connector =
    Arc<dyn Fn(&ServerConfig, &str, Arc<dyn SecretStore>) -> Result<Arc<dyn SyncApi>, SyncError> + Send + Sync>;

pub fn http_connector() -> Connector {
    Arc::new(|cfg: &ServerConfig, device_id: &str, secrets: Arc<dyn SecretStore>| {
        Ok(Arc::new(HttpApi::new(cfg.clone(), device_id, secrets)?) as Arc<dyn SyncApi>)
    })
}

pub struct Worker {
    pub db: Db,
    pub clock: Arc<dyn Clock>,
    pub secrets: Arc<dyn SecretStore>,
    pub events: Arc<dyn SyncEvents>,
    pub handle: SyncHandle,
    pub connect: Connector,
}

enum Tick {
    Idle,
    Failed(Duration),
}

impl Worker {
    /// Run forever. Spawn it on the app's async runtime.
    pub async fn run(self) {
        if let Err(e) = engine::recover_in_flight(&self.db).await {
            log::warn!("sync: recovering in-flight ops failed: {e}");
        }
        let mut api: Option<(ServerConfig, String, Arc<dyn SyncApi>)> = None;
        let mut failures: u32 = 0;
        loop {
            let wait = match self.tick(&mut api).await {
                Tick::Idle => {
                    failures = 0;
                    IDLE_PROBE
                }
                Tick::Failed(d) => {
                    failures = failures.saturating_add(1);
                    d.max(retry_delay(failures.saturating_sub(1)))
                }
            };
            tokio::select! {
                () = self.handle.notify.notified() => {
                    // Let a burst of writes (a checkout touches several tables) settle into one round.
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
                () = tokio::time::sleep(wait) => {}
            }
        }
    }

    fn publish(&self, state: SyncState, last_error: Option<String>, success: bool, chain: Option<Vec<ChainBreakView>>) {
        let prev = self.handle.status();
        let status = SyncStatus {
            state,
            pending: prev.pending,
            dead: prev.dead,
            last_success_at: if success { Some(iso(self.clock.now())) } else { prev.last_success_at },
            last_error,
            chain_broken: chain.unwrap_or(prev.chain_broken),
        };
        self.handle.set(status.clone());
        self.events.status(&status);
    }

    async fn refresh_counts(&self) {
        if let Ok((pending, dead)) = outbox_counts(&self.db).await {
            let mut s = self.handle.status();
            s.pending = pending;
            s.dead = dead;
            self.handle.set(s);
        }
    }

    async fn tick(&self, api: &mut Option<(ServerConfig, String, Arc<dyn SyncApi>)>) -> Tick {
        self.refresh_counts().await;
        let device_id = match settings::device_get::<String>(&self.db.pool, keys::DEVICE_ID).await {
            Ok(Some(d)) => d,
            Ok(None) => {
                self.publish(SyncState::Unregistered, None, false, None);
                return Tick::Idle;
            }
            Err(e) => {
                self.publish(SyncState::Error, Some(e.to_string()), false, None);
                return Tick::Failed(Duration::from_secs(60));
            }
        };
        // A pending restore: a routine pull would move the cursor past this counter's own post-backup rows,
        // which the recovery pull must bring back.
        if let Ok(Some(_)) = crate::restore::pending(&self.db.pool).await {
            self.publish(SyncState::RestorePending, None, false, None);
            return Tick::Idle;
        }
        // After an offline restore, the retired identity's queued ops go first, under that identity.
        if let Ok(Some(retired)) = crate::restore::retired(&self.db.pool).await
            && let Ok(Some(cfg)) = config::load(&self.db.pool).await
        {
            match (self.connect)(&cfg, &retired.device_id, self.secrets.clone()) {
                Ok(old) => match engine::drain_retired(&self.db, old.as_ref(), self.clock.as_ref()).await {
                    Ok(_) => {}
                    Err(RoundError::Remote(SyncError::Offline(m))) => {
                        self.publish(SyncState::Offline, Some(m), false, None);
                        return Tick::Failed(Duration::ZERO);
                    }
                    Err(e) => log::warn!("sync: retired identity {}: {e}", retired.device_code),
                },
                Err(e) => log::warn!("sync: retired identity {}: {e}", retired.device_code),
            }
        }
        match registration::is_registered(&self.db, self.secrets.as_ref()).await {
            Ok(true) => {}
            Ok(false) => {
                self.publish(SyncState::Unregistered, None, false, None);
                return Tick::Idle;
            }
            Err(e) => {
                self.publish(SyncState::Error, Some(e.to_string()), false, None);
                return Tick::Failed(Duration::from_secs(60));
            }
        }
        if let Ok(Some(true)) = settings::device_get::<bool>(&self.db.pool, BOOTSTRAP_PENDING_KEY).await {
            self.publish(SyncState::BootstrapPending, None, false, None);
            return Tick::Idle;
        }
        let cfg = match config::load(&self.db.pool).await {
            Ok(Some(c)) => c,
            _ => {
                self.publish(SyncState::Unregistered, None, false, None);
                return Tick::Idle;
            }
        };
        if !matches!(api, Some((c, d, _)) if *c == cfg && *d == device_id) {
            match (self.connect)(&cfg, &device_id, self.secrets.clone()) {
                Ok(a) => *api = Some((cfg, device_id.clone(), a)),
                Err(e) => {
                    self.publish(SyncState::Error, Some(e.to_string()), false, None);
                    return Tick::Failed(Duration::from_secs(60));
                }
            }
        }
        let Some(client) = api.as_ref().map(|(_, _, c)| c.clone()) else {
            return Tick::Failed(Duration::from_secs(60));
        };
        self.publish(SyncState::Syncing, None, false, None);
        let result = async {
            let health = client.health().await?;
            let outcome = engine::round(&self.db, client.as_ref(), self.clock.as_ref(), &device_id).await?;
            Ok::<_, RoundError>((health, outcome))
        }
        .await;
        self.refresh_counts().await;
        match result {
            Ok((health, outcome)) => {
                if !outcome.pulled.tables.is_empty() {
                    self.events.applied(&outcome.pulled.tables);
                }
                let chain = health.chain_broken.iter().map(ChainBreakView::from).collect();
                let s = self.handle.status();
                let state = if s.pending > 0 || s.dead > 0 { SyncState::Pending } else { SyncState::Synced };
                self.publish(state, None, true, Some(chain));
                if outcome.pushed.retried > 0 { Tick::Failed(Duration::ZERO) } else { Tick::Idle }
            }
            Err(RoundError::Remote(SyncError::Offline(m))) => {
                self.publish(SyncState::Offline, Some(m), false, None);
                Tick::Failed(Duration::ZERO)
            }
            Err(RoundError::Remote(SyncError::Unauthorized(m))) => {
                self.publish(SyncState::Unauthorized, Some(m), false, None);
                Tick::Failed(Duration::from_secs(300))
            }
            Err(e) => {
                self.publish(SyncState::Error, Some(e.to_string()), false, None);
                Tick::Failed(Duration::ZERO)
            }
        }
    }
}
