/*
    Interrupt Manager
*/

pub mod idt;
#[macro_use]
pub mod handler;
//mod tss;


use arch::target_arch::device::cpu;
use self::idt::GateDescriptor;

use kernel::manager_cluster::get_kernel_manager_cluster;

use core::mem::{MaybeUninit, size_of};


pub struct InterruptManager {
    idt: MaybeUninit<&'static mut [GateDescriptor; InterruptManager::IDT_MAX as usize]>,
    main_selector: u16,
}

/*
//#[no_mangle]//関数名をそのままにするため
pub extern "C"  fn inthandlerdef_main() {
    let master =  isr_to_irq(unsafe { pic::get_isr_master() } );
    let slave = isr_to_irq(unsafe { pic::get_isr_slave() } );
    print!("Int from IRQ-{:02}",if slave != 0{ slave + 8 }else{master});
    unsafe {
        pic::pic0_eoi(master);
    }
    if master == 2{//いるかな(割り込みがないのに設定するのはどうかと思って検討中)
        pic::pic1_eoi(slave);
    }
}


fn isr_to_irq(isr : u8) -> u8 {
    if isr == 0{
        return 0;
    }
    let mut i = isr;
    let mut cnt = 0;
    loop{
        if i == 1{
            return cnt;
        }
        cnt = cnt + 1;
        i = i >> 1;
    }
}
*/

impl InterruptManager {
    pub const LIMIT_IDT: u16 = 0x100 * (size_of::<idt::GateDescriptor>() as u16) - 1;
    //0xfffという情報あり
    pub const IDT_MAX: u16 = 0xff;

    pub const fn new() -> InterruptManager {
        InterruptManager {
            idt: MaybeUninit::uninit(),
            main_selector: 0,
        }
    }

    pub fn init(&mut self, selector: u16) -> bool {
        self.main_selector = selector;
        self.idt.write(unsafe {
            &mut *(
                get_kernel_manager_cluster().memory_manager.lock().unwrap()
                    .alloc_physical_page(false, true, false)
                    .expect("Cannot alloc memory for interrupt manager.") as *mut [_; Self::IDT_MAX as usize])
        });

        unsafe {
            for i in 0..Self::IDT_MAX {
                self.set_gatedec(i, GateDescriptor::new(Self::dummy_handler, 0, 0, 0));
            }
            self.flush();
        }
        true
    }

    unsafe fn flush(&self) {
        let idtr = idt::IDTR {
            limit: InterruptManager::LIMIT_IDT,
            offset: self.idt.read() as *const _ as u64,
        };
        cpu::lidt(&idtr as *const _ as usize);
    }

    pub unsafe fn set_gatedec(&mut self, interrupt_num: u16, descr: GateDescriptor) {
        if interrupt_num < Self::IDT_MAX {
            self.idt.read()[interrupt_num as usize] = descr;
        }
    }

    pub fn get_main_selector(&self) -> u16 {
        self.main_selector
    }

    pub fn dummy_handler() {}
}
