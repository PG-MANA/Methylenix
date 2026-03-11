//!
//! Modules to handle device or system like UEFI
//!

pub mod acpi;
pub mod efi;
pub mod device {
    pub mod ethernet {
        pub mod i210;
    }
    pub mod lpc;
    pub mod nvme;
    #[allow(dead_code)]
    pub mod serial_port;
}
pub mod dtb;
pub mod multiboot;
pub mod pci;
