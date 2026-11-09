//! Receipt printers (doc §7.2, §7.5): the transport trait and its three implementations (Windows spooler RAW,
//! TCP 9100, serial COM), the per-counter printer profile in `device_settings`, and the print worker.
//!
//! Transports are blocking on purpose: the worker always calls them on `spawn_blocking` under a hard
//! timeout, so a frozen spooler or a dead socket can stall only that one abandoned thread, never the till.

pub mod transport_network;
pub mod transport_serial;
pub mod transport_spooler;
pub mod worker;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use sqlx::SqlitePool;

use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::db::repo::WriteCtx;
use crate::db::{Db, settings};
use crate::error::{AppError, AppResult};
use crate::hardware::escpos::{CodePage, CutMode, DrawerPin, Layout, PaperWidth};

/// Why a transport call failed, as a stable code for the job's `last_error` and the till banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintFault {
    /// `CONNECT_FAILED`, `WRITE_FAILED`, `STATUS_FAILED`, `PRINTER_NOT_FOUND`, `UNSUPPORTED`, …
    pub code: &'static str,
    pub detail: String,
}

impl PrintFault {
    pub fn new(code: &'static str, detail: impl std::fmt::Display) -> Self {
        PrintFault { code, detail: detail.to_string() }
    }
}

/// Real-time printer state from `DLE EOT 2` / `DLE EOT 4`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrinterStatus {
    pub cover_open: bool,
    pub paper_out: bool,
    pub paper_near_end: bool,
    pub error: bool,
}

impl PrinterStatus {
    /// Decode the replies to `DLE EOT 2` (offline cause) and `DLE EOT 4` (paper sensor).
    pub fn decode(offline: u8, paper: u8) -> Self {
        PrinterStatus {
            cover_open: offline & 0x04 != 0,
            error: offline & 0x40 != 0,
            paper_out: paper & 0x60 != 0 || offline & 0x20 != 0,
            paper_near_end: paper & 0x0C != 0,
        }
    }

    /// The fault that stops printing, if any (near-end still prints).
    pub fn blocking_fault(self) -> Option<&'static str> {
        if self.paper_out {
            Some("PAPER_OUT")
        } else if self.cover_open {
            Some("COVER_OPEN")
        } else if self.error {
            Some("PRINTER_ERROR")
        } else {
            None
        }
    }
}

/// One way of getting bytes to a printer.
pub trait PrinterTransport: Send + Sync {
    fn send(&self, bytes: &[u8]) -> Result<(), PrintFault>;
    /// Real-time status where the transport can read back (network, serial); `Ok(None)` when it can't
    /// (the spooler hides the device).
    fn status(&self) -> Result<Option<PrinterStatus>, PrintFault>;
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransportConfig {
    /// A Windows printer queue, written with the RAW datatype (USB printers with a vendor/generic driver).
    Spooler { printer_name: String },
    /// TCP 9100.
    Network { host: String, port: u16 },
    /// `COM3` at the printer's baud rate.
    Serial { port: String, baud: u32 },
}

/// This counter's receipt printer (and the drawer on its RJ11 port). Device-local, not synced.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct ReceiptPrinterProfile {
    pub transport: TransportConfig,
    pub width: PaperWidth,
    pub codepage: CodePage,
    pub cut: CutMode,
    pub feed_lines: u8,
    pub qr: bool,
    pub drawer_pin: DrawerPin,
}

impl ReceiptPrinterProfile {
    pub fn layout(&self) -> Layout {
        Layout {
            width: self.width,
            codepage: self.codepage,
            cut: self.cut,
            feed_lines: self.feed_lines.min(10),
            qr: self.qr,
            drawer_pin: self.drawer_pin,
        }
    }
}

/// `device_settings` key of a printer profile (`printer_key` on `print_jobs`).
pub fn profile_key(printer_key: &str) -> String {
    format!("printer.{printer_key}")
}

pub async fn load_profile(pool: &SqlitePool, printer_key: &str) -> AppResult<Option<ReceiptPrinterProfile>> {
    settings::device_get(pool, &profile_key(printer_key)).await
}

/// Common receipt-printer baud rates.
const BAUDS: [u32; 6] = [9_600, 19_200, 38_400, 57_600, 115_200, 4_800];

fn validate(p: &ReceiptPrinterProfile) -> AppResult<()> {
    let bad = |why: &str| Err(AppError::validation("PRINTER_PROFILE_INVALID", why.to_owned()));
    match &p.transport {
        TransportConfig::Spooler { printer_name } => {
            let n = printer_name.trim().chars().count();
            if n == 0 || n > 200 {
                return bad("printer name");
            }
        }
        TransportConfig::Network { host, port } => {
            let h = host.trim();
            if h.is_empty() || h.len() > 253 || h.contains(char::is_whitespace) || *port == 0 {
                return bad("host or port");
            }
        }
        TransportConfig::Serial { port, baud } => {
            let digits = port.trim().strip_prefix("COM").unwrap_or("");
            if digits.is_empty() || digits.len() > 3 || !digits.chars().all(|c| c.is_ascii_digit()) {
                return bad("COM port");
            }
            if !BAUDS.contains(baud) {
                return bad("baud rate");
            }
        }
    }
    if p.feed_lines > 10 {
        return bad("feed lines");
    }
    Ok(())
}

/// Save this counter's printer profile (Settings → Printer, `SETTINGS_MANAGE`), audited as
/// `SETTINGS_CHANGE` with the before and after values.
pub async fn save_profile(
    db: &Db,
    ctx: &WriteCtx,
    actor: &Actor,
    printer_key: &str,
    profile: &ReceiptPrinterProfile,
) -> AppResult<()> {
    validate(profile)?;
    let before = load_profile(&db.pool, printer_key).await?;
    let key = profile_key(printer_key);
    let mut tx = db.write().await?;
    let conn = tx.conn();
    settings::device_set(conn, &key, profile).await?;
    audit::record(
        conn,
        &AuditCtx {
            store_id: &ctx.store_id,
            device_id: &ctx.device_id,
            business_date: &ctx.business_date,
            now: &ctx.now,
        },
        AuditEntry {
            action: AuditAction::SETTINGS_CHANGE,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("device_setting"),
            entity_id: Some(&key),
            metadata: json!({ "before": before, "after": profile }),
        },
    )
    .await?;
    tx.commit().await
}

/// Builds the transport for a profile; production uses [`connect`], tests inject fakes.
pub type Connector = Arc<dyn Fn(&TransportConfig) -> Arc<dyn PrinterTransport> + Send + Sync>;

/// The real transports.
pub fn connect(config: &TransportConfig) -> Arc<dyn PrinterTransport> {
    match config {
        TransportConfig::Spooler { printer_name } => Arc::new(transport_spooler::Spooler::new(printer_name)),
        TransportConfig::Network { host, port } => Arc::new(transport_network::Network::new(host, *port)),
        TransportConfig::Serial { port, baud } => Arc::new(transport_serial::Serial::new(port, *baud)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_dle_eot_replies() {
        // Fixed bits 1 and 4 are always set (0x12); everything else clear ⇒ ready.
        assert_eq!(PrinterStatus::decode(0x12, 0x12).blocking_fault(), None);
        assert_eq!(PrinterStatus::decode(0x12, 0x72).blocking_fault(), Some("PAPER_OUT"));
        assert_eq!(PrinterStatus::decode(0x16, 0x12).blocking_fault(), Some("COVER_OPEN"));
        assert_eq!(PrinterStatus::decode(0x52, 0x12).blocking_fault(), Some("PRINTER_ERROR"));
        let near = PrinterStatus::decode(0x12, 0x1E);
        assert!(near.paper_near_end && near.blocking_fault().is_none());
    }
}
