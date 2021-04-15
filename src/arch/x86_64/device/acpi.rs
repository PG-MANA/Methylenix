//!
//! Arch-depended ACPI support
//!

use crate::arch::target_arch::device::cpu::{
    in_byte, in_dword, in_word, out_byte, out_dword, out_word,
};
use crate::arch::target_arch::interrupt::IstIndex;

use crate::kernel::drivers::acpi::aml::{AmlError, ConstData};
use crate::kernel::drivers::acpi::event::AcpiEventManager;
use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress,
};
use crate::kernel::task_manager::work_queue::WorkList;

pub fn setup_interrupt(acpi_manager: &AcpiManager) -> bool {
    let irq = acpi_manager.get_fadt_manager().get_sci_int();
    make_device_interrupt_handler!(handler, acpi_event_handler);
    get_cpu_manager_cluster()
        .interrupt_manager
        .set_device_interrupt_function(
            handler,
            Some(irq as u8),
            IstIndex::NormalInterrupt,
            0x20 + irq,
            0,
        );

    return true;
}

extern "C" fn acpi_event_handler() {
    if let Some(acpi_event) = get_kernel_manager_cluster()
        .acpi_event_manager
        .find_occurred_fixed_event()
    {
        let work = WorkList::new(AcpiEventManager::acpi_fixed_event_worker, acpi_event as _);
        get_cpu_manager_cluster().work_queue.add_work(work);
        if !get_kernel_manager_cluster()
            .acpi_event_manager
            .reset_fixed_event_status(acpi_event)
        {
            pr_err!("Cannot reset flag: {:?}", acpi_event);
        }
    } else {
        pr_err!("Unknown ACPI Event");
    }

    get_cpu_manager_cluster().interrupt_manager.send_eoi();
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
    let size = MSize::new(access_size);
    let virtual_address = get_kernel_manager_cluster()
        .memory_manager
        .lock()
        .unwrap()
        .io_map(
            address,
            size,
            MemoryPermissionFlags::data(),
            Some(MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS),
        )
        .or(Err(AmlError::InvalidOperation))?;
    let result = try {
        unsafe {
            match align {
                1 => {
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let mut original_data = *(virtual_address.to_usize() as *const u8);
                    original_data &= !bit_mask << bit_index;
                    original_data |= (data.to_int() << bit_index) as u8;
                    *(virtual_address.to_usize() as *mut u8) = original_data;
                }
                2 => {
                    let aligned_address = virtual_address.to_usize() & !0b1;
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let mut original_data = *(aligned_address as *const u16);
                    original_data &= !bit_mask
                        << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index);
                    original_data |= (data.to_int()
                        << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index))
                        as u16;
                    *(virtual_address.to_usize() as *mut u16) = original_data;
                }
                4 => {
                    let aligned_address = virtual_address.to_usize() & !0b11;
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let mut original_data = *(aligned_address as *const u32);
                    original_data &= !bit_mask
                        << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index);
                    original_data |= (data.to_int()
                        << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index))
                        as u32;
                    *(virtual_address.to_usize() as *mut u32) = original_data;
                }
                8 => {
                    let aligned_address = virtual_address.to_usize() & !0b111;
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let mut original_data = *(aligned_address as *const u64);
                    original_data &= !bit_mask
                        << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index);
                    original_data |= (data.to_int()
                        << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index))
                        as u64;
                    *(virtual_address.to_usize() as *mut u64) = original_data;
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
        .lock()
        .unwrap()
        .free(virtual_address)
        .or(Err(AmlError::InvalidOperation))?;
    return result;
}

pub fn read_memory(
    address: PAddress,
    bit_index: usize,
    align: usize,
    num_of_bits: usize,
) -> Result<ConstData, AmlError> {
    let size = MSize::new((bit_index + num_of_bits) >> 3);
    let virtual_address = get_kernel_manager_cluster()
        .memory_manager
        .lock()
        .unwrap()
        .io_map(
            address,
            size,
            MemoryPermissionFlags::data(),
            Some(MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS),
        )
        .or(Err(AmlError::InvalidOperation))?;
    let result = try {
        unsafe {
            match align {
                1 => {
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let data = *(virtual_address.to_usize() as *const u8);
                    ConstData::Byte((data >> bit_index) & bit_mask)
                }
                2 => {
                    let aligned_address = virtual_address.to_usize() & !0b1;
                    let mut bit_mask = 0;
                    for _ in 0..num_of_bits {
                        bit_mask <<= 1;
                        bit_mask |= 1;
                    }
                    let data = *(aligned_address as *const u16);
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
                    let data = *(aligned_address as *const u32);
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
                    let data = *(aligned_address as *const u64);
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
        .lock()
        .unwrap()
        .free(virtual_address)
        .or(Err(AmlError::InvalidOperation))?;
    pr_info!("Result: {:?}", result);
    return result;
}
