//!
//! Modules to handle device or system like UEFI
//!

pub mod acpi;
pub mod efi;
pub mod device {
    pub mod lpc;
}
pub mod multiboot;
pub mod pci;
