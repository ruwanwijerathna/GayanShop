//! The Windows spooler with the RAW datatype (`OpenPrinterW` → `StartDocPrinterW("RAW")` → `WritePrinter`).
//! `WritePrinter` can block forever on a frozen spooler; the worker runs it on `spawn_blocking` under a
//! timeout and abandons the thread if it hangs (doc §7.5). The spooler hides the device, so no status.

use super::{PrintFault, PrinterStatus, PrinterTransport};

pub struct Spooler {
    printer_name: String,
}

impl Spooler {
    pub fn new(printer_name: &str) -> Self {
        Spooler { printer_name: printer_name.trim().to_owned() }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn write_raw(printer_name: &str, bytes: &[u8]) -> Result<(), PrintFault> {
    use windows_sys::Win32::Graphics::Printing::{
        ClosePrinter, DOC_INFO_1W, EndDocPrinter, EndPagePrinter, OpenPrinterW, PRINTER_HANDLE, StartDocPrinterW,
        StartPagePrinter, WritePrinter,
    };
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let name = wide(printer_name);
    let mut doc_name = wide("GayanShop receipt");
    let mut datatype = wide("RAW");
    let len = u32::try_from(bytes.len()).map_err(|e| PrintFault::new("WRITE_FAILED", e))?;
    let os = || std::io::Error::last_os_error();
    // SAFETY: every pointer passed points into a live, NUL-terminated buffer owned by this frame; the printer
    // handle is closed on every path after a successful open.
    unsafe {
        let mut handle: PRINTER_HANDLE = std::mem::zeroed();
        if OpenPrinterW(name.as_ptr(), &mut handle, std::ptr::null()) == 0 {
            return Err(PrintFault::new("PRINTER_NOT_FOUND", format!("{printer_name}: {}", os())));
        }
        let info = DOC_INFO_1W {
            pDocName: doc_name.as_mut_ptr(),
            pOutputFile: std::ptr::null_mut(),
            pDatatype: datatype.as_mut_ptr(),
        };
        let result = if StartDocPrinterW(handle, 1, &info) == 0 {
            Err(PrintFault::new("WRITE_FAILED", os()))
        } else {
            let mut written = 0u32;
            let r = if StartPagePrinter(handle) == 0 {
                Err(PrintFault::new("WRITE_FAILED", os()))
            } else if WritePrinter(handle, bytes.as_ptr().cast(), len, &mut written) == 0 || written != len {
                Err(PrintFault::new("WRITE_FAILED", format!("{written}/{len} bytes: {}", os())))
            } else {
                Ok(())
            };
            EndPagePrinter(handle);
            EndDocPrinter(handle);
            r
        };
        ClosePrinter(handle);
        result
    }
}

#[cfg(not(windows))]
fn write_raw(printer_name: &str, _bytes: &[u8]) -> Result<(), PrintFault> {
    Err(PrintFault::new("UNSUPPORTED", format!("the Windows spooler is not available for {printer_name}")))
}

impl PrinterTransport for Spooler {
    fn send(&self, bytes: &[u8]) -> Result<(), PrintFault> {
        write_raw(&self.printer_name, bytes)
    }

    fn status(&self) -> Result<Option<PrinterStatus>, PrintFault> {
        Ok(None)
    }
}
