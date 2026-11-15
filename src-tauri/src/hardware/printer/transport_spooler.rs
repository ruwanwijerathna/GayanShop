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

/// Every printer queue Windows knows about (local and connected), sorted, for the Settings dropdown.
pub fn list_printers() -> Result<Vec<String>, PrintFault> {
    let mut names = enumerate()?;
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    Ok(names)
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn enumerate() -> Result<Vec<String>, PrintFault> {
    use windows_sys::Win32::Graphics::Printing::{
        EnumPrintersW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_INFO_4W,
    };
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let (mut needed, mut returned) = (0u32, 0u32);
    // SAFETY: a sizing call with a null buffer; `needed` and `returned` are live locals.
    unsafe {
        EnumPrintersW(flags, std::ptr::null(), 4, std::ptr::null_mut(), 0, &mut needed, &mut returned);
    }
    if needed == 0 {
        return Ok(Vec::new());
    }
    // A `u64` buffer keeps the structs it receives 8-byte aligned.
    let words = usize::try_from(needed).map_err(|e| PrintFault::new("STATUS_FAILED", e))?.div_ceil(8);
    let mut buf = vec![0u64; words];
    // SAFETY: `buf` is at least `needed` bytes and lives past every read below; on success it holds
    // `returned` PRINTER_INFO_4W records whose name pointers point into the same buffer.
    unsafe {
        if EnumPrintersW(flags, std::ptr::null(), 4, buf.as_mut_ptr().cast(), needed, &mut needed, &mut returned) == 0 {
            return Err(PrintFault::new("STATUS_FAILED", std::io::Error::last_os_error()));
        }
        let count = usize::try_from(returned).map_err(|e| PrintFault::new("STATUS_FAILED", e))?;
        let infos = std::slice::from_raw_parts(buf.as_ptr().cast::<PRINTER_INFO_4W>(), count);
        let mut names = Vec::with_capacity(count);
        for info in infos {
            let p = info.pPrinterName;
            if p.is_null() {
                continue;
            }
            let mut len = 0usize;
            while *p.add(len) != 0 {
                len += 1;
            }
            names.push(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)));
        }
        Ok(names)
    }
}

#[cfg(not(windows))]
fn enumerate() -> Result<Vec<String>, PrintFault> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_never_fails_and_is_sorted_and_unique() {
        let names = list_printers().unwrap_or_default();
        let mut sorted = names.clone();
        sorted.sort_by_key(|n| n.to_lowercase());
        sorted.dedup();
        assert_eq!(names, sorted);
    }
}
