//!
//! Arch-depended ACPI support
//!

use crate::arch::target_arch::device::cpu::{
    in_byte, in_dword, in_word, out_byte, out_dword, out_word,
};
use crate::arch::target_arch::interrupt::InterruptManager;

use crate::kernel::drivers::acpi::aml::{AmlError, AmlVariable, ConstData};
use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::sync::spin_lock::Mutex;

use alloc::sync::Arc;

pub fn setup_interrupt(acpi_manager: &AcpiManager) -> bool {
    let irq = acpi_manager.get_fadt_manager().get_sci_int();
    let index = InterruptManager::irq_to_index(irq as u8);
    get_cpu_manager_cluster()
        .interrupt_manager
        .set_device_interrupt_function(acpi_event_handler, Some(irq as u8), index, 0, true)
}

fn acpi_event_handler(index: usize) {
    get_kernel_manager_cluster()
        .acpi_event_manager
        .sci_handler();
    get_cpu_manager_cluster()
        .interrupt_manager
        .send_eoi_level_trigger(index as u8);
}

#[inline]
pub fn read_io_byte(port: usize) -> u8 {
    unsafe { in_byte(port as u16) }
}

#[inline]
pub fn write_io_byte(port: usize, data: u8) {
    unsafe { out_byte(port as u16, data) }
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
                    pr_err!("Cannot read 64bit data from I/O port.");
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
                    pr_err!("Cannot write 64bit data into I/O port.");
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
