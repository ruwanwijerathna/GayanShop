//! Hardware the till drives from Rust (doc §7): receipt printers and the cash drawer on their RJ11 port.
//! The scanner is read in the frontend (§7.1); label printers arrive in Phase 2.

pub mod escpos;
pub mod printer;
