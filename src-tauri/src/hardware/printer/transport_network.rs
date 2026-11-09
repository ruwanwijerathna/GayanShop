//! TCP 9100 ("raw" / JetDirect) with the doc §7.5 timeouts: connect 2 s, write 5 s. One connection per job,
//! so a printer that was power-cycled between sales just works on the next one.

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use super::{PrintFault, PrinterStatus, PrinterTransport};
use crate::hardware::escpos::{STATUS_OFFLINE, STATUS_PAPER};

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(2);

pub struct Network {
    host: String,
    port: u16,
}

impl Network {
    pub fn new(host: &str, port: u16) -> Self {
        Network { host: host.trim().to_owned(), port }
    }

    fn open(&self) -> Result<TcpStream, PrintFault> {
        let addrs: Vec<SocketAddr> = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|e| PrintFault::new("PRINTER_NOT_FOUND", e))?
            .collect();
        let mut last = PrintFault::new("PRINTER_NOT_FOUND", &self.host);
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
                Ok(s) => {
                    s.set_write_timeout(Some(WRITE_TIMEOUT)).map_err(|e| PrintFault::new("CONNECT_FAILED", e))?;
                    s.set_read_timeout(Some(READ_TIMEOUT)).map_err(|e| PrintFault::new("CONNECT_FAILED", e))?;
                    s.set_nodelay(true).map_err(|e| PrintFault::new("CONNECT_FAILED", e))?;
                    return Ok(s);
                }
                Err(e) => last = PrintFault::new("CONNECT_FAILED", format!("{addr}: {e}")),
            }
        }
        Err(last)
    }
}

/// Send one `DLE EOT n` and read its one-byte reply.
pub(super) fn query<S: Read + Write>(s: &mut S, cmd: &[u8; 3]) -> Result<u8, PrintFault> {
    s.write_all(cmd).map_err(|e| PrintFault::new("STATUS_FAILED", e))?;
    s.flush().map_err(|e| PrintFault::new("STATUS_FAILED", e))?;
    let mut b = [0u8; 1];
    s.read_exact(&mut b).map_err(|e| PrintFault::new("STATUS_FAILED", e))?;
    Ok(b[0])
}

impl PrinterTransport for Network {
    fn send(&self, bytes: &[u8]) -> Result<(), PrintFault> {
        let mut s = self.open()?;
        s.write_all(bytes).map_err(|e| PrintFault::new("WRITE_FAILED", e))?;
        s.flush().map_err(|e| PrintFault::new("WRITE_FAILED", e))?;
        // Best effort: the bytes are in the printer's buffer once write_all returned.
        let _ = s.shutdown(Shutdown::Write);
        Ok(())
    }

    fn status(&self) -> Result<Option<PrinterStatus>, PrintFault> {
        let mut s = self.open()?;
        let offline = query(&mut s, &STATUS_OFFLINE)?;
        let paper = query(&mut s, &STATUS_PAPER)?;
        Ok(Some(PrinterStatus::decode(offline, paper)))
    }
}
