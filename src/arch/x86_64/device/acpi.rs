//!
//! Arch-depended ACPI support
//!

use crate::arch::target_arch::device::cpu::{
    in_byte, in_dword, in_word, out_byte, out_dword, out_word,
};
use crate::arch::target_arch::interrupt::{InterruptManager, IstIndex};

use crate::kernel::drivers::acpi::aml::{AmlError, ConstData};
use crate::kernel::drivers::acpi::aml::{AmlPciConfig, AmlVariable};
use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress,
};

use crate::kernel::sync::spin_lock::Mutex;

use alloc::sync::Arc;

static mut INTERRUPT_VECTOR: u8 = 0;

pub fn setup_interrupt(acpi_manager: &AcpiManager) -> bool {
    let irq = acpi_manager.get_fadt_manager().get_sci_int();
    let index = InterruptManager::irq_to_index(irq as u8);
    make_device_interrupt_handler!(handler, acpi_event_handler);
    if get_cpu_manager_cluster()
        .interrupt_manager
        .set_device_interrupt_function(
            handler,
            Some(irq as u8),
            IstIndex::NormalInterrupt,
            index,
            0,
            true,
        )
    {
        unsafe { INTERRUPT_VECTOR = index as u8 };
        true
    } else {
        false
    }
}

extern "C" fn acpi_event_handler() {
    get_kernel_manager_cluster()
        .acpi_event_manager
        .sci_handler();
    get_cpu_manager_cluster()
        .interrupt_manager
        .send_eoi_level_trigger(unsafe { INTERRUPT_VECTOR });
}

#[inline]
pub fn read_io_byte(port: usize) -> u8 {
    unsafe { in_byte(port as u16) }
}

#[inline]
pub fn write_io_byte(port: usize, data: u8) {
    unsafe { out_byte(port as u16, data) }
}

pub fn read_embedded_controller(address: u8) -> Result<u8, AmlError> {
    if let Some(ec) = get_kernel_manager_cluster()
        .acpi_device_manager
        .get_embedded_controller()
    {
        Ok(ec.read_data(address))
    } else {
        pr_err!("Embedded Controller is not available.");
        Err(AmlError::InvalidOperation)
    }
}

pub fn write_embedded_controller(address: u8, data: u8) -> Result<(), AmlError> {
    if let Some(ec) = get_kernel_manager_cluster()
        .acpi_device_manager
        .get_embedded_controller()
    {
        ec.write_data(address, data);
        Ok(())
    } else {
        pr_err!("Embedded Controller is not available.");
        Err(AmlError::InvalidOperation)
    }
}

pub fn read_io(
    port: usize,
    bit_index: usize,
    align: usize,
    num_of_bits: usize,
) -> Result<ConstData, AmlError> {
    if port > u16::MAX as _ {
        pr_err!("Invalid port number: {:#X}", port);
        Err(AmlError::InvalidOperation)
    } else {
        pr_debug!("Read SystemI/O(Port: {:#X}, Align: {})", port, align);
        unsafe {
            match align {
                1 => {
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let data = in_byte(port as _);
                    Ok(ConstData::Byte((data >> bit_index) & bit_mask))
                }
                2 => {
                    let aligned_port = port & !0b1;
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let data = in_word(aligned_port as _);
                    Ok(ConstData::Word(
                        data >> ((((port - aligned_port) << 3) + bit_index) & bit_mask),
                    ))
                }
                4 => {
                    let aligned_port = port & !0b11;
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let data = in_dword(aligned_port as _);
                    Ok(ConstData::DWord(
                        data >> ((((port - aligned_port) << 3) + bit_index) & bit_mask),
                    ))
                }
                8 => {
                    pr_err!("Cannot out qword to I/O port.");
                    Err(AmlError::InvalidOperation)
                }
                _ => {
                    pr_err!("Invalid I/O port operation.");
                    Err(AmlError::InvalidOperation)
                }
            }
        }
    }
}

pub fn write_io(
    port: usize,
    bit_index: usize,
    align: usize,
    data: ConstData,
) -> Result<(), AmlError> {
    if port > u16::MAX as _ {
        pr_err!("Invalid port number: {:#X}", port);
        Err(AmlError::InvalidOperation)
    } else {
        pr_debug!(
            "Write SystemI/O(Port: {:#X}, Align: {}) <= {:#X}",
            port,
            align,
            data.to_int()
        );
        let access_size = (match data {
            ConstData::Byte(_) => 1,
            ConstData::Word(_) => 2,
            ConstData::DWord(_) => 4,
            ConstData::QWord(_) => 8,
        })
        .max(align);

        unsafe {
            match access_size {
                1 => out_byte(port as _, (data.to_int() << bit_index) as _),
                2 => {
                    let aligned_port = port & !0b1;
                    out_word(
                        aligned_port as _,
                        (data.to_int() << (((port - aligned_port) << 3) + bit_index)) as _,
                    );
                }
                4 => {
                    let aligned_port = port & !0b11;
                    out_dword(
                        aligned_port as _,
                        (data.to_int() << (((port - aligned_port) << 3) + bit_index)) as _,
                    );
                }
                8 => {
                    pr_err!("Cannot out qword to I/O port.");
                    return Err(AmlError::InvalidOperation);
                }
                _ => {
                    pr_err!("Invalid I/O port operation.");
                    return Err(AmlError::InvalidOperation);
                }
            }
        }
        Ok(())
    }
}

pub fn read_memory(
    address: PAddress,
    bit_index: usize,
    align: usize,
    num_of_bits: usize,
) -> Result<ConstData, AmlError> {
    let size = MSize::new(((bit_index + num_of_bits) >> 3).max(1));
    let virtual_address = get_kernel_manager_cluster()
        .memory_manager
        .io_map(
            address,
            size,
            MemoryPermissionFlags::data(),
            Some(MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS),
        )
        .or_else(|e| {
            pr_err!(
                "Failed to io_map(PhysicalAddress: {:#X}, Size: {:#X}): {:?}",
                address.to_usize(),
                size.to_usize(),
                e
            );
            Err(AmlError::InvalidOperation)
        })?;
    let result = try {
        unsafe {
            match align {
                0 | 1 => {
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let data = core::ptr::read_volatile(virtual_address.to_usize() as *const u8);
                    ConstData::Byte((data >> bit_index) & bit_mask)
                }
                2 => {
                    let aligned_address = virtual_address.to_usize() & !0b1;
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let data = core::ptr::read_volatile(aligned_address as *const u16);
                    ConstData::Word(
                        data >> ((((virtual_address.to_usize() - aligned_address) << 3)
                            + bit_index)
                            & bit_mask),
                    )
                }
                4 => {
                    let aligned_address = virtual_address.to_usize() & !0b11;
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let data = core::ptr::read_volatile(aligned_address as *const u32);
                    ConstData::DWord(
                        data >> ((((virtual_address.to_usize() - aligned_address) << 3)
                            + bit_index)
                            & bit_mask),
                    )
                }
                8 => {
                    let aligned_address = virtual_address.to_usize() & !0b111;
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let data = core::ptr::read_volatile(aligned_address as *const u64);
                    ConstData::QWord(
                        data >> ((((virtual_address.to_usize() - aligned_address) << 3)
                            + bit_index)
                            & bit_mask),
                    )
                }
                _ => {
                    pr_err!("Invalid memory operation.");
                    Err(AmlError::InvalidOperation)?
                }
            }
        }
    };
    get_kernel_manager_cluster()
        .memory_manager
        .free(virtual_address)
        .or(Err(AmlError::InvalidOperation))?;
    pr_debug!(
        "Read (Address: {:#X}, BitIndex: {}, NumOfBits: {}) => {:?}",
        address.to_usize(),
        bit_index,
        num_of_bits,
        result
    );
    return result;
}

pub fn write_memory(
    address: PAddress,
    bit_index: usize,
    align: usize,
    data: ConstData,
    num_of_bits: usize,
) -> Result<(), AmlError> {
    let access_size = (match data {
        ConstData::Byte(_) => 1,
        ConstData::Word(_) => 2,
        ConstData::DWord(_) => 4,
        ConstData::QWord(_) => 8,
    })
    .max(align);
    pr_debug!(
        "Write (Address: {:#X}, BitIndex: {}, NumOfBits: {}) <= {:?}(AccessSize: {})",
        address.to_usize(),
        bit_index,
        num_of_bits,
        data,
        access_size
    );
    let size = MSize::new(access_size);
    let virtual_address = get_kernel_manager_cluster()
        .memory_manager
        .io_map(
            address,
            size,
            MemoryPermissionFlags::data(),
            Some(MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS),
        )
        .or_else(|e| {
            pr_err!(
                "Failed to io_map(PhysicalAddress: {:#X}, Size: {:#X}): {:?}",
                address.to_usize(),
                size.to_usize(),
                e
            );
            Err(AmlError::InvalidOperation)
        })?;
    let result = try {
        unsafe {
            match align {
                0 | 1 => {
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let mut original_data =
                        core::ptr::read_volatile(virtual_address.to_usize() as *const u8);
                    original_data &= !bit_mask << bit_index;
                    original_data |= ((data.to_int() & (bit_mask as usize)) << bit_index) as u8;
                    core::ptr::write_volatile(virtual_address.to_usize() as *mut u8, original_data);
                }
                2 => {
                    let aligned_address = virtual_address.to_usize() & !0b1;
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let mut original_data = core::ptr::read_volatile(aligned_address as *const u16);
                    original_data &= !bit_mask
                        << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index);
                    original_data |= ((data.to_int() & (bit_mask as usize))
                        << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index))
                        as u16;
                    core::ptr::write_volatile(aligned_address as *mut u16, original_data);
                }
                4 => {
                    let aligned_address = virtual_address.to_usize() & !0b11;
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let mut original_data = core::ptr::read_volatile(aligned_address as *const u32);
                    original_data &= !bit_mask
                        << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index);
                    original_data |= ((data.to_int() & (bit_mask as usize))
                        << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index))
                        as u32;
                    core::ptr::write_volatile(aligned_address as *mut u32, original_data);
                }
                8 => {
                    let aligned_address = virtual_address.to_usize() & !0b111;
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let mut original_data = core::ptr::read_volatile(aligned_address as *const u64);
                    original_data &= !bit_mask
                        << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index);
                    original_data |= ((data.to_int() & (bit_mask as usize))
                        << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index))
                        as u64;
                    core::ptr::write_volatile(aligned_address as *mut u64, original_data);
                }
                _ => {
                    pr_err!("Invalid memory operation.");
                    Err(AmlError::InvalidOperation)?
                }
            }
        }
    };
    get_kernel_manager_cluster()
        .memory_manager
        .free(virtual_address)
        .or(Err(AmlError::InvalidOperation))?;
    return result;
}

pub fn read_pci(
    pci_config: AmlPciConfig,
    byte_index: usize,
    bit_index: usize,
    _align: usize,
    num_of_bits: usize,
) -> Result<ConstData, AmlError> {
    if pci_config.bus > u8::MAX as u16
        || pci_config.device > u8::MAX as u16
        || pci_config.function > u8::MAX as u16
    {
        pr_err!("PCI Express(R) is not supported.");
        return Err(AmlError::UnsupportedType);
    } else if pci_config.function >= 8 {
        pr_err!("Function({}) must be less than 8.", pci_config.function);
        return Err(AmlError::InvalidOperation);
    }
    let offset = pci_config.offset + byte_index;
    let aligned_offset = offset & !0b11;
    let bit_offset_base = (offset - aligned_offset) << 3;
    let bit_offset = bit_index + bit_offset_base;

    if bit_offset >= 32 {
        pr_err!("Invalid BitOffset: ({} + {})", bit_index, bit_offset_base);
        Err(AmlError::InvalidOperation)?;
    } else if num_of_bits > 64 {
        pr_err!("Unsupported BitMask({})", num_of_bits);
        Err(AmlError::InvalidOperation)?;
    }

    let mut bit_mask = 0;
    for _ in 0..num_of_bits {
        bit_mask <<= 1;
        bit_mask |= 1;
    }
    let mut result = 0u64;
    let pci_manager = &get_kernel_manager_cluster().pci_manager;
    pci_manager.write_config_address_register(
        pci_config.bus as _,
        pci_config.device as _,
        pci_config.function as _,
        aligned_offset as _,
    );
    let first_byte_data = pci_manager.read_config_data_register();

    result |= ((first_byte_data >> bit_offset) as u64) & bit_mask;
    let mut index = 1;
    bit_mask >>= 32 - bit_offset;

    while bit_mask != 0 {
        get_kernel_manager_cluster()
            .pci_manager
            .write_config_address_register(
                pci_config.bus as _,
                pci_config.device as _,
                pci_config.function as _,
                (aligned_offset + index) as _,
            );
        result |= pci_manager.read_config_data_register() as u64 & bit_mask;
        index += 1;
        bit_mask >>= 32;
    }

    pr_debug!(
        "Read PCI: {}:{}:{} offset: {}(bit_index: {}) => {}",
        pci_config.bus,
        pci_config.device,
        pci_config.function,
        aligned_offset,
        bit_offset,
        result
    );

    if num_of_bits <= 8 {
        Ok(ConstData::Byte(result as _))
    } else if num_of_bits <= 16 {
        Ok(ConstData::Word(result as _))
    } else if num_of_bits <= 32 {
        Ok(ConstData::DWord(result as _))
    } else {
        Ok(ConstData::QWord(result as _))
    }
}

pub fn write_pci(
    pci_config: AmlPciConfig,
    byte_index: usize,
    bit_index: usize,
    _align: usize,
    num_of_bits: usize,
    data: ConstData,
) -> Result<(), AmlError> {
    if pci_config.bus > u8::MAX as u16
        || pci_config.device > u8::MAX as u16
        || pci_config.function > u8::MAX as u16
    {
        pr_err!("PCI Express(R) is not supported.");
        return Err(AmlError::UnsupportedType);
    } else if pci_config.function >= 8 {
        pr_err!("Function({}) must be less than 8.", pci_config.function);
        return Err(AmlError::InvalidOperation);
    }
    let offset = pci_config.offset + byte_index;
    let aligned_offset = offset & !0b11;
    let bit_offset_base = (offset - aligned_offset) << 3;
    let bit_offset = bit_index + bit_offset_base;

    if bit_offset >= 32 {
        pr_err!("Invalid BitOffset: ({} + {})", bit_index, bit_offset_base);
        Err(AmlError::InvalidOperation)?;
    } else if num_of_bits > 64 {
        pr_err!("Unsupported BitMask({})", num_of_bits);
        Err(AmlError::InvalidOperation)?;
    }

    pr_debug!(
        "Write PCI: {}:{}:{} offset: {}(bit_index: {}) <= {:?}",
        pci_config.bus,
        pci_config.device,
        pci_config.function,
        aligned_offset,
        bit_offset,
        data
    );

    let mut bit_mask = 0usize;
    for _ in 0..num_of_bits {
        bit_mask <<= 1;
        bit_mask |= 1;
    }
    let mut write_data = data.to_int();
    let pci_manager = &get_kernel_manager_cluster().pci_manager;
    pci_manager.write_config_address_register(
        pci_config.bus as _,
        pci_config.device as _,
        pci_config.function as _,
        aligned_offset as _,
    );

    let first_byte_data = pci_manager.read_config_data_register();
    let buffer = (first_byte_data & !(bit_mask << bit_offset) as u32)
        | ((write_data & bit_mask) << bit_offset) as u32;
    pci_manager.write_config_data_register(buffer);

    let mut index = 1;
    bit_mask >>= 32 - bit_offset;
    write_data >>= 32 - bit_offset;

    while bit_mask != 0 {
        get_kernel_manager_cluster()
            .pci_manager
            .write_config_address_register(
                pci_config.bus as _,
                pci_config.device as _,
                pci_config.function as _,
                (aligned_offset + index) as _,
            );
        let read_data = pci_manager.read_config_data_register();
        let buffer =
            (read_data & !(bit_mask as u32)) | (write_data as u32 & bit_mask as u32) as u32;
        pci_manager.write_config_data_register(buffer);
        index += 1;
        bit_mask >>= 32;
        write_data >>= 32;
    }

    Ok(())
}

pub fn osi(arg: &[Arc<Mutex<AmlVariable>>]) -> Result<AmlVariable, AmlError> {
    let locked_arg_0 = arg[0].try_lock().or(Err(AmlError::MutexError))?;
    if let AmlVariable::String(s) = &*locked_arg_0 {
        if s.starts_with("Linux") {
            Ok(AmlVariable::ConstData(ConstData::Byte(0)))
        } else if s.starts_with("Windows") {
            Ok(AmlVariable::ConstData(ConstData::Byte(1)))
        } else {
            pr_info!("_OSI: {}", s);
            Ok(AmlVariable::ConstData(ConstData::Byte(0)))
        }
    } else {
        pr_err!("Invalid arguments: {:?}", arg);
        Err(AmlError::InvalidOperation)
    }
}
