//!
//! Modules to handle device or system like UEFI
//!

pub mod acpi;
// TODO: merge them
#[cfg(target_arch = "aarch64")]
pub mod boot_information {
    pub use crate::arch::target_arch::boot_info::BootInformation;
}
#[cfg(not(target_arch = "aarch64"))]
pub mod boot_information;
pub mod efi;
pub mod device {
    pub mod ethernet {
        pub mod i210;
    }
    pub mod lpc;
    pub mod nvme;
    #[allow(dead_code)]
    pub mod serial_port;
    pub mod vga_text;
}
pub mod dtb;
pub mod multiboot;
pub mod pci;
