//! Block 1.7 — the printing pipeline (doc §7.2–§7.5, §13.3): the print worker against a fake transport that
//! prints, hangs forever, errors and reports paper-out; startup recovery; and the TCP 9100 transport against a
//! local socket. The headline assertion: a checkout completes in < 100 ms while the printer is hung, the job
//! ends `failed`, and no second thread is started for that printer.

mod common;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use common::{CASHIER, CASHIER_PIN, Till, cart, cash, line, till};
use gayanshop_lib::commands::checkout::sell;
use gayanshop_lib::db::repo::checkout::CheckoutInput;
use gayanshop_lib::db::repo::print_jobs::{self, PrintKind, RECEIPT_PRINTER};
use gayanshop_lib::db::settings;
use gayanshop_lib::hardware::escpos::{CodePage, CutMode, DrawerPin, PaperWidth, drawer_kick};
use gayanshop_lib::hardware::printer::transport_network::Network;
use gayanshop_lib::hardware::printer::worker::{PrintHub, PrinterHealth, WorkerConfig};
use gayanshop_lib::hardware::printer::{
    PrintFault, PrinterStatus, PrinterTransport, ReceiptPrinterProfile, TransportConfig, profile_key,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Print,
    /// `send` blocks until released (a frozen spooler).
    Hang,
    /// `send` fails (printer unplugged).
    Error,
    /// `DLE EOT 4` reports the roll is empty.
    PaperOut,
}

struct Fake {
    mode: Mutex<Mode>,
    released: (Mutex<bool>, Condvar),
    sent: Mutex<Vec<Vec<u8>>>,
    sends: AtomicUsize,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
}

impl Fake {
    fn new(mode: Mode) -> Arc<Fake> {
        Arc::new(Fake {
            mode: Mutex::new(mode),
            released: (Mutex::new(false), Condvar::new()),
            sent: Mutex::new(Vec::new()),
            sends: AtomicUsize::new(0),
            in_flight: AtomicUsize::new(0),
            max_in_flight: AtomicUsize::new(0),
        })
    }

    fn set(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
    }

    fn release(&self) {
        *self.released.0.lock().unwrap() = true;
        self.released.1.notify_all();
    }

    fn sent(&self) -> Vec<Vec<u8>> {
        self.sent.lock().unwrap().clone()
    }

    fn enter(&self) {
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_in_flight.fetch_max(now, Ordering::SeqCst);
    }
}

impl PrinterTransport for Fake {
    fn send(&self, bytes: &[u8]) -> Result<(), PrintFault> {
        self.enter();
        self.sends.fetch_add(1, Ordering::SeqCst);
        let mode = *self.mode.lock().unwrap();
        let r = match mode {
            Mode::Hang => {
                let mut released = self.released.0.lock().unwrap();
                while !*released {
                    released = self.released.1.wait(released).unwrap();
                }
                self.sent.lock().unwrap().push(bytes.to_vec());
                Ok(())
            }
            Mode::Error => Err(PrintFault::new("WRITE_FAILED", "unplugged")),
            Mode::Print | Mode::PaperOut => {
                self.sent.lock().unwrap().push(bytes.to_vec());
                Ok(())
            }
        };
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        r
    }

    fn status(&self) -> Result<Option<PrinterStatus>, PrintFault> {
        self.enter();
        let r = match *self.mode.lock().unwrap() {
            Mode::PaperOut => Some(PrinterStatus { paper_out: true, ..PrinterStatus::default() }),
            Mode::Hang => None, // the spooler can't report status
            _ => Some(PrinterStatus::default()),
        };
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(r)
    }
}

fn fast() -> WorkerConfig {
    WorkerConfig {
        io_timeout: Duration::from_millis(300),
        backoff_base: Duration::from_millis(20),
        backoff_max: Duration::from_millis(80),
        max_attempts: 5,
        probe_interval: Duration::from_millis(50),
    }
}

fn profile() -> ReceiptPrinterProfile {
    ReceiptPrinterProfile {
        transport: TransportConfig::Network { host: "fake".into(), port: 9100 },
        width: PaperWidth::Mm80,
        codepage: CodePage::Pc437,
        cut: CutMode::Partial,
        feed_lines: 4,
        qr: false,
        drawer_pin: DrawerPin::Pin2,
        open_drawer: true,
    }
}

async fn configure(t: &Till) {
    let mut tx = t.t.db.write().await.unwrap();
    settings::device_set(tx.conn(), &profile_key(RECEIPT_PRINTER), &profile()).await.unwrap();
    tx.commit().await.unwrap();
}

async fn hub(t: &Till, fake: &Arc<Fake>) -> PrintHub {
    let f = fake.clone();
    PrintHub::start(
        t.t.db.clone(),
        RECEIPT_PRINTER,
        Arc::new(move |_| f.clone() as Arc<dyn PrinterTransport>),
        Arc::new(gayanshop_lib::clock::SystemClock),
        chrono_tz::Asia::Colombo,
        fast(),
        Arc::new(|| {}),
    )
    .await
    .unwrap()
}

async fn wait_until<F, Fut>(what: &str, mut cond: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cond().await {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn jobs(t: &Till) -> Vec<(String, String, i64, Option<String>)> {
    sqlx::query_as("SELECT kind, status, attempts, last_error FROM print_jobs ORDER BY created_at, kind DESC, id")
        .fetch_all(t.pool())
        .await
        .unwrap()
}

async fn status_count(t: &Till, status: &str) -> i64 {
    t.count(&format!("SELECT COUNT(*) FROM print_jobs WHERE status = '{status}'")).await
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn cash_sale(t: &Till, token: &str, product: &str) -> CheckoutInput {
    let _ = t;
    let mut input = cart(token, vec![line(product, 25_000, 1)], vec![cash(25_000, Some(30_000))]);
    input.customer_phone = Some("077 123 4567".into());
    input
}

async fn sell_now(t: &Till, input: &CheckoutInput) {
    sell(&t.auth, &t.clock, &t.t.db, &t.ctx(), input).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cash_sale_kicks_the_drawer_then_prints_the_receipt() {
    let t = till().await;
    t.login(CASHIER, CASHIER_PIN).await;
    configure(&t).await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let fake = Fake::new(Mode::Print);
    let hub = hub(&t, &fake).await;

    sell_now(&t, &cash_sale(&t, "p1", &mug)).await;
    hub.notify();
    wait_until("both jobs done", || async { status_count(&t, "done").await == 2 }).await;

    let sent = fake.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0], drawer_kick(DrawerPin::Pin2), "the kick goes first, on its own");
    let receipt = &sent[1];
    for needle in [
        "GayanShop".as_bytes(),
        b"T01-20261108-0001",
        b"Mug",
        b"Rs. 250.00",
        b"Change",
        b"Customer: 07X XXX X567",
        b"Exchange within 7 days.",
    ] {
        assert!(contains(receipt, needle), "receipt lacks {:?}", String::from_utf8_lossy(needle));
    }
    assert!(!contains(receipt, b"771234567"), "the phone is masked");
    assert!(contains(receipt, &[0x1D, b'k', 73]), "Code128 barcode of receipt_no");
    assert!(contains(receipt, b"Powered By BraceFrame - (076 9950 199)"), "BraceFrame branding");
    assert!(receipt.ends_with(&[0x1B, b'd', 4, 0x1D, b'V', 1]), "feed then partial cut");
    assert_eq!(hub.health().unwrap().health, PrinterHealth::Ready);
    assert_eq!(fake.max_in_flight.load(Ordering::SeqCst), 1, "one call at a time");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_drawer_off_a_cash_sale_prints_the_receipt_and_sends_no_kick() {
    let t = till().await;
    t.login(CASHIER, CASHIER_PIN).await;
    let mut tx = t.t.db.write().await.unwrap();
    let no_drawer = ReceiptPrinterProfile { open_drawer: false, ..profile() };
    settings::device_set(tx.conn(), &profile_key(RECEIPT_PRINTER), &no_drawer).await.unwrap();
    tx.commit().await.unwrap();
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let fake = Fake::new(Mode::Print);
    let hub = hub(&t, &fake).await;

    sell_now(&t, &cash_sale(&t, "p1", &mug)).await;
    hub.notify();
    wait_until("both jobs done", || async { status_count(&t, "done").await == 2 }).await;

    let sent = fake.sent();
    assert_eq!(sent.len(), 1, "only the receipt reaches the printer");
    assert!(contains(&sent[0], b"Mug"));
    assert!(!contains(&sent[0], &drawer_kick(DrawerPin::Pin2)), "no kick bytes anywhere");
    assert_eq!(status_count(&t, "failed").await, 0);
}

#[test]
fn a_profile_saved_before_the_drawer_switch_keeps_the_drawer_on() {
    let old = r#"{"transport":{"kind":"network","host":"h","port":9100},"width":"mm80","codepage":"pc437","cut":"partial","feed_lines":4,"qr":true,"drawer_pin":"pin2"}"#;
    let p: ReceiptPrinterProfile = serde_json::from_str(old).unwrap();
    assert!(p.open_drawer);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hung_printer_never_slows_a_sale_or_piles_up_threads() {
    let t = till().await;
    t.login(CASHIER, CASHIER_PIN).await;
    configure(&t).await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let fake = Fake::new(Mode::Hang);
    let hub = hub(&t, &fake).await;

    sell_now(&t, &cash_sale(&t, "h1", &mug)).await;
    hub.notify();
    wait_until("the printer is stuck in a call", || async { fake.in_flight.load(Ordering::SeqCst) == 1 }).await;

    // The sale path never touches the printer: a checkout (+ ping) is fast while the call is hung.
    let input = cash_sale(&t, "h2", &mug);
    let started = Instant::now();
    sell_now(&t, &input).await;
    hub.notify();
    let took = started.elapsed();
    assert!(took < Duration::from_millis(100), "checkout took {took:?} with the printer hung");

    wait_until("the hung job fails", || async {
        t.count("SELECT COUNT(*) FROM print_jobs WHERE status = 'failed' AND last_error = 'TIMEOUT'").await == 1
    })
    .await;
    let health = hub.health().unwrap();
    assert_eq!((health.health, health.fault.as_deref()), (PrinterHealth::Unhealthy, Some("TIMEOUT")));

    // More sales while hung: nothing new is started for this printer, however long it stays stuck.
    let threads = hub.threads_started();
    for i in 3..6 {
        sell_now(&t, &cash_sale(&t, &format!("h{i}"), &mug)).await;
        hub.notify();
    }
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(hub.threads_started(), threads, "no thread pile-up while the call is hung");
    assert_eq!(fake.sends.load(Ordering::SeqCst), 1);
    assert_eq!(fake.max_in_flight.load(Ordering::SeqCst), 1);
    assert_eq!(t.count("SELECT COUNT(*) FROM orders WHERE status = 'completed'").await, 5, "selling continued");

    // The spooler comes back: the abandoned call returns, a probe succeeds, the queue drains in order.
    fake.set(Mode::Print);
    fake.release();
    wait_until("the queue drains", || async { status_count(&t, "done").await == 9 }).await;
    wait_until("the printer is ready", || async { hub.health().unwrap().health == PrinterHealth::Ready }).await;
    assert_eq!(status_count(&t, "failed").await, 1, "the timed-out job waits for Retry, never reprints alone");
    assert_eq!(t.count("SELECT COUNT(*) FROM print_jobs WHERE status IN ('queued', 'printing')").await, 0);
    assert_eq!(fake.max_in_flight.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_errors_retry_with_backoff_then_fail_until_retried() {
    let t = till().await;
    configure(&t).await;
    let fake = Fake::new(Mode::Error);
    let hub = hub(&t, &fake).await;
    {
        let mut tx = t.t.db.write().await.unwrap();
        print_jobs::enqueue(tx.conn(), &t.ctx(), PrintKind::Test, None, &serde_json::json!({})).await.unwrap();
        tx.commit().await.unwrap();
    }
    hub.notify();
    wait_until("the job fails", || async { status_count(&t, "failed").await == 1 }).await;
    assert_eq!(jobs(&t).await, vec![("test".into(), "failed".into(), 5, Some("WRITE_FAILED".into()))]);
    assert_eq!(fake.sends.load(Ordering::SeqCst), 5, "five attempts");
    let health = hub.health().unwrap();
    assert_eq!((health.health, health.fault.as_deref()), (PrinterHealth::Fault, Some("WRITE_FAILED")));

    // Retry (the banner button) requeues it with fresh attempts.
    fake.set(Mode::Print);
    assert_eq!(print_jobs::retry_failed(&t.t.db, &t.ctx(), RECEIPT_PRINTER).await.unwrap(), 1);
    hub.notify();
    wait_until("the retried job prints", || async { status_count(&t, "done").await == 1 }).await;
    let page = &fake.sent()[0];
    assert!(contains(page, b"SAMPLE RECEIPT") && page.ends_with(&drawer_kick(DrawerPin::Pin2)));
    assert_eq!(hub.health().unwrap().health, PrinterHealth::Ready);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paper_out_fails_the_waiting_jobs_and_retry_prints_them() {
    let t = till().await;
    t.login(CASHIER, CASHIER_PIN).await;
    configure(&t).await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let fake = Fake::new(Mode::PaperOut);
    let hub = hub(&t, &fake).await;

    sell_now(&t, &cash_sale(&t, "po1", &mug)).await;
    hub.notify();
    wait_until("both jobs fail", || async { status_count(&t, "failed").await == 2 }).await;
    assert_eq!(t.count("SELECT COUNT(*) FROM print_jobs WHERE last_error = 'PAPER_OUT'").await, 2);
    assert_eq!(fake.sends.load(Ordering::SeqCst), 0, "nothing is sent to an empty roll");
    let health = hub.health().unwrap();
    assert_eq!((health.health, health.fault.as_deref()), (PrinterHealth::Fault, Some("PAPER_OUT")));
    assert_eq!(print_jobs::counts(t.pool(), RECEIPT_PRINTER, &[]).await.unwrap(), (0, 2));

    fake.set(Mode::Print);
    assert_eq!(print_jobs::retry_failed(&t.t.db, &t.ctx(), RECEIPT_PRINTER).await.unwrap(), 2);
    hub.notify();
    wait_until("both print", || async { status_count(&t, "done").await == 2 }).await;
    assert_eq!(fake.sent()[0], drawer_kick(DrawerPin::Pin2));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jobs_from_before_a_restart_wait_for_print_now_or_skip() {
    let t = till().await;
    t.login(CASHIER, CASHIER_PIN).await;
    configure(&t).await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    sell_now(&t, &cash_sale(&t, "r1", &mug)).await; // committed, then "the app crashed"
    let fake = Fake::new(Mode::Print);
    let hub = hub(&t, &fake).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(fake.sent().is_empty(), "no silent reprint after a restart");
    let held = hub.held().unwrap();
    assert_eq!(held.len(), 2);

    // New sales still print; the recovered ones keep waiting.
    sell_now(&t, &cash_sale(&t, "r2", &mug)).await;
    hub.notify();
    wait_until("the new sale prints", || async { status_count(&t, "done").await == 2 }).await;
    assert_eq!(hub.held().unwrap().len(), 2);

    // "Print now".
    let ids = hub.take_held().unwrap();
    print_jobs::requeue(&t.t.db, &t.ctx(), &ids).await.unwrap();
    hub.notify();
    wait_until("the recovered jobs print", || async { status_count(&t, "done").await == 4 }).await;
    assert!(hub.held().unwrap().is_empty());

    // A second restart with one unprinted job, skipped this time: abandoned and audited.
    sell_now(&t, &cash_sale(&t, "r3", &mug)).await;
    let again = hub_after_restart(&t).await;
    let ids = again.take_held().unwrap();
    assert_eq!(ids.len(), 2);
    let owner = t.owner();
    assert_eq!(print_jobs::abandon(&t.t.db, &t.ctx(), &owner, &ids, "skipped after restart").await.unwrap(), 2);
    assert_eq!(status_count(&t, "abandoned").await, 2);
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'PRINT_JOB_ABANDON'").await, 1);
}

async fn hub_after_restart(t: &Till) -> PrintHub {
    let fake = Fake::new(Mode::Print);
    hub(t, &fake).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_printer_profile_jobs_wait_and_selling_continues() {
    let t = till().await;
    t.login(CASHIER, CASHIER_PIN).await;
    let mug = t.product("Mug", 25_000, false, 0, None).await;
    let fake = Fake::new(Mode::Print);
    let hub = hub(&t, &fake).await;
    sell_now(&t, &cash_sale(&t, "n1", &mug)).await;
    hub.notify();
    wait_until("not configured", || async { hub.health().unwrap().health == PrinterHealth::NotConfigured }).await;
    assert_eq!(status_count(&t, "queued").await, 2);
    assert_eq!(fake.sends.load(Ordering::SeqCst), 0);

    configure(&t).await;
    hub.notify();
    wait_until("printed once configured", || async { status_count(&t, "done").await == 2 }).await;
}

#[test]
fn network_transport_speaks_raw_9100_and_dle_eot() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        // 1st connection: status queries (cover closed, paper out).
        let (mut s, _) = listener.accept().unwrap();
        let mut q = [0u8; 3];
        s.read_exact(&mut q).unwrap();
        assert_eq!(q, [0x10, 0x04, 0x02]);
        s.write_all(&[0x12]).unwrap();
        s.read_exact(&mut q).unwrap();
        assert_eq!(q, [0x10, 0x04, 0x04]);
        s.write_all(&[0x72]).unwrap();
        // 2nd connection: a job.
        let (mut s, _) = listener.accept().unwrap();
        let mut got = Vec::new();
        s.read_to_end(&mut got).unwrap();
        got
    });
    let net = Network::new("127.0.0.1", port);
    let status = net.status().unwrap().unwrap();
    assert_eq!(status.blocking_fault(), Some("PAPER_OUT"));
    net.send(b"\x1b@hello").unwrap();
    assert_eq!(server.join().unwrap(), b"\x1b@hello");

    // Nothing listening: a fast, typed failure rather than a hang.
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let started = Instant::now();
    let err = Network::new("127.0.0.1", closed).send(b"x").unwrap_err();
    assert_eq!(err.code, "CONNECT_FAILED");
    assert!(started.elapsed() < Duration::from_secs(3));
}
