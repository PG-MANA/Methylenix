//!
//! Arch-depended ACPI support
//!

use crate::kernel::drivers::acpi::aml::{AmlError, AmlVariable, ConstData};
use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::sync::spin_lock::Mutex;

use alloc::sync::Arc;

pub fn setup_interrupt(_acpi_manager: &AcpiManager) -> bool {
    true
    //let irq = acpi_manager.get_fadt_manager().get_sci_int();
}

#[allow(dead_code)]
fn acpi_event_handler(_: usize) -> bool {
    get_kernel_manager_cluster()
        .acpi_event_manager
        .sci_handler();
    true
}

#[inline]
pub fn read_io_byte(_port: usize) -> u8 {
    unreachable!()
}

#[inline]
pub fn write_io_byte(_port: usize, _data: u8) {
    unreachable!()
}

#[inline]
pub fn read_io_word(_port: usize) -> u16 {
    unreachable!()
}

#[inline]
pub fn write_io_word(_port: usize, _data: u16) {
    unreachable!()
}

#[inline]
pub fn read_io_dword(_port: usize) -> u32 {
    unreachable!()
}

pub fn read_io(
    _port: usize,
    _bit_index: usize,
    _align: usize,
    _num_of_bits: usize,
) -> Result<ConstData, AmlError> {
    Err(AmlError::InvalidOperation)
}

pub fn write_io(
    _port: usize,
    _bit_index: usize,
    _align: usize,
    _data: ConstData,
) -> Result<(), AmlError> {
    Err(AmlError::InvalidOperation)
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
