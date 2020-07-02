/*
 * Interrupt Manager
 */

pub mod idt;
#[macro_use]
pub mod handler;
//mod tss;

use self::idt::GateDescriptor;
use arch::target_arch::device::cpu;
use arch::target_arch::device::io_apic::IoApicManager;
use arch::target_arch::device::local_apic::LocalApicManager;
use arch::target_arch::device::local_apic_timer::LocalApicTimer;

use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::memory_manager::MemoryPermissionFlags;
use kernel::timer_manager::Timer;

use core::mem::{size_of, MaybeUninit};

pub struct InterruptManager {
    idt: MaybeUninit<&'static mut [GateDescriptor; InterruptManager::IDT_MAX as usize]>,
    main_selector: u16,
    io_apic: IoApicManager,
    local_apic: LocalApicManager, /* temporary */
    lvt_timer: LocalApicTimer,
}

impl InterruptManager {
    pub const LIMIT_IDT: u16 = 0x100 * (size_of::<idt::GateDescriptor>() as u16) - 1;
    pub const IDT_MAX: u16 = 0xff;

    pub const fn new() -> InterruptManager {
        InterruptManager {
            idt: MaybeUninit::uninit(),
            main_selector: 0,
            io_apic: IoApicManager::new(),
            local_apic: LocalApicManager::new(),
            lvt_timer: LocalApicTimer::new(),
        }
    }

    pub fn dummy_handler() {}

    pub fn init(&mut self, selector: u16) -> bool {
        self.idt.write(unsafe {
            &mut *(get_kernel_manager_cluster()
                .memory_manager
                .lock()
                .unwrap()
                .alloc_pages(0, MemoryPermissionFlags::data())
                .expect("Cannot alloc memory for interrupt manager.")
                as *mut [_; Self::IDT_MAX as usize])
        });
        self.main_selector = selector;

        unsafe {
            for i in 0..Self::IDT_MAX {
                self.set_gatedec(i, GateDescriptor::new(Self::dummy_handler, 0, 0, 0));
            }
            self.flush();
        }
        self.io_apic.init();
        self.local_apic.init();
        self.lvt_timer.init();
        return true;
    }

    unsafe fn flush(&self) {
        let idtr = idt::IDTR {
            limit: InterruptManager::LIMIT_IDT,
            offset: self.idt.read() as *const _ as u64,
        };
        cpu::lidt(&idtr as *const _ as usize);
    }

    unsafe fn set_gatedec(&mut self, interrupt_num: u16, descr: GateDescriptor) {
        if interrupt_num < Self::IDT_MAX {
            self.idt.read()[interrupt_num as usize] = descr;
        }
    }

    pub fn get_main_selector(&self) -> u16 {
        self.main_selector
    }

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
            self.set_gatedec(
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

    pub fn send_eoi(&self) {
        self.local_apic.send_eoi();
    }

    pub fn sync_timer<T: Timer>(&mut self, timer: &T) {
        if self
            .lvt_timer
            .enable_deadline_mode(0x30, &mut self.local_apic)
            == false
        {
            if self
                .lvt_timer
                .set_up_interrupt(&mut self.local_apic, 0x30, timer)
                == false
            {
                panic!("Cannot set up APIC Timer");
            }
            pr_info!("Using Local APIC Timer");
        } else {
            pr_info!("Using Local APIC Timer TSC Deadline Mode");
        }
        make_interrupt_hundler!(inthandler30, LocalApicTimer::inthandler30_main);
        self.set_device_interrupt_function(
            inthandler30, /*上のマクロで指定した名前*/
            None,
            0x30,
            0,
        );
    }

    pub fn start_timer(&mut self) {
        self.lvt_timer.start_interrupt(&self.local_apic);
    }
}
