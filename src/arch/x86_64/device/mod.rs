//!
//! Arch-depended device driver
//!
//! This module handles low level settings.

pub mod acpi;
pub mod cpu;
pub mod crt;
pub mod io_apic;
pub mod local_apic;
pub mod local_apic_timer;
pub mod pci;
pub mod pic;
pub mod pit;
pub mod serial_port;
pub mod text;
