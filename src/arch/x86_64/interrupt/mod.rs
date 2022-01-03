//!
//! Interrupt Manager
//!
//! This manager controls IDT and APIC.

pub mod idt;
#[macro_use]
pub mod handler;
mod tss;

use self::idt::GateDescriptor;
use self::tss::TssManager;

use crate::arch::target_arch::context::{context_data::ContextData, ContextManager};
use crate::arch::target_arch::device::cpu;
use crate::arch::target_arch::device::local_apic::LocalApicManager;

use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{Address, MPageOrder, MSize, MemoryPermissionFlags};
use crate::kernel::sync::spin_lock::SpinLockFlag;

use crate::{alloc_non_linear_pages, alloc_pages};
use core::mem::{size_of, MaybeUninit};

pub struct StoredIrqData {
    r_flags: u64,
}

/// InterruptManager has no SpinLockFlag, When you use this, be careful of Mutex.
///
/// This has io_apic and local_apic handler inner.
/// This struct may be changed in the future.
pub struct InterruptManager {
    lock: SpinLockFlag,
    idt: MaybeUninit<&'static mut [GateDescriptor; InterruptManager::IDT_MAX as usize]>,
    main_selector: u16,
    local_apic: LocalApicManager,
    tss_manager: TssManager,
}

/// Interruption Number
///
/// This enum is used to decide which index the specific device should use.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum InterruptionIndex {
    SerialPort = 0x24,
    Nvme = 0xee,
    LocalApicTimer = 0xef,
    RescheduleIpi = 0xf8,
}

/// IST index for each interrupt.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum IstIndex {
    NormalInterrupt = 0,
    TaskSwitch = 1,
}

impl InterruptManager {
    pub const LIMIT_IDT: u16 = 0x100 * (size_of::<idt::GateDescriptor>() as u16) - 1;
    pub const IDT_MAX: u16 = 0xff;

    /// Create InterruptManager with invalid data.
    ///
    /// Before use, **you must call [`init`]**.
    ///
    /// [`init`]: #method.init
    pub const fn new() -> InterruptManager {
        InterruptManager {
            idt: MaybeUninit::uninit(),
            lock: SpinLockFlag::new(),
            main_selector: 0,
            local_apic: LocalApicManager::new(),
            tss_manager: TssManager::new(),
        }
    }

    /// Allocate memory for idt and init with invalid GateDescriptor.
    fn init_idt(&mut self) {
        self.idt.write(unsafe {
            &mut *(alloc_pages!(MPageOrder::new(0), MemoryPermissionFlags::data())
                .expect("Cannot alloc memory for interrupt manager.")
                .to_usize() as *mut [_; Self::IDT_MAX as usize])
        });
        unsafe {
            for i in 0..Self::IDT_MAX {
                self.set_gate_descriptor(i, GateDescriptor::new(Self::dummy_handler, 0, 0, 0));
            }
            self.flush();
        }
    }

    /// Allocate and setup Interrupt Stack Table.
    ///
    /// This function allocates stack and set rsp into TSS.
    fn init_ist(&mut self) {
        let stack_order = ContextManager::DEFAULT_INTERRUPT_STACK_ORDER;
        let stack = alloc_non_linear_pages!(stack_order, MemoryPermissionFlags::data())
            .expect("Cannot allocate stack for interrupts.");
        assert!(self.tss_manager.set_ist(
            IstIndex::TaskSwitch as u8,
            (stack + stack_order.to_offset()).to_usize()
        ));
    }

    /// Setup RSP(for privilege level 0~2)
    ///
    /// This function allocates stack and set rsp into TSS.
    /// If allocating the stack is failed, this function will panic.
    /// rsp must be in the range 0 ~ 2.
    #[allow(dead_code)]
    fn set_rsp(&mut self, rsp: u8, stack_size: MSize) -> bool {
        let stack = alloc_pages!(
            stack_size.to_order(None).to_page_order(),
            MemoryPermissionFlags::data()
        )
        .expect("Cannot allocate pages for rsp.");

        let _lock = self.lock.lock();
        self.tss_manager
            .set_rsp(rsp, (stack + stack_size).to_usize())
    }

    /// Init this manager.
    ///
    /// This function alloc page from memory manager and
    /// fills all of IDT converted from the allocated page with a invalid handler.
    /// After that, this also init LocalApicManager.
    pub fn init(&mut self, selector: u16) {
        let flag = Self::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        self.main_selector = selector;
        self.init_idt();
        self.tss_manager.load_current_tss();
        self.init_ist();
        self.local_apic.init();
        drop(_lock);
        Self::restore_local_irq(flag);
        return;
    }

    /// Init this manager by copying some data from given manager.
    ///
    /// This function alloc page from memory manager and
    /// fills all of IDT converted from the allocated page with a invalid handler.
    /// After that, this also init LocalApicManager.
    /// This will be used to init the application processors.
    /// GDT and TSS Descriptor must be valid.
    pub fn init_ap(&mut self, original: &Self) {
        let flag = Self::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        self.main_selector = original.main_selector;
        self.init_idt();
        self.tss_manager.load_current_tss();
        self.init_ist();
        self.local_apic
            .init_from_other_manager(original.get_local_apic_manager());
        drop(_lock);
        Self::restore_local_irq(flag);
        return;
    }

    /// Init Inter Processors Interrupt.
    ///
    /// This function makes interrupt handler for ipi.
    pub fn init_ipi(&mut self) {
        make_context_switch_interrupt_handler!(
            reschedule_handler,
            InterruptManager::reschedule_ipi_handler
        );
        self.set_device_interrupt_function(
            reschedule_handler,
            None,
            IstIndex::TaskSwitch,
            InterruptionIndex::RescheduleIpi as u16,
            0,
            false,
        );
    }

    /// Flush IDT to cpu and apply it.
    ///
    /// This function sets the address of IDT into CPU.
    /// Unless you change the address of IDT, you don't have to call it.
    unsafe fn flush(&self) {
        let idtr = idt::DescriptorTableRegister {
            limit: InterruptManager::LIMIT_IDT,
            offset: self.idt.assume_init_read() as *const _ as u64,
        };
        cpu::lidt(&idtr as *const _ as usize);
    }

    /// Set GateDescriptor into IDT.
    ///
    /// This function is used to register interrupt handler.
    /// This is inner use only.
    /// if index < Self::IDT_MAX, this function does nothing.
    unsafe fn set_gate_descriptor(&mut self, index: u16, descriptor: GateDescriptor) {
        if index < Self::IDT_MAX {
            self.idt.assume_init_read()[index as usize] = descriptor;
        }
    }

    /// Return using selector.
    pub fn get_main_selector(&self) -> u16 {
        self.main_selector
    }

    /// Register interrupt handler.
    ///
    /// This function sets the function into IDT and
    /// redirect the target interruption into this CPU (I/O APIC).
    ///
    ///  * function: the handler to call when the interruption occurs
    ///  * irq: if the target device interrupts by irq, set this argument.
    ///         if this is some(irq), this function will call [`set_redirect`].
    ///  * index: the index of IDT to connect handler
    ///  * privilege_level: the ring level to allow interrupt. If you want to allow user interrupt,
    ///                     set this to 3.
    ///
    ///  If index <= 32(means CPU internal exception) or index > 0xFF(means intel reserved area),
    ///  this function will return false.
    ///
    ///  [`set_redirect`]: ../device/io_apic/struct.IoApicManager.html#method.set_redirect
    pub fn set_device_interrupt_function(
        &mut self,
        function: unsafe extern "C" fn(),
        irq: Option<u8>,
        ist: IstIndex,
        index: u16,
        privilege_level: u8,
        is_level_trigger: bool,
    ) -> bool {
        if index <= 32 || index > 0xFF {
            /* CPU exception interrupt */
            /* intel reserved */
            return false;
        }
        let type_attr: u8 = 0xe | (privilege_level & 0x3) << 5 | 1 << 7;

        let flag = Self::save_and_disable_local_irq();
        let _lock = self.lock.lock();
        unsafe {
            self.set_gate_descriptor(
                index,
                GateDescriptor::new(function, self.main_selector, ist as u8, type_attr),
            );
        }
        if let Some(irq) = irq {
            get_kernel_manager_cluster()
                .arch_depend_data
                .io_apic_manager
                .lock()
                .unwrap()
                .set_redirect(
                    self.local_apic.get_apic_id(),
                    irq,
                    index as u8,
                    is_level_trigger,
                );
        }
        drop(_lock);
        Self::restore_local_irq(flag);
        return true;
    }

    /// Save current the interrupt status and disable interrupt
    ///
    /// This function disables interrupt and return interrupt status before disable interrupt.
    /// The return value will be used by [`restore_local_irq`].
    /// This can be nested called.
    pub fn save_and_disable_local_irq() -> StoredIrqData {
        let r_flags = unsafe { cpu::get_r_flags() };
        unsafe { cpu::disable_interrupt() };
        StoredIrqData { r_flags }
    }

    /// Restore the interrupt status before calling [`save_and_disable_local_irq`]
    ///
    /// if the interrupt was enabled before calling [`save_and_disable_local_irq`],
    /// this will enable interrupt, otherwise this will not change the interrupt status.
    pub fn restore_local_irq(original: StoredIrqData) {
        unsafe { cpu::set_r_flags(original.r_flags) };
    }

    /// Restore the interrupt status with StoredIrqData reference.
    pub unsafe fn restore_local_irq_by_reference(original: &StoredIrqData) {
        cpu::set_r_flags(original.r_flags);
    }

    /// Send end of interrupt to Local APIC.
    pub fn send_eoi(&self) {
        self.local_apic.send_eoi();
    }

    /// Send end of interrupt to Local APIC and also send to I/O APIC.
    pub fn send_eoi_level_trigger(&self, vector: u8) {
        self.local_apic.send_eoi();
        if let Ok(io) = get_kernel_manager_cluster()
            .arch_depend_data
            .io_apic_manager
            .try_lock()
        {
            io.send_eoi(vector)
        }
    }

    /// Return the reference of LocalApicManager.
    ///
    /// Currently, this manager contains LocalApicManager.
    /// If this structure is changed, this function will be deleted.
    pub fn get_local_apic_manager(&self) -> &LocalApicManager {
        &self.local_apic
    }

    /// Send Inter Processor Interrupt to reschedule.
    pub fn send_reschedule_ipi(&self, cpu_id: usize) {
        self.local_apic.send_interrupt_command(
            cpu_id as u32,
            0,
            0,
            false,
            InterruptionIndex::RescheduleIpi as _,
        );
    }

    /// Convert IRQ to Interrupt Index
    pub const fn irq_to_index(irq: u8) -> u16 {
        irq as u16 + 0x20
    }

    /// Dummy handler to init IDT
    ///
    /// This function does nothing.
    pub extern "C" fn dummy_handler() {}

    pub extern "C" fn reschedule_ipi_handler() {
        get_cpu_manager_cluster().interrupt_manager.send_eoi();
        /* Do nothing */
    }

    /// Post script for interrupt
    ///
    /// This function calls `schedule` if needed.
    pub extern "C" fn post_interrupt_handler(context_data: u64) {
        if get_cpu_manager_cluster().run_queue.should_call_schedule() {
            get_cpu_manager_cluster()
                .run_queue
                .schedule(Some(unsafe { &*(context_data as *const ContextData) }));
        }
    }
}
