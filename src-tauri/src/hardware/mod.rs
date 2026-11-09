//! Hardware the till drives from Rust (doc §7): receipt printers and the cash drawer on their RJ11 port.
//! The scanner is read in the frontend (§7.1); barcode label printers use `label` (§7.4).

pub mod escpos;
pub mod label;
pub mod printer;
