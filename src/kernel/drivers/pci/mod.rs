//!
//! PCI
//!

use crate::arch::target_arch::device::pci::{
    read_config_data_register, setup_arch_depend_pci_device, write_config_address_register,
    write_config_data_register,
};

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ClassCode {
    pub base: u8,
    pub sub: u8,
    pub programming_interface: u8,
    pub revision: u8,
}

#[allow(dead_code)]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum BaseAddressType {
    IO,
    Prefetchable,
    NonPrefetchable,
}

pub struct PciManager {}

impl PciManager {
    const INVALID_VENDOR_ID: u16 = 0xffff;

    pub const fn new() -> Self {
        Self {}
    }

    pub fn write_config_address_register(
        &self,
        bus: u8,
        device: u8,
        function: u8,
        register_offset: u8,
    ) {
        assert!(function < 8);
        assert_eq!(
            register_offset & 0b11,
            0,
            "Invalid register offset: {:#X}",
            register_offset
        );
        let data: u32 = (1 << 31/* Enable */)
            | ((bus as u32) << 16)
            | ((device as u32) << 11)
            | ((function as u32) << 8)
            | (register_offset as u32);
        write_config_address_register(data)
    }

    pub fn read_config_data_register(&self) -> u32 {
        read_config_data_register()
    }

    pub fn write_config_data_register(&self, data: u32) {
        write_config_data_register(data)
    }

    fn read_vendor_id(&self, bus: u8, device: u8, function: u8) -> u16 {
        self.write_config_address_register(bus, device, function, 0);
        (self.read_config_data_register() & 0xffff) as u16
    }

    fn read_header_type(&self, bus: u8, device: u8, function: u8) -> u8 {
        self.write_config_address_register(bus, device, function, 0xc);
        ((self.read_config_data_register() >> 16) & 0xff) as u8
    }

    fn read_class_code(&self, bus: u8, device: u8, function: u8) -> ClassCode {
        self.write_config_address_register(bus, device, function, 0x8);
        let class_and_revision = self.read_config_data_register();
        ClassCode {
            base: (class_and_revision >> 24) as u8,
            sub: ((class_and_revision >> 16) & 0xff) as u8,
            programming_interface: ((class_and_revision >> 8) & 0xff) as u8,
            revision: (class_and_revision & 0xff) as u8,
        }
    }

    #[allow(dead_code)]
    fn read_base_address_header_type_0(
        &self,
        bus: u8,
        device: u8,
        function: u8,
        index: u8,
    ) -> (BaseAddressType, usize) {
        assert!(index < 6);
        self.write_config_address_register(bus, device, function, 0x10 + (index << 2));
        let address = self.read_config_data_register();
        if (address & 1) == 1 {
            (BaseAddressType::IO, (address & !0b11) as usize)
        } else {
            let base_address_type = if address & (1 << 3) != 0 {
                BaseAddressType::Prefetchable
            } else {
                BaseAddressType::NonPrefetchable
            };

            if (address & (2 << 1)) != 0 {
                assert!(index < 5);
                self.write_config_address_register(
                    bus,
                    device,
                    function,
                    0x10 + ((index + 1) << 2),
                );
                let high = self.read_config_data_register();
                let full_address = ((address as u64) & !0b1111) | ((high as u64) << 32);
                (base_address_type, full_address as usize)
            } else {
                (base_address_type, (address & !0b1111) as usize)
            }
        }
    }

    fn scan_function(&self, bus: u8, device: u8, function: u8, header_type: Option<u8>) {
        let vendor_id = self.read_vendor_id(bus, device, function);
        if vendor_id == Self::INVALID_VENDOR_ID {
            return;
        }
        let header_type =
            header_type.unwrap_or_else(|| self.read_header_type(bus, device, function));
        let class_code = self.read_class_code(bus, device, function);

        setup_arch_depend_pci_device(self, bus, device, function, header_type, class_code);
    }

    fn scan_device(&self, bus: u8, device: u8) {
        let header_type = self.read_header_type(bus, device, 0);
        self.scan_function(bus, device, 0, Some(header_type));
        if (header_type & (1 << 7)) != 0 {
            for function in 1..7 {
                if self.read_vendor_id(bus, device, 0) == Self::INVALID_VENDOR_ID {
                    continue;
                }
                self.scan_function(bus, device, function, None);
            }
        }
    }

    fn scan_bus(&self, bus: u8) {
        for device in 0..32 {
            if self.read_vendor_id(bus, device, 0) == Self::INVALID_VENDOR_ID {
                continue;
            }
            self.scan_device(bus, device)
        }
    }

    pub fn scan_root_bus(&self) {
        let root_bus_header_type = self.read_header_type(0, 0, 0);
        if (root_bus_header_type & (1 << 7)) != 0 {
            for function in 0..8 {
                if self.read_vendor_id(0, 0, function) == Self::INVALID_VENDOR_ID {
                    continue;
                }
                self.scan_bus(function);
            }
        } else {
            self.scan_bus(0);
        }
    }
}
