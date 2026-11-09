//! Serial (COM port) receipt printers. On Windows the port is opened as `\.\COMn`, set to 8-N-1 at the
//! profile's baud rate, with write (5 s) and read (2 s) timeouts so a printer with no cable fails fast.

use std::fs::{File, OpenOptions};
use std::io::Write;

use super::transport_network::query;
use super::{PrintFault, PrinterStatus, PrinterTransport};
use crate::hardware::escpos::{STATUS_OFFLINE, STATUS_PAPER};

pub struct Serial {
    port: String,
    baud: u32,
}

impl Serial {
    pub fn new(port: &str, baud: u32) -> Self {
        Serial { port: port.trim().to_owned(), baud }
    }

    fn path(&self) -> String {
        if cfg!(windows) && !self.port.starts_with(r"\.\") { format!(r"\.\{}", self.port) } else { self.port.clone() }
    }

    fn open(&self) -> Result<File, PrintFault> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.path())
            .map_err(|e| PrintFault::new("PRINTER_NOT_FOUND", format!("{}: {e}", self.port)))?;
        configure(&file, self.baud)?;
        Ok(file)
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn configure(file: &File, baud: u32) -> Result<(), PrintFault> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Devices::Communication::{
        COMMTIMEOUTS, DCB, GetCommState, NOPARITY, ONESTOPBIT, SetCommState, SetCommTimeouts,
    };
    let handle = file.as_raw_handle();
    // SAFETY: `handle` is the live handle of `file` for the duration of these calls; DCB and COMMTIMEOUTS
    // are plain C structs that the API reads/writes in place.
    unsafe {
        let mut dcb: DCB = std::mem::zeroed();
        dcb.DCBlength = u32::try_from(std::mem::size_of::<DCB>()).unwrap_or(0);
        if GetCommState(handle, &mut dcb) == 0 {
            return Err(PrintFault::new("CONNECT_FAILED", std::io::Error::last_os_error()));
        }
        dcb.BaudRate = baud;
        dcb.ByteSize = 8;
        dcb.Parity = NOPARITY;
        dcb.StopBits = ONESTOPBIT;
        if SetCommState(handle, &dcb) == 0 {
            return Err(PrintFault::new("CONNECT_FAILED", std::io::Error::last_os_error()));
        }
        let timeouts = COMMTIMEOUTS {
            ReadIntervalTimeout: 0,
            ReadTotalTimeoutMultiplier: 0,
            ReadTotalTimeoutConstant: 2_000,
            WriteTotalTimeoutMultiplier: 0,
            WriteTotalTimeoutConstant: 5_000,
        };
        if SetCommTimeouts(handle, &timeouts) == 0 {
            return Err(PrintFault::new("CONNECT_FAILED", std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn configure(_file: &File, _baud: u32) -> Result<(), PrintFault> {
    Ok(())
}

impl PrinterTransport for Serial {
    fn send(&self, bytes: &[u8]) -> Result<(), PrintFault> {
        let mut f = self.open()?;
        f.write_all(bytes).map_err(|e| PrintFault::new("WRITE_FAILED", e))?;
        f.flush().map_err(|e| PrintFault::new("WRITE_FAILED", e))
    }

    fn status(&self) -> Result<Option<PrinterStatus>, PrintFault> {
        let mut f = self.open()?;
        let offline = query(&mut f, &STATUS_OFFLINE)?;
        let paper = query(&mut f, &STATUS_PAPER)?;
        Ok(Some(PrinterStatus::decode(offline, paper)))
    }
}
