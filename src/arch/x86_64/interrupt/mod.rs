//module
pub mod idt;
#[macro_use]
pub mod handler;
//mod tss;

//use
use arch::x86_64::device::cpu;
use core::mem;

pub struct IDTMan {
    idt: usize,
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
impl IDTMan {
    pub const LIMIT_IDT: u16 = 0x100 * (mem::size_of::<idt::GateDescriptor>() as u16) - 1; //0xfffという情報あり
    pub const IDT_MAX: u16 = 0xff;

    pub unsafe fn new(idt_memory: usize /*IDT用メモリ域(4KiB)*/, _gdt: u64) -> IDTMan {
        let idt_man = IDTMan { idt: idt_memory };

        for i in 0..IDTMan::IDT_MAX {
            idt_man.set_gatedec(
                i as usize,
                idt::GateDescriptor::new(IDTMan::dummy_handler, 0, 0, 0),
            );
        }
        idt_man.flush();
        idt_man
    }

    unsafe fn flush(&self) {
        let idtr = idt::IDTR {
            limit: IDTMan::LIMIT_IDT,
            offset: self.idt as u64,
        };
        cpu::lidt(&idtr as *const _ as usize);
    }

    pub unsafe fn set_gatedec(
        &self,
        num: usize, /*割り込み番号*/
        descr: idt::GateDescriptor,
    ) {
        *((self.idt + (num * mem::size_of::<idt::GateDescriptor>())) as *mut idt::GateDescriptor) =
            descr;
    }

    pub fn dummy_handler() {}
}
