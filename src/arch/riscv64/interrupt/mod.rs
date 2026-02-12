//!
//! Interrupt Manager
//!

pub mod plicv1;

use crate::arch::target_arch::context::context_data::ContextData;
use crate::arch::target_arch::device::cpu;
use crate::arch::target_arch::initialization::set_cpu_manager_cluster_gp;

use crate::kernel::drivers::dtb::{DtbManager, DtbNodeInfo};
use crate::kernel::drivers::pci::msi::MsiInfo;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{Address, PAddress};
use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;

use core::arch::global_asm;

struct InterruptInformation {
    lock: IrqSaveSpinLockFlag,
    handlers: [usize; 1024],
}
static mut INTERRUPT_INFO: InterruptInformation = InterruptInformation {
    lock: IrqSaveSpinLockFlag::new(),
    handlers: [0usize; 1024],
};

fn get_interrupt_info_mut() -> &'static mut InterruptInformation {
    unsafe { (&raw mut INTERRUPT_INFO).as_mut().unwrap() }
}

trait InterruptController {
    fn set_priority(&self, interrupt_id: u32, priority: u32);
    fn set_pending(&self, interrupt_id: u32, pending: bool);
    fn set_enable(&self, interrupt_id: u32, context: u32, enable: bool);
    fn set_priority_threshold(&self, context: u32, threshold: u32);
    fn claim_interrupt(&self, context: u32) -> u32;
    fn send_eoi(&self, context: u32, interrupt_id: u32);
    fn get_pending_register_address_and_data(&self, interrupt_id: u32) -> (PAddress, u8);
}

/// InterruptManager has no SpinLockFlag, When you use this, be careful of Mutex.
///
/// This struct may be changed in the future.
pub struct InterruptManager {
    lock: IrqSaveSpinLockFlag,
}

pub struct StoredIrqData {
    sie: u64,
}

impl InterruptManager {
    /// Create InterruptManager with invalid data.
    ///
    /// Before use, **you must call [`Self::init`]**.
    pub const fn new() -> InterruptManager {
        InterruptManager {
            lock: IrqSaveSpinLockFlag::new(),
        }
    }

    pub fn init(&mut self) {
        unsafe extern "C" {
            fn interrupt_vector();
        }
        unsafe { cpu::set_stvec(interrupt_vector as *const fn() as usize as u64) };
    }

    pub fn init_ap(&mut self) {
        unsafe extern "C" {
            fn interrupt_vector();
        }
        unsafe { cpu::set_stvec(interrupt_vector as *const fn() as usize as u64) };
    }

    pub fn init_ipi(&self) {
        // Using SBI IPI
        // TODO: implement Local Interrupt Controller
    }

    /// Register interrupt handler.
    ///
    /// This function sets the function into INTERRUPT_HANDLERS and
    /// setup GIC redistributor.
    ///
    pub fn set_device_interrupt_function(
        &self,
        function: fn(usize) -> bool,
        interrupt_id: u32,
        priority: u32,
        _is_level_trigger: bool,
    ) -> Result<usize, ()> {
        let interrupt_info = get_interrupt_info_mut();
        if interrupt_id as usize >= interrupt_info.handlers.len() {
            pr_err!("Invalid interrupt id: {:#X}", interrupt_id);
            return Err(());
        }
        let _self_lock = self.lock.lock();
        let _lock = interrupt_info.lock.lock();
        let handler_address = interrupt_info.handlers[interrupt_id as usize];
        if handler_address != 0 {
            if handler_address != function as *const fn(usize) as usize {
                drop(_lock);
                drop(_self_lock);
                pr_err!("Index is in use.");
                return Err(());
            }
        } else {
            interrupt_info.handlers[interrupt_id as usize] = function as *const fn(usize) as usize;
            cpu::synchronize(&interrupt_info.handlers[interrupt_id as usize]);
        }

        // TODO: detect dynamically
        let ic = &mut get_kernel_manager_cluster().arch_depend_data.plic;
        ic.set_priority(interrupt_id, priority);
        ic.set_enable(interrupt_id, cpu::get_hartid() as u32, true);
        drop(_lock);
        drop(_self_lock);
        Ok(interrupt_id as usize)
    }

    pub fn setup_msi_interrupt(
        &self,
        function: fn(usize) -> bool,
        priority: Option<u8>,
        _is_level_trigger: bool,
    ) -> Result<MsiInfo, ()> {
        let interrupt_info = get_interrupt_info_mut();
        let priority = priority.unwrap_or(0) as u32;
        let _self_lock = self.lock.lock();
        let _lock = interrupt_info.lock.lock();
        let mut interrupt_id = 0u32;
        for (i, e) in interrupt_info.handlers.iter_mut().enumerate() {
            if *e == 0 {
                *e = function as *const fn() as usize;
                cpu::synchronize(e);
                interrupt_id = i as u32;
                break;
            }
        }
        drop(_lock);
        // TODO: detect dynamically
        let ic = &mut get_kernel_manager_cluster().arch_depend_data.plic;
        ic.set_priority(interrupt_id, priority);
        ic.set_enable(interrupt_id, cpu::get_hartid() as u32, true);
        let (address, data) = ic.get_pending_register_address_and_data(interrupt_id);

        drop(_self_lock);
        Ok(MsiInfo {
            message_address: address.to_usize() as u64,
            message_data: data as u64,
            interrupt_id: interrupt_id as usize,
        })
    }

    pub fn read_interrupt_info_from_dtb(
        dtb_manager: &DtbManager,
        info: &DtbNodeInfo,
        index: usize,
    ) -> Option<(
        u32,  /* interrupt_id */
        bool, /* is_level_trigger */
    )> {
        dtb_manager
            .get_property(info, &DtbManager::PROP_INTERRUPTS)
            .and_then(|i| {
                dtb_manager
                    .read_property_as_u32(&i, index)
                    .map(|i| (i, true))
            })
    }

    /// Save current the interrupt status and disable interrupt
    ///
    /// This function disables interrupt and returns interrupt status before disabling interrupt.
    /// The return value will be used by [`restore_local_irq`].
    /// This can be nested called.
    pub fn save_and_disable_local_irq() -> StoredIrqData {
        StoredIrqData {
            sie: unsafe { cpu::save_sie_and_disable_interrupt() },
        }
    }

    /// Restore the interrupt status before calling [`save_and_disable_local_irq`]
    ///
    /// If the interrupt was enabled before calling [`save_and_disable_local_irq`],
    ///  this will enable interrupt, otherwise this will not change the interrupt status.
    pub fn restore_local_irq(original: StoredIrqData) {
        unsafe { Self::restore_local_irq_by_reference(&original) };
    }

    /// Restore the interrupt status with StoredIrqData reference.
    pub unsafe fn restore_local_irq_by_reference(original: &StoredIrqData) {
        unsafe { cpu::restore_sie(original.sie) }
    }

    pub fn send_reschedule_ipi(&self, cpu_id: usize) {
        /* cpu_id is hartid */
        // Using SBI IPI
        // TODO: implement Local Interrupt Controller
        let _lock = self.lock.lock();
        let mask = 1 << cpu_id;
        unsafe {
            cpu::sbi_call(
                mask,
                0,
                0,
                0,
                0,
                0,
                cpu::SBI_FID_SEND_IPI,
                cpu::SBI_EID_SEND_IPI,
            )
        };
    }

    fn reschedule_ipi_handler() -> bool {
        get_cpu_manager_cluster().run_queue.should_call_schedule()
    }

    fn send_eoi(&self, interrupt_id: u32) {
        get_kernel_manager_cluster()
            .arch_depend_data
            .plic
            .send_eoi(cpu::get_hartid() as u32, interrupt_id);
    }

    /// Interrupt Handler from the userland
    extern "C" fn interrupt_handler(context_data: *mut ContextData, _from_user: bool) {
        let scause = cpu::get_scause();
        let is_interrupt = (scause & cpu::SCAUSE_INTERRUPT) == cpu::SCAUSE_INTERRUPT;
        let reason = scause & !cpu::SCAUSE_INTERRUPT;

        pr_debug!(
            "SCAUSE: {scause:#X}(I: {is_interrupt}, Reason: {reason:#X}), FromUser: {_from_user}"
        );
        if is_interrupt {
            match reason {
                cpu::SCAUSE_SUPERVISOR_SOFTWARE_INTERRUPT => {
                    if !Self::reschedule_ipi_handler() {
                        pr_err!("Unknown IPI");
                    }
                }
                cpu::SCAUSE_SUPERVISOR_TIMER_INTERRUPT => {}
                cpu::SCAUSE_SUPERVISOR_EXTERNAL_INTERRUPT => {
                    // TODO: detect dynamically
                    let ic = &mut get_kernel_manager_cluster().arch_depend_data.plic;
                    let int_id = ic.claim_interrupt(cpu::get_hartid() as _);
                    pr_debug!("Int ID: {int_id}");
                }
                _ => {
                    pr_err!("Unknown Exception: {reason}");
                }
            }
        } else {
            match reason {
                cpu::SCAUSE_ENVIRONMENT_CALL_U_MODE => {
                    crate::kernel::system_call::system_call_handler(unsafe { &mut *context_data });
                }
                _ => {
                    unimplemented!("Unimplemented Exception: {reason}");
                }
            }
        }

        if get_cpu_manager_cluster().run_queue.should_call_schedule() {
            get_cpu_manager_cluster()
                .run_queue
                .schedule(Some(unsafe { &*(context_data as *const ContextData) }));
        }
    }
}

global_asm!(
    "
.section    .text
.global     interrupt_vector
.type       interrupt_vector, %function

interrupt_vector:
    // Save sp to sscratch and load the kernel stack.
    // When sscratch is zero, it indicates the interrupt from kernel.
    csrrw   sp, sscratch, sp
    beqz    sp, 1f
    // From user
    addi    sp, sp, -({c})
    sd      x1, (8 * 0)(sp)
    // x2 is the stack pointer, save later
    sd      x3, (8 * 2)(sp)
    sd      x4, (8 * 3)(sp)
    sd      x5, (8 * 4)(sp)
    sd      x6, (8 * 5)(sp)
    sd      x7, (8 * 6)(sp)
    sd      x8, (8 * 7)(sp)
    sd      x9, (8 * 8)(sp)
    sd      x10, (8 * 9)(sp)
    sd      x11, (8 * 10)(sp)
    sd      x12, (8 * 11)(sp)
    sd      x13, (8 * 12)(sp)
    sd      x14, (8 * 13)(sp)
    sd      x15, (8 * 14)(sp)
    sd      x16, (8 * 15)(sp)
    sd      x17, (8 * 16)(sp)
    sd      x18, (8 * 17)(sp)
    sd      x19, (8 * 18)(sp)
    sd      x20, (8 * 19)(sp)
    sd      x21, (8 * 20)(sp)
    sd      x22, (8 * 21)(sp)
    sd      x23, (8 * 22)(sp)
    sd      x24, (8 * 23)(sp)
    sd      x25, (8 * 24)(sp)
    sd      x26, (8 * 25)(sp)
    sd      x27, (8 * 26)(sp)
    sd      x28, (8 * 27)(sp)
    sd      x29, (8 * 28)(sp)
    sd      x30, (8 * 29)(sp)
    sd      x31, (8 * 30)(sp)
    // Store the status
    csrr    t0, sstatus
    csrr    t1, sepc
    sd      t0, (8 * 31)(sp)
    sd      t1, (8 * 32)(sp)
    // Store original sscratch
    addi    t0, sp, {c}
    sd      t0, (8 * 33)(sp)
    // Store the user stack and Clear sscratch to indicate the kernel mode
    csrrw   t0, sscratch, x0
    sd      t0, (8 * 1)(sp)
    // Load Kernel Stack and Global Pointer
    call     {set_cpu_manager_cluster_gp}

    // Call the handler
    mv      a0, sp
    li      a1, 1
    call     {interrupt_handler}

    // Restore interrupt status
    ld      t0, (8 * 31)(sp)
    ld      t1, (8 * 32)(sp)
    csrw    sstatus, t0
    csrw    sepc, t1
    // Restore the user stack to sscratch
    ld      t0, (8 * 1)(sp)
    csrw    sscratch, t0
    // Restore general registers
    ld      x1, (8 * 0)(sp)
    ld      x3, (8 * 2)(sp)
    ld      x4, (8 * 3)(sp)
    ld      x5, (8 * 4)(sp)
    ld      x6, (8 * 5)(sp)
    ld      x7, (8 * 6)(sp)
    ld      x8, (8 * 7)(sp)
    ld      x9, (8 * 8)(sp)
    ld      x10, (8 * 9)(sp)
    ld      x11, (8 * 10)(sp)
    ld      x12, (8 * 11)(sp)
    ld      x13, (8 * 12)(sp)
    ld      x14, (8 * 13)(sp)
    ld      x15, (8 * 14)(sp)
    ld      x16, (8 * 15)(sp)
    ld      x17, (8 * 16)(sp)
    ld      x18, (8 * 17)(sp)
    ld      x19, (8 * 18)(sp)
    ld      x20, (8 * 19)(sp)
    ld      x21, (8 * 20)(sp)
    ld      x22, (8 * 21)(sp)
    ld      x23, (8 * 22)(sp)
    ld      x24, (8 * 23)(sp)
    ld      x25, (8 * 24)(sp)
    ld      x26, (8 * 25)(sp)
    ld      x27, (8 * 26)(sp)
    ld      x28, (8 * 27)(sp)
    ld      x29, (8 * 28)(sp)
    ld      x30, (8 * 29)(sp)
    ld      x31, (8 * 30)(sp)
    // Restore kernel stack to sp (which may be changed, it will be used at the next interrupt)
    ld      sp, (8 * 33)(sp)
    // Restore user sp and kernel stack.
    csrrw   sp, sscratch, sp
    sret
1:
    // From kernel
    // Restore sp and continue
    csrrw   sp, sscratch, sp
    addi    sp, sp, -({c})
    sd      x1, (8 * 0)(sp)
    // x2 is the stack pointer, save later
    sd      x3, (8 * 2)(sp)
    sd      x4, (8 * 3)(sp)
    sd      x5, (8 * 4)(sp)
    sd      x6, (8 * 5)(sp)
    sd      x7, (8 * 6)(sp)
    sd      x8, (8 * 7)(sp)
    sd      x9, (8 * 8)(sp)
    sd      x10, (8 * 9)(sp)
    sd      x11, (8 * 10)(sp)
    sd      x12, (8 * 11)(sp)
    sd      x13, (8 * 12)(sp)
    sd      x14, (8 * 13)(sp)
    sd      x15, (8 * 14)(sp)
    sd      x16, (8 * 15)(sp)
    sd      x17, (8 * 16)(sp)
    sd      x18, (8 * 17)(sp)
    sd      x19, (8 * 18)(sp)
    sd      x20, (8 * 19)(sp)
    sd      x21, (8 * 20)(sp)
    sd      x22, (8 * 21)(sp)
    sd      x23, (8 * 22)(sp)
    sd      x24, (8 * 23)(sp)
    sd      x25, (8 * 24)(sp)
    sd      x26, (8 * 25)(sp)
    sd      x27, (8 * 26)(sp)
    sd      x28, (8 * 27)(sp)
    sd      x29, (8 * 28)(sp)
    sd      x30, (8 * 29)(sp)
    sd      x31, (8 * 30)(sp)
    // Store interrupt status
    csrr    t0, sstatus
    csrr    t1, sepc
    sd      t0, (8 * 31)(sp)
    sd      t1, (8 * 32)(sp)
    // Store sscratch (= zero)
    sd      x0, (8 * 33)(sp)
    // Store the original stack and Clear sscratch to indicate the kernel mode
    addi    t0, sp, {c}
    sd      t0, (8 * 1)(sp)

    // Call the handler
    mv      a0, sp
    li      a1, 0
    call    {interrupt_handler}

    // Restore interrupt status
    ld      t0, (8 * 31)(sp)
    ld      t1, (8 * 32)(sp)
    csrw    sstatus, t0
    csrw    sepc, t1
    // Restore general registers
    ld      x1, (8 * 0)(sp)
    ld      x3, (8 * 2)(sp)
    ld      x4, (8 * 3)(sp)
    ld      x5, (8 * 4)(sp)
    ld      x6, (8 * 5)(sp)
    ld      x7, (8 * 6)(sp)
    ld      x8, (8 * 7)(sp)
    ld      x9, (8 * 8)(sp)
    ld      x10, (8 * 9)(sp)
    ld      x11, (8 * 10)(sp)
    ld      x12, (8 * 11)(sp)
    ld      x13, (8 * 12)(sp)
    ld      x14, (8 * 13)(sp)
    ld      x15, (8 * 14)(sp)
    ld      x16, (8 * 15)(sp)
    ld      x17, (8 * 16)(sp)
    ld      x18, (8 * 17)(sp)
    ld      x19, (8 * 18)(sp)
    ld      x20, (8 * 19)(sp)
    ld      x21, (8 * 20)(sp)
    ld      x22, (8 * 21)(sp)
    ld      x23, (8 * 22)(sp)
    ld      x24, (8 * 23)(sp)
    ld      x25, (8 * 24)(sp)
    ld      x26, (8 * 25)(sp)
    ld      x27, (8 * 26)(sp)
    ld      x28, (8 * 27)(sp)
    ld      x29, (8 * 28)(sp)
    ld      x30, (8 * 29)(sp)
    ld      x31, (8 * 30)(sp)
    ld      x1, (8 * 1)(sp)
    sret
.size       interrupt_vector, . - interrupt_vector
",
    c = const size_of::<ContextData>(),
    set_cpu_manager_cluster_gp = sym set_cpu_manager_cluster_gp,
    interrupt_handler = sym InterruptManager::interrupt_handler,
);
