//!
//! Supervisor Binary Interface version3.0
//!

use super::cpu::{ecall, get_time};

use crate::kernel::manager_cluster::get_cpu_manager_cluster;
use crate::kernel::timer_manager::{CountTimer, GlobalTimerManager, IntervalTimer};

const EID_BASE_EXTENSION: u64 = 0x10;

#[expect(dead_code)]
#[repr(usize)]
enum BaseExtension {
    GetSbiSpecificationVersion,
    GetSbiImplementationId,
    GetSbiImplementationVersion,
    ProbeSbiExtension,
    GetMachineVendorId,
    GetMachineArchitectureId,
    GetMachineImplementationId,
}

const EID_S_MODE_IPI: u64 = 0x735049;

const EID_TIME: u64 = 0x54494D45;

pub struct SbiTimer {
    frequency: u64,
}

const EID_HSM: u64 = 0x48534D;

#[expect(dead_code)]
#[repr(usize)]
enum Hsm {
    HartStart,
    HartStop,
    GetStatus,
    HartSuspend,
}

#[allow(dead_code)]
#[repr(isize)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SbiError {
    Success = 0,
    ErrFailed = -1,
    ErrNotSupported = -2,
    ErrInvalidParam = -3,
    ErrDenied = -4,
    ErrInvalidAddress = -5,
    ErrAlreadyAvailable = -6,
    ErrAlreadyStarted = -7,
    ErrAlreadyStopped = -8,
    ErrNoShmem = -9,
    ErrInvalidState = -10,
    ErrBadRange = -11,
    ErrTimeout = -12,
    ErrIo = -13,
    ErrDeniedLocked = -14,
}

impl SbiError {
    pub fn to_result(&self) -> Result<(), SbiError> {
        if *self == SbiError::Success {
            Ok(())
        } else {
            Err(*self)
        }
    }
}

impl TryFrom<u64> for SbiError {
    type Error = ();

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        let value = value.cast_signed() as isize;
        if value <= 0 && value >= Self::ErrDeniedLocked as isize {
            Ok(unsafe { core::mem::transmute::<isize, SbiError>(value) })
        } else {
            Err(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn sbi_call(
    arg0: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    function_id: u64,
    extension_id: u64,
) -> Result<u64, SbiError> {
    let (a0, a1) = unsafe {
        ecall(
            arg0,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            function_id,
            extension_id,
        )
    };
    SbiError::try_from(a0)
        .unwrap_or(SbiError::ErrNotSupported)
        .to_result()?;
    Ok(a1)
}

pub fn get_sbi_version() -> Result<(u8, u32), SbiError> {
    sbi_call(
        0,
        0,
        0,
        0,
        0,
        0,
        BaseExtension::GetSbiSpecificationVersion as _,
        EID_BASE_EXTENSION,
    )
    .map(|v| ((v >> 24) as u8, (v & 0xFFFFFF) as u32))
}

pub fn probe_sbi_extension(eid: u64) -> Result<(), SbiError> {
    sbi_call(
        eid,
        0,
        0,
        0,
        0,
        0,
        BaseExtension::ProbeSbiExtension as _,
        EID_BASE_EXTENSION,
    )
    .and_then(|r| {
        if r != 0 {
            Ok(())
        } else {
            Err(SbiError::ErrNotSupported)
        }
    })
}

pub fn dump_sbi_extension() {
    let Ok((major, minor)) = get_sbi_version() else {
        pr_info!("SBI is not supported");
        return;
    };
    pr_info!("SBI version: {major}.{minor}");
    pr_info!("Timer Extension: {}", probe_sbi_extension(EID_TIME).is_ok());
    pr_info!(
        "IPI Extension: {}",
        probe_sbi_extension(EID_S_MODE_IPI).is_ok()
    );
    pr_info!(
        "Hart State Management Extension: {}",
        probe_sbi_extension(EID_HSM).is_ok()
    );
}

pub fn send_ipi(hartid: usize) -> Result<(), SbiError> {
    probe_sbi_extension(EID_S_MODE_IPI)?;
    sbi_call(1, hartid as _, 0, 0, 0, 0, 0, EID_S_MODE_IPI).map(|_| ())
}

impl SbiTimer {
    pub fn new(frequency: u64) -> Result<Self, SbiError> {
        probe_sbi_extension(EID_TIME)?;
        get_cpu_manager_cluster()
            .interrupt_manager
            .set_timer_interrupt_function(SbiTimer::interrupt_handler)
            .or(Err(SbiError::ErrNotSupported))?;
        Ok(SbiTimer { frequency })
    }

    fn interrupt_handler() {
        Self::common_handler();
        get_cpu_manager_cluster()
            .arch_depend_data
            .timer
            .reload_timer();
    }
}

impl CountTimer for SbiTimer {
    fn get_count(&self) -> usize {
        get_time() as usize
    }

    fn get_frequency_hz(&self) -> usize {
        self.frequency as _
    }

    fn is_count_up_timer(&self) -> bool {
        true
    }

    fn get_difference(&self, earlier: usize, later: usize) -> usize {
        if earlier <= later {
            earlier + (self.get_max_counter_value() - later)
        } else {
            earlier - later
        }
    }

    fn get_ending_count_value(&self, _start: usize, _difference: usize) -> usize {
        todo!()
    }

    fn get_max_counter_value(&self) -> usize {
        usize::MAX
    }
}

impl IntervalTimer for SbiTimer {
    fn start_interrupt(&mut self) -> bool {
        self.reload_timer();
        true
    }

    fn stop_interrupt(&mut self) -> bool {
        sbi_call(usize::MAX as _, 0, 0, 0, 0, 0, 0, EID_TIME).is_ok()
    }

    fn reload_timer(&mut self) {
        let interval = (GlobalTimerManager::TIMER_INTERVAL_MS * self.frequency) / 1000;
        let new = self.get_count().overflowing_add(interval as _).0;
        pr_debug!("Reload: {} + {interval} => {new}", self.get_count());
        if let Err(e) = sbi_call(new as _, 0, 0, 0, 0, 0, 0, EID_TIME) {
            pr_warn!("Failed to reload timer: {e:?}");
        }
    }
}

pub fn hart_start(hartid: usize, start_address: usize, opaque: usize) -> Result<(), SbiError> {
    probe_sbi_extension(EID_HSM)?;
    sbi_call(
        hartid as _,
        start_address as _,
        opaque as _,
        0,
        0,
        0,
        Hsm::HartStart as _,
        EID_HSM,
    )
    .map(|_| ())
}
