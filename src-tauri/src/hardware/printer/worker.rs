//! The print worker (doc §7.5): one tokio task per printer, **at most one in-flight job**, hard I/O timeouts,
//! an unhealthy state that never piles up threads, `DLE EOT` paper/cover checks before each job, retries with
//! exponential backoff, and startup recovery that never reprints silently.
//!
//! The sale never waits for any of this: `checkout_order` commits its `print_jobs` row and only *pings* the
//! worker through an unbounded channel (a non-blocking send).
//!
//! Every transport call runs on a dedicated OS thread under a timeout. On a timeout the job fails, the printer
//! turns **unhealthy** and the blocked thread is abandoned (it can't be killed); no further thread is started
//! for that printer until the abandoned one has returned and a health probe succeeds. A plain thread rather
//! than `spawn_blocking`, so an abandoned call can never hold up runtime shutdown when the app exits.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono_tz::Tz;
use serde::Serialize;
use specta::Type;
use sqlx::FromRow;
use tokio::sync::{mpsc, oneshot};

use super::{AnyProfile, Connector, PrintFault, PrinterTransport, load_any};
use crate::clock::{Clock, iso};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::hardware::escpos::render;

#[derive(Debug, Clone, Copy)]
pub struct WorkerConfig {
    /// Hard limit on one transport call (connect + write, or a status query).
    pub io_timeout: Duration,
    pub backoff_base: Duration,
    pub backoff_max: Duration,
    /// Attempts before a job is left `failed` for the cashier's Retry.
    pub max_attempts: i64,
    /// How often an unhealthy printer is re-probed.
    pub probe_interval: Duration,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        WorkerConfig {
            io_timeout: Duration::from_secs(5),
            backoff_base: Duration::from_secs(2),
            backoff_max: Duration::from_secs(30),
            max_attempts: 5,
            probe_interval: Duration::from_secs(5),
        }
    }
}

impl WorkerConfig {
    /// 2 s, 4 s, 8 s, … capped at 30 s.
    pub fn backoff(&self, attempts: i64) -> Duration {
        let exp = u32::try_from(attempts.clamp(1, 16) - 1).unwrap_or(0);
        self.backoff_base.saturating_mul(2u32.saturating_pow(exp)).min(self.backoff_max)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrinterHealth {
    /// No profile in `device_settings` yet: jobs wait, selling continues.
    NotConfigured,
    Ready,
    /// The last call failed (paper out, cover open, connect/write error); see `fault`.
    Fault,
    /// A call timed out; its thread is abandoned and the printer is probed until it answers again.
    Unhealthy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthState {
    pub health: PrinterHealth,
    pub fault: Option<String>,
}

struct Shared {
    health: Mutex<HealthState>,
    /// Jobs found unprinted at startup: never printed until someone chooses "Print now".
    held: Mutex<HashSet<String>>,
    threads: AtomicU64,
}

/// The handle the app keeps: ping after a commit, read the health for the banner, resolve the recovery list.
#[derive(Clone)]
pub struct PrintHub {
    tx: mpsc::UnboundedSender<()>,
    shared: Arc<Shared>,
    pub printer_key: String,
}

/// Called after every state change (the app emits `db://changed` for `print_jobs`; tests ignore it).
pub type OnChange = Arc<dyn Fn() + Send + Sync>;

fn lock<T>(m: &Mutex<T>) -> AppResult<std::sync::MutexGuard<'_, T>> {
    m.lock().map_err(|_| AppError::internal("print worker lock poisoned"))
}

impl PrintHub {
    /// Load the recovery list, then start the worker task. Must run inside a tokio runtime.
    pub async fn start(
        db: Db,
        printer_key: &str,
        connector: Connector,
        clock: Arc<dyn Clock>,
        tz: Tz,
        config: WorkerConfig,
        on_change: OnChange,
    ) -> AppResult<PrintHub> {
        let held: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM print_jobs WHERE printer_key = ?1 AND status IN ('queued', 'printing', 'failed')",
        )
        .bind(printer_key)
        .fetch_all(&db.pool)
        .await?;
        let shared = Arc::new(Shared {
            health: Mutex::new(HealthState { health: PrinterHealth::Ready, fault: None }),
            held: Mutex::new(held.into_iter().collect()),
            threads: AtomicU64::new(0),
        });
        let (tx, rx) = mpsc::unbounded_channel();
        let worker = Worker {
            db,
            printer_key: printer_key.to_owned(),
            connector,
            clock,
            tz,
            config,
            shared: shared.clone(),
            on_change,
            stuck: None,
            retry_at: HashMap::new(),
        };
        tokio::spawn(worker.run(rx));
        let hub = PrintHub { tx, shared, printer_key: printer_key.to_owned() };
        hub.notify();
        Ok(hub)
    }

    /// Wake the worker (a job was committed, a job was retried, the profile changed). Never blocks.
    pub fn notify(&self) {
        let _ = self.tx.send(());
    }

    pub fn health(&self) -> AppResult<HealthState> {
        Ok(lock(&self.shared.health)?.clone())
    }

    /// Jobs from before the restart that are waiting for "Print now / Skip".
    pub fn held(&self) -> AppResult<Vec<String>> {
        let mut ids: Vec<String> = lock(&self.shared.held)?.iter().cloned().collect();
        ids.sort();
        Ok(ids)
    }

    /// Release the recovery list; the caller requeues (print now) or abandons (skip) the returned ids.
    pub fn take_held(&self) -> AppResult<Vec<String>> {
        let mut held = lock(&self.shared.held)?;
        let mut ids: Vec<String> = held.drain().collect();
        ids.sort();
        Ok(ids)
    }

    /// Transport threads started so far (the "no pile-up" proof).
    pub fn threads_started(&self) -> u64 {
        self.shared.threads.load(Ordering::SeqCst)
    }
}

#[derive(FromRow)]
struct Job {
    id: String,
    kind: String,
    payload: String,
    attempts: i64,
}

/// An abandoned transport thread: `done` flips when (if ever) the blocked call returns.
struct Stuck {
    done: Arc<AtomicBool>,
}

enum Call<T> {
    Done(Result<T, PrintFault>),
    TimedOut(Stuck),
}

struct Worker {
    db: Db,
    printer_key: String,
    connector: Connector,
    clock: Arc<dyn Clock>,
    tz: Tz,
    config: WorkerConfig,
    shared: Arc<Shared>,
    on_change: OnChange,
    stuck: Option<Stuck>,
    retry_at: HashMap<String, Instant>,
}

impl Worker {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<()>) {
        loop {
            let next = match self.cycle().await {
                Ok(next) => next,
                Err(e) => {
                    log::error!("print worker {}: {e}", self.printer_key);
                    Some(self.config.probe_interval)
                }
            };
            let woke = match next {
                Some(delay) => tokio::select! {
                    m = rx.recv() => m.is_some(),
                    () = tokio::time::sleep(delay) => true,
                },
                None => rx.recv().await.is_some(),
            };
            if !woke {
                return; // the hub was dropped
            }
            while rx.try_recv().is_ok() {}
        }
    }

    fn set_health(&self, health: PrinterHealth, fault: Option<&str>) {
        if let Ok(mut h) = self.shared.health.lock() {
            let next = HealthState { health, fault: fault.map(str::to_owned) };
            if *h != next {
                *h = next;
                drop(h);
                (self.on_change)();
            }
        }
    }

    fn health(&self) -> PrinterHealth {
        self.shared.health.lock().map(|h| h.health).unwrap_or(PrinterHealth::Unhealthy)
    }

    /// One transport call on its own thread, under the hard timeout.
    async fn call<T, F>(&self, transport: Arc<dyn PrinterTransport>, f: F) -> Call<T>
    where
        T: Send + 'static,
        F: FnOnce(&dyn PrinterTransport) -> Result<T, PrintFault> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let done = Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        self.shared.threads.fetch_add(1, Ordering::SeqCst);
        let spawned = std::thread::Builder::new().name(format!("printer-{}", self.printer_key)).spawn(move || {
            let r = f(transport.as_ref());
            flag.store(true, Ordering::SeqCst);
            let _ = tx.send(r);
        });
        if let Err(e) = spawned {
            return Call::Done(Err(PrintFault::new("THREAD_FAILED", e)));
        }
        match tokio::time::timeout(self.config.io_timeout, rx).await {
            Ok(Ok(r)) => Call::Done(r),
            Ok(Err(_)) => Call::Done(Err(PrintFault::new("TRANSPORT_CRASHED", "the transport thread ended"))),
            Err(_) => Call::TimedOut(Stuck { done }),
        }
    }

    async fn set_job(&self, id: &str, status: &str, attempts: Option<i64>, error: Option<&str>) -> AppResult<()> {
        let mut tx = self.db.write().await?;
        sqlx::query(
            "UPDATE print_jobs SET status = ?1, attempts = COALESCE(?2, attempts), last_error = ?3, updated_at = ?4
             WHERE id = ?5",
        )
        .bind(status)
        .bind(attempts)
        .bind(error)
        .bind(iso(self.clock.now()))
        .bind(id)
        .execute(tx.conn())
        .await?;
        tx.commit().await?;
        (self.on_change)();
        Ok(())
    }

    /// Paper out / cover open: every waiting job is reported failed ("2 receipts waiting [Retry]").
    async fn fail_waiting(&self, code: &str) -> AppResult<()> {
        let held = lock(&self.shared.held)?.clone();
        let ids: Vec<String> =
            sqlx::query_scalar("SELECT id FROM print_jobs WHERE printer_key = ?1 AND status IN ('queued', 'printing')")
                .bind(&self.printer_key)
                .fetch_all(&self.db.pool)
                .await?;
        for id in ids.iter().filter(|id| !held.contains(*id)) {
            self.set_job(id, "failed", None, Some(code)).await?;
        }
        Ok(())
    }

    /// The next job to print, or how long until a backed-off one is due. Drawer kicks go before the receipt
    /// of the same sale (same `created_at`).
    async fn next_job(&self) -> AppResult<(Option<Job>, Option<Duration>)> {
        let jobs: Vec<Job> = sqlx::query_as(
            "SELECT id, kind, payload, attempts FROM print_jobs WHERE printer_key = ?1 AND status = 'queued'
             ORDER BY created_at, CASE kind WHEN 'drawer_kick' THEN 0 ELSE 1 END, id",
        )
        .bind(&self.printer_key)
        .fetch_all(&self.db.pool)
        .await?;
        let held = lock(&self.shared.held)?.clone();
        let now = Instant::now();
        let mut wait: Option<Duration> = None;
        for job in jobs {
            if held.contains(&job.id) {
                continue;
            }
            match self.retry_at.get(&job.id) {
                Some(at) if *at > now => {
                    let d = *at - now;
                    wait = Some(wait.map_or(d, |w| w.min(d)));
                }
                _ => return Ok((Some(job), None)),
            }
        }
        Ok((None, wait))
    }

    /// Print everything that is due. Returns when to look again (`None` = wait for a ping).
    async fn cycle(&mut self) -> AppResult<Option<Duration>> {
        // An abandoned call still blocked: don't touch the printer, not even to probe it.
        if let Some(stuck) = &self.stuck {
            if !stuck.done.load(Ordering::SeqCst) {
                return Ok(Some(self.config.probe_interval));
            }
            self.stuck = None;
        }
        let Some(profile) = load_any(&self.db.pool, &self.printer_key).await? else {
            self.set_health(PrinterHealth::NotConfigured, None);
            return Ok(None);
        };
        let transport = (self.connector)(profile.transport());
        let reads_status = profile.reads_status();

        // Unhealthy: one probe; stay unhealthy (and idle) until it answers. A label printer has no ESC/POS
        // status, so the abandoned call returning is its probe.
        if self.health() == PrinterHealth::Unhealthy {
            match self.call(transport.clone(), move |t| if reads_status { t.status() } else { Ok(None) }).await {
                Call::TimedOut(stuck) => {
                    self.stuck = Some(stuck);
                    return Ok(Some(self.config.probe_interval));
                }
                Call::Done(Err(f)) => {
                    log::warn!("printer {} probe failed: {} {}", self.printer_key, f.code, f.detail);
                    return Ok(Some(self.config.probe_interval));
                }
                Call::Done(Ok(_)) => self.set_health(PrinterHealth::Ready, None),
            }
        }

        loop {
            let (job, wait) = self.next_job().await?;
            let Some(job) = job else { return Ok(wait) };

            // Paper and cover first, where the transport can tell (network, serial; receipt printers only).
            match self.call(transport.clone(), move |t| if reads_status { t.status() } else { Ok(None) }).await {
                Call::TimedOut(stuck) => {
                    self.set_job(&job.id, "failed", Some(job.attempts + 1), Some("TIMEOUT")).await?;
                    self.set_health(PrinterHealth::Unhealthy, Some("TIMEOUT"));
                    self.stuck = Some(stuck);
                    return Ok(Some(self.config.probe_interval));
                }
                Call::Done(Ok(Some(status))) => {
                    if let Some(code) = status.blocking_fault() {
                        self.fail_waiting(code).await?;
                        self.set_health(PrinterHealth::Fault, Some(code));
                        return Ok(None);
                    }
                }
                Call::Done(Ok(None)) => {}
                Call::Done(Err(f)) => {
                    self.failed_attempt(&job, &f).await?;
                    continue;
                }
            }

            let payload: serde_json::Value = serde_json::from_str(&job.payload)?;
            let now = iso(self.clock.now());
            let rendered = match &profile {
                AnyProfile::Receipt(p) => {
                    render::job(&self.db.pool, &job.kind, &payload, p.layout(), self.tz, &now).await
                }
                AnyProfile::Label(p) => crate::hardware::label::render_job(&self.db.pool, &job.kind, &payload, p).await,
            };
            let bytes = match rendered {
                Ok(b) => b,
                Err(e) => {
                    log::error!("print job {} could not be rendered: {e}", job.id);
                    self.set_job(&job.id, "failed", Some(job.attempts + 1), Some("RENDER_FAILED")).await?;
                    continue;
                }
            };
            self.set_job(&job.id, "printing", Some(job.attempts + 1), None).await?;
            match self.call(transport.clone(), move |t| t.send(&bytes)).await {
                Call::TimedOut(stuck) => {
                    self.set_job(&job.id, "failed", None, Some("TIMEOUT")).await?;
                    self.set_health(PrinterHealth::Unhealthy, Some("TIMEOUT"));
                    self.stuck = Some(stuck);
                    return Ok(Some(self.config.probe_interval));
                }
                Call::Done(Ok(())) => {
                    self.retry_at.remove(&job.id);
                    self.set_job(&job.id, "done", None, None).await?;
                    self.set_health(PrinterHealth::Ready, None);
                }
                Call::Done(Err(f)) => {
                    // `attempts` was already counted when the job went to `printing`.
                    let counted = Job { attempts: job.attempts + 1, ..job };
                    self.retry_or_fail(&counted, &f).await?;
                }
            }
        }
    }

    /// A status query failed (couldn't connect): counts as an attempt.
    async fn failed_attempt(&mut self, job: &Job, f: &PrintFault) -> AppResult<()> {
        let counted =
            Job { id: job.id.clone(), kind: job.kind.clone(), payload: String::new(), attempts: job.attempts + 1 };
        self.retry_or_fail(&counted, f).await
    }

    async fn retry_or_fail(&mut self, job: &Job, f: &PrintFault) -> AppResult<()> {
        log::warn!("print job {} attempt {} failed: {} {}", job.id, job.attempts, f.code, f.detail);
        self.set_health(PrinterHealth::Fault, Some(f.code));
        if job.attempts >= self.config.max_attempts {
            self.retry_at.remove(&job.id);
            self.set_job(&job.id, "failed", Some(job.attempts), Some(f.code)).await
        } else {
            self.retry_at.insert(job.id.clone(), Instant::now() + self.config.backoff(job.attempts));
            self.set_job(&job.id, "queued", Some(job.attempts), Some(f.code)).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_to_thirty_seconds() {
        let c = WorkerConfig::default();
        let s: Vec<u64> = (1..=6).map(|a| c.backoff(a).as_secs()).collect();
        assert_eq!(s, vec![2, 4, 8, 16, 30, 30]);
    }
}
