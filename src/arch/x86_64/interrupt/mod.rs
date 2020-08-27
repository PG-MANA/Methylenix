//!
//! Interrupt Manager
//!
//! This manager controls IDT and APIC.

pub mod idt;
#[macro_use]
pub mod handler;
//mod tss;

use self::idt::GateDescriptor;
use arch::target_arch::device::cpu;
use arch::target_arch::device::io_apic::IoApicManager;
use arch::target_arch::device::local_apic::LocalApicManager;

use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::memory_manager::MemoryPermissionFlags;

use core::mem::{size_of, MaybeUninit};
use kernel::memory_manager::data_type::Address;

/// InterruptManager has no SpinLockFlag, When you use this, be careful of Mutex.
///
/// This has io_apic and local_apic handler inner.
/// This struct may be changed in the future.
pub struct InterruptManager {
    idt: MaybeUninit<&'static mut [GateDescriptor; InterruptManager::IDT_MAX as usize]>,
    main_selector: u16,
    io_apic: IoApicManager,
    local_apic: LocalApicManager, /* temporary */
}

/// Interruption Number
///
/// This enum is used to decide which index the specific device should use.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum InterruptionIndex {
    LocalApicTimer = 0xef,
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
            main_selector: 0,
            io_apic: IoApicManager::new(),
            local_apic: LocalApicManager::new(),
        }
    }

    /// Init this manager.
    ///
    /// This function alloc page from memory manager and
    /// fills all of IDT converted from the allocated page with a invalid handler.
    /// After that, this also init IoApicManager and LocalApicManager.
    ///
    /// Currently, this function always returns true.
    pub fn init(&mut self, selector: u16) -> bool {
        self.idt.write(unsafe {
            &mut *(get_kernel_manager_cluster()
                .memory_manager
                .lock()
                .unwrap()
                .alloc_pages(0.into(), MemoryPermissionFlags::data())
                .expect("Cannot alloc memory for interrupt manager.")
                .to_usize() as *mut [_; Self::IDT_MAX as usize])
        });
        self.main_selector = selector;

        unsafe {
            for i in 0..Self::IDT_MAX {
                self.set_gate_descriptor(i, GateDescriptor::new(Self::dummy_handler, 0, 0, 0));
            }
            self.flush();
        }
        self.io_apic.init();
        self.local_apic.init();
        return true;
    }

    /// Flush IDT to cpu and apply it.
    ///
    /// This function sets the address of IDT into CPU.
    /// Unless you change the address of IDT, you don't have to call it.
    unsafe fn flush(&self) {
        let idtr = idt::IDTR {
            limit: InterruptManager::LIMIT_IDT,
            offset: self.idt.read() as *const _ as u64,
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
            self.idt.read()[index as usize] = descriptor;
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
        function: unsafe fn(),
        irq: Option<u8>,
        index: u16,
        privilege_level: u8,
    ) -> bool {
        if index <= 32 || index > 0xFF {
            /* CPU exception interrupt */
            /* intel reserved */
            return false;
        }
        let type_attr: u8 = 0xe | (privilege_level & 0x3) << 5 | 1 << 7;

        unsafe {
            self.set_gate_descriptor(
                index,
                GateDescriptor::new(function, self.main_selector, 0, type_attr),
            );
        }
        if let Some(irq) = irq {
            self.io_apic
                .set_redirect(self.local_apic.get_apic_id(), irq, index as u8);
        }
        return true;
    }

    /// Send end of interruption to Local APIC.
    pub fn send_eoi(&self) {
        self.local_apic.send_eoi();
    }

    /// Return the reference of LocalApicManager.
    ///
    /// Currently, this manager contains LocalApicManager.
    /// If this structure is changed, this function will be deleted.
    pub fn get_local_apic_manager(&self) -> &LocalApicManager {
        &self.local_apic
    }

    /// Dummy handler to init IDT
    ///
    /// This function does nothing.
    pub fn dummy_handler() {}
}
