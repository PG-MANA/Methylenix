//!
//! Print module
//!

#[cfg(target_os = "uefi")]
pub mod efi_text_output;
#[cfg(target_os = "uefi")]
use efi_text_output::WRITER;

#[cfg(target_os = "none")]
pub mod serial_port;
#[cfg(target_os = "none")]
use serial_port::WRITER;

use core::fmt;

macro_rules! println {
    () => ($crate::print::print(format_args!("\n")));
    ($fmt:expr) => ($crate::print::print(format_args!("{}\n", format_args!($fmt))));
    ($fmt:expr, $($arg:tt)*) => ($crate::print::print(format_args!("{}\n", format_args!($fmt, $($arg)*))));
}

macro_rules! kprintln {
    () => ($crate::print::print(format_args!("\n")));
    ($fmt:expr) => ($crate::print::print(format_args!("{}\n", format_args!($fmt))));
    ($fmt:expr, $($arg:tt)*) => ($crate::print::print(format_args!("{}\n", format_args!($fmt, $($arg)*))));
}
macro_rules! pr_err {
    ($fmt:expr) => ($crate::print::print(format_args!("[ERROR] {}\n", format_args!($fmt))));
    ($fmt:expr, $($arg:tt)*) => ($crate::print::print(format_args!("[ERROR] {}\n", format_args!($fmt, $($arg)*))));
}

macro_rules! pr_warn {
    ($fmt:expr) => ($crate::print::print(format_args!("[WARN] {}\n", format_args!($fmt))));
    ($fmt:expr, $($arg:tt)*) => ($crate::print::print(format_args!("[WARN] {}\n", format_args!($fmt, $($arg)*))));
}

pub fn print(args: fmt::Arguments) {
    use core::fmt::Write;
    if let Some(s) = unsafe { (&raw mut WRITER).as_mut().unwrap() } {
        let _ = s.write_fmt(args);
    }
}
