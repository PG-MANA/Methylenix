//!
//! Interrupt Manager
//!

pub mod gic;

use crate::arch::target_arch::context::context_data::ContextData;
use crate::arch::target_arch::device::cpu;
use crate::arch::target_arch::interrupt::gic::{GicManager, GicV3Group};

use crate::kernel::drivers::pci::msi::MsiInfo;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::Address;
use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;

use core::arch::global_asm;

static mut INTERRUPT_HANDLER: [usize; u8::MAX as _] = [0usize; u8::MAX as _];
static mut INTERRUPT_HANDLER_LOCK: IrqSaveSpinLockFlag = IrqSaveSpinLockFlag::new();

const INTERRUPT_FROM_IRQ: u64 = cpu::SPSR_I;
const INTERRUPT_FROM_FIQ: u64 = cpu::SPSR_F;
const INTERRUPT_FROM_SYNCHRONOUS_LOWER: u64 = 0x01;

const MSI_DEFAULT_PRIORITY: u8 = 0x30;

/// InterruptManager has no SpinLockFlag, When you use this, be careful of Mutex.
///
/// This has io_apic and local_apic handler inner.
/// This struct may be changed in the future.
pub struct InterruptManager {
    lock: IrqSaveSpinLockFlag,
}

pub struct StoredIrqData {
    daif: u64,
}

impl InterruptManager {
    const RESCHEDULE_SGI: u32 = 15;

    /// Create InterruptManager with invalid data.
    ///
    /// Before use, **you must call [`init`]**.
    ///
    /// [`init`]: #method.init
    pub const fn new() -> InterruptManager {
        InterruptManager {
            lock: IrqSaveSpinLockFlag::new(),
        }
    }

    pub fn init(&mut self) {
        extern "C" {
            fn interrupt_vector();
        }
        let _lock = self.lock.lock();
        unsafe { cpu::set_vbar(interrupt_vector as *const fn() as usize as u64) };
    }

    pub fn init_ap(&mut self) {
        extern "C" {
            fn interrupt_vector();
        }
        let _lock = self.lock.lock();
        unsafe { cpu::set_vbar(interrupt_vector as *const fn() as usize as u64) };
    }

    pub fn init_ipi(&self) {
        self.set_device_interrupt_function(
            Self::reschedule_ipi_handler,
            Self::RESCHEDULE_SGI,
            0x10,
            None,
            false,
        )
        .expect("Failed to setup IPI");
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
        priority_level: u8,
        group: Option<GicV3Group>,
        is_level_trigger: bool,
    ) -> Result<usize, ()> {
        if interrupt_id as usize >= unsafe { INTERRUPT_HANDLER.len() } {
            pr_err!("Invalid interrupt id: {:#X}", interrupt_id);
            return Err(());
        }
        let _self_lock = self.lock.lock();
        let _lock = unsafe { INTERRUPT_HANDLER_LOCK.lock() };
        let group = group.unwrap_or(GicV3Group::NonSecureEl1);
        let handler_address = unsafe { INTERRUPT_HANDLER[interrupt_id as usize] };
        if handler_address != 0 {
            if handler_address == function as *const fn(usize) as usize {
                if interrupt_id > 31 {
                    return Ok(interrupt_id as usize);
                }
            } else {
                drop(_lock);
                drop(_self_lock);
                pr_err!("Index is in use.");
                return Err(());
            }
        } else {
            unsafe {
                INTERRUPT_HANDLER[interrupt_id as usize] = function as *const fn(usize) as usize
            };
        }

        if interrupt_id < 32 {
            /* Setup SGI/PPI */
            let redistributor = &mut get_cpu_manager_cluster()
                .arch_depend_data
                .gic_redistributor_manager;
            if !redistributor.is_available() {
                drop(_lock);
                drop(_self_lock);
                pr_err!("GIC Redistributor is not enabled.");
                return Err(());
            }
            redistributor.set_priority(interrupt_id, priority_level);
            redistributor.set_group(interrupt_id, group);
            redistributor.set_trigger_mode(interrupt_id, is_level_trigger);
            redistributor.set_enable(interrupt_id, true);
        } else if interrupt_id < 1020 {
            /* Setup SPI */
            let gic_distributor = &get_kernel_manager_cluster().arch_depend_data.gic_manager;
            gic_distributor.set_priority(interrupt_id, priority_level);
            gic_distributor.set_group(interrupt_id, group);
            gic_distributor.set_routing(interrupt_id, false, unsafe { cpu::get_mpidr() });
            gic_distributor.set_trigger_mode(interrupt_id, is_level_trigger);
            gic_distributor.set_enable(interrupt_id, true);
        } else {
            unimplemented!()
        }
        drop(_lock);
        drop(_self_lock);
        return Ok(interrupt_id as usize);
    }

    pub fn setup_msi_interrupt(
        &self,
        function: fn(usize) -> bool,
        priority_level: Option<u8>,
        is_level_trigger: bool,
    ) -> Result<MsiInfo, ()> {
        /* TODO: support ITS */
        let _self_lock = self.lock.lock();
        let _lock = unsafe { INTERRUPT_HANDLER_LOCK.lock() };
        let mut interrupt_id = 0u32;
        for i in 32..unsafe { INTERRUPT_HANDLER.len() } {
            if unsafe { INTERRUPT_HANDLER[i] } == 0 {
                unsafe { INTERRUPT_HANDLER[i] = function as *const fn() as usize };
                interrupt_id = i as u32;
                break;
            }
        }
        drop(_lock);
        /* Setup SPI */
        let gic_distributor = &get_kernel_manager_cluster().arch_depend_data.gic_manager;
        gic_distributor.set_priority(interrupt_id, priority_level.unwrap_or(MSI_DEFAULT_PRIORITY));
        gic_distributor.set_group(interrupt_id, GicV3Group::NonSecureEl1);
        gic_distributor.set_routing(interrupt_id, false, unsafe { cpu::get_mpidr() });
        gic_distributor.set_trigger_mode(interrupt_id, is_level_trigger);
        gic_distributor.set_enable(interrupt_id, true);
        let (address, data) = gic_distributor.get_pending_register_address_and_data(interrupt_id);

        drop(_self_lock);
        return Ok(MsiInfo {
            message_address: address.to_usize() as u64,
            message_data: data as u64,
            interrupt_id: interrupt_id as usize,
        });
    }

    /// Save current the interrupt status and disable interrupt
    ///
    /// This function disables interrupt and return interrupt status before disable interrupt.
    /// The return value will be used by [`restore_local_irq`].
    /// This can be nested called.
    pub fn save_and_disable_local_irq() -> StoredIrqData {
        StoredIrqData {
            daif: unsafe { cpu::save_daif_and_disable_irq_fiq() },
        }
    }

    /// Restore the interrupt status before calling [`save_and_disable_local_irq`]
    ///
    /// if the interrupt was enabled before calling [`save_and_disable_local_irq`],
    /// this will enable interrupt, otherwise this will not change the interrupt status.
    pub fn restore_local_irq(original: StoredIrqData) {
        unsafe { cpu::restore_irq_fiq(original.daif) };
    }

    /// Restore the interrupt status with StoredIrqData reference.
    pub unsafe fn restore_local_irq_by_reference(original: &StoredIrqData) {
        cpu::restore_irq_fiq(original.daif)
    }

    pub fn send_reschedule_ipi(&self, cpu_id: usize) {
        /* cpu_id is mpidr */
        let affinity_0 = (cpu_id & 0xff) as u64;
        let affinity_1 = ((cpu_id >> 8) & 0xff) as u64;
        let affinity_2 = ((cpu_id >> 16) & 0xff) as u64;
        let affinity_3 = ((cpu_id >> 32) & 0xff) as u64;

        let icc_sgi1r = (affinity_3 << 48)
            | (affinity_2 << 32)
            | ((Self::RESCHEDULE_SGI as u64) << 24)
            | (affinity_1 << 16)
            | affinity_0;
        unsafe { cpu::set_icc_sgi1r_el1(icc_sgi1r) };
    }

    #[allow(dead_code)]
    fn reschedule_ipi_handler(_: usize) -> bool {
        /* Do nothing */
        return true;
    }

    fn send_eoi(&self, index: u32, group: GicV3Group) {
        let redistributor = &mut get_cpu_manager_cluster()
            .arch_depend_data
            .gic_redistributor_manager;
        if !redistributor.is_available() {
            pr_err!("Failed to send EOI: Redistributor is not available");
            return;
        }
        redistributor.send_eoi(index, group);
    }

    /// IRQ/FIQ Handler
    #[no_mangle]
    extern "C" fn interrupt_handler(context_data: *mut ContextData, from_mark: u64) {
        match from_mark {
            INTERRUPT_FROM_FIQ | INTERRUPT_FROM_IRQ => {
                Self::irq_fiq_handler(context_data, from_mark);
            }
            INTERRUPT_FROM_SYNCHRONOUS_LOWER => {
                crate::kernel::system_call::system_call_handler(unsafe { &mut *context_data });
            }
            _ => { /* Do nothing */ }
        }
        if get_cpu_manager_cluster().run_queue.should_call_schedule() {
            get_cpu_manager_cluster()
                .run_queue
                .schedule(Some(unsafe { &*(context_data as *const ContextData) }));
        }
    }

    fn irq_fiq_handler(_context_data: *mut ContextData, _from_mark: u64) {
        let redistributor = &get_cpu_manager_cluster()
            .arch_depend_data
            .gic_redistributor_manager;
        if !redistributor.is_available() {
            pr_err!("GIC Redistributor is not available");
            return;
        }
        let (index, group) = redistributor.get_acknowledge();
        if index == GicManager::INTERRUPT_ID_INVALID {
            return;
        }
        let _lock = unsafe { INTERRUPT_HANDLER_LOCK.lock() };
        let address = unsafe { INTERRUPT_HANDLER[index as usize] };
        drop(_lock);

        if address != 0 {
            if unsafe {
                (core::mem::transmute::<usize, fn(usize) -> bool>(address))(index as usize)
            } {
                get_cpu_manager_cluster()
                    .interrupt_manager
                    .send_eoi(index, group);
            } else {
                pr_err!("Failed to process interrupt.");
            }
        } else {
            pr_err!("Invalid Interrupt: {:#X}", index);
        }
    }
}

global_asm!(
    "
.section .text
.global interrupt_vector

.balign 0x800
interrupt_vector:
synchronous_current_el_stack_pointer_0:
    b   synchronous_current_el_stack_pointer_0

.balign 0x080
irq_current_el_stack_pointer_0:
    sub     sp,  sp, {c}
    stp     x0,  x1, [sp, #(16 * 0)]
    stp     x2,  x3, [sp, #(16 * 1)]
    mov     x1, {irq_mark}
    b       interrupt_entry

.balign 0x080
fiq_current_el_stack_pointer_0:
    sub     sp,  sp, {c}
    stp     x0,  x1, [sp, #(16 * 0)]
    stp     x2,  x3, [sp, #(16 * 1)]
    mov     x1, {fiq_mark}
    b       interrupt_entry

.balign 0x080
s_error_current_el_stack_pointer_0:
    b   s_error_current_el_stack_pointer_0

.balign 0x080
synchronous_current_el_stack_pointer_x:
    b   synchronous_current_el_stack_pointer_x

.balign 0x080
irq_current_el_stack_pointer_x:
    sub     sp,  sp, {c}
    stp     x0,  x1, [sp, #(16 * 0)]
    stp     x2,  x3, [sp, #(16 * 1)]
    mov     x1, {irq_mark}
    b       interrupt_entry

.balign 0x080
fiq_current_el_stack_pointer_x:
    sub     sp,  sp, {c}
    stp     x0,  x1, [sp, #(16 * 0)]
    stp     x2,  x3, [sp, #(16 * 1)]
    mov     x1, {fiq_mark}
    b       interrupt_entry

.balign 0x080
s_error_current_el_stack_pointer_x:
    b   s_error_current_el_stack_pointer_x

.balign 0x080
synchronous_lower_el_aarch64:
    sub     sp,  sp, {c}
    stp     x0,  x1, [sp, #(16 * 0)]
    stp     x2,  x3, [sp, #(16 * 1)]
    mov     x1, {synchronous_lower}
    b       interrupt_entry

.balign 0x080
irq_lower_el_aarch64:
    sub     sp,  sp, {c}
    stp     x0,  x1, [sp, #(16 * 0)]
    stp     x2,  x3, [sp, #(16 * 1)]
    mov     x1, {irq_mark}
    b       interrupt_entry

.balign 0x080
fiq_lower_el_aarch64:
    sub     sp,  sp, {c}
    stp     x0,  x1, [sp, #(16 * 0)]
    stp     x2,  x3, [sp, #(16 * 1)]
    mov     x1, {fiq_mark}
    b       interrupt_entry

.balign 0x080
s_error_lower_el_aarch64:
    b   s_error_lower_el_aarch64

.balign 0x080
synchronous_lower_el_aarch32:
    b   synchronous_lower_el_aarch32

.balign 0x080
irq_lower_el_aarch32:
    b   irq_lower_el_aarch32

.balign 0x080
fiq_lower_el_aarch32:
    b   fiq_lower_el_aarch32

.balign 0x080
s_error_lower_el_aarch32:
    b   s_error_lower_el_aarch32

// sp must be subbed {c} sizes, x0 ~ x3 must be saved
interrupt_entry:
    //sub     sp, sp, {c}
    //stp     x0, x1, [sp, #(16 * 0)]
    //stp     x2,  x3, [sp, #(16 * 1)]
    mrs     x2, elr_el1
    mrs     x3, spsr_el1
    stp     x2, x3, [sp, #(8 * 34)] 
    and     x3, x3, {m}
    cmp     x3, {el0}
    b.ne    1f
    mrs     x2, sp_el0
    mrs     x3, tpidr_el0
    stp     x2, x3, [sp, #(8 * 32)]
    b       2f
1:
    mov     x2, sp
    add     x2, x2, {c}
    str     x2,  [sp, #(8 * 32)]
2:
    stp     x4,  x5, [sp, #(16 * 2)]
    stp     x6,  x7, [sp, #(16 * 3)]
    stp     x8,  x9, [sp, #(16 * 4)]
    stp    x10, x11, [sp, #(16 * 5)]
    stp    x12, x13, [sp, #(16 * 6)]
    stp    x14, x15, [sp, #(16 * 7)]
    stp    x16, x17, [sp, #(16 * 8)]
    stp    x18, x19, [sp, #(16 * 9)]
    stp    x20, x21, [sp, #(16 * 10)]
    stp    x22, x23, [sp, #(16 * 11)]
    stp    x24, x25, [sp, #(16 * 12)]
    stp    x26, x27, [sp, #(16 * 13)]
    stp    x28, x29, [sp, #(16 * 14)]
    str    x30,      [sp, #(16 * 15)]
    mov    x29,  sp
    mov     x0, x29
    bl      interrupt_handler
    mov     sp, x29
    ldp     x2, x3,  [sp, #(16 * 1)]
    ldp     x4,  x5, [sp, #(16 * 2)]
    ldp     x6,  x7, [sp, #(16 * 3)]
    ldp     x8,  x9, [sp, #(16 * 4)]
    ldp    x10, x11, [sp, #(16 * 5)]
    ldp    x12, x13, [sp, #(16 * 6)]
    ldp    x14, x15, [sp, #(16 * 7)]
    ldp    x16, x17, [sp, #(16 * 8)]
    ldp    x18, x19, [sp, #(16 * 9)]
    ldp    x20, x21, [sp, #(16 * 10)]
    ldp    x22, x23, [sp, #(16 * 11)]
    ldp    x24, x25, [sp, #(16 * 12)]
    ldp    x26, x27, [sp, #(16 * 13)]
    ldp    x28, x29, [sp, #(16 * 14)]
    ldr    x30,      [sp, #(16 * 15)]
    ldp     x0,  x1, [sp, #(8 * 34)]
    msr     elr_el1,  x0
    msr     spsr_el1, x1
    and     x1, x1, {m}
    cmp     x1, {el0}
    b.ne    3f
    ldp     x0, x1, [sp, #(8 * 32)]
    msr     sp_el0, x0
    msr     tpidr_el0, x1
    ldp     x0, x1,  [sp, #(16 * 0)]
    add     sp, sp, {c}
    eret
3:
    ldp     x0, x1,  [sp, #(16 * 0)]
    // May be wrong...
    //ldr     sp, [sp, #(8 * 32)]
    add     sp, sp, {c}
    eret
",
    c = const core::mem::size_of::<ContextData>(),
    m = const cpu::SPSR_M,
    el0 = const cpu::SPSR_M_EL0T,
    irq_mark = const INTERRUPT_FROM_IRQ,
    fiq_mark = const INTERRUPT_FROM_FIQ,
    synchronous_lower = const INTERRUPT_FROM_SYNCHRONOUS_LOWER,
);
