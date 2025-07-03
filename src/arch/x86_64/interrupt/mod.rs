//!
//! Interrupt Manager
//!
//! This manager controls IDT and APIC.

pub mod idt;
mod tss;

use self::idt::GateDescriptor;
use self::tss::TssManager;

use crate::arch::target_arch::context::{ContextManager, context_data::ContextData};
use crate::arch::target_arch::device::cpu;
use crate::arch::target_arch::device::local_apic::LocalApicManager;

use crate::kernel::drivers::pci::msi::MsiInfo;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{Address, MSize};
use crate::kernel::memory_manager::{alloc_non_linear_pages, alloc_pages};
use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;

use core::arch::global_asm;

/// IRQ Start from this value
const IDT_DEVICE_MIN: usize = 0x20;
const NUM_OF_IRQ: usize = 0x10;
const IDT_AVAILABLE_MIN: usize = IDT_DEVICE_MIN + NUM_OF_IRQ;
const IDT_MAX: usize = 0xff;

const MSR_EFER: u32 = 0xC0000080;
const MSR_EFER_SYSCALL_ENABLE: u64 = 0x01;

const MSR_STAR: u32 = 0xc0000081;
const MSR_LSTAR: u32 = 0xc0000082;

pub struct StoredIrqData {
    r_flags: u64,
}

static mut INTERRUPT_HANDLER: [usize; IDT_MAX - IDT_DEVICE_MIN + 1] =
    [0usize; IDT_MAX - IDT_DEVICE_MIN + 1];
static mut IRQ_IS_LEVEL_TRIGGER: [u8; NUM_OF_IRQ / u8::BITS as usize] =
    [0; NUM_OF_IRQ / u8::BITS as usize];

static mut IDT_LOCK: IrqSaveSpinLockFlag = IrqSaveSpinLockFlag::new();
static mut IDT: [GateDescriptor; IDT_MAX + 1] = [GateDescriptor::invalid(); IDT_MAX + 1];

/// InterruptManager has no SpinLockFlag, When you use this, be careful of Mutex.
///
/// This has io_apic and local_apic handler inner.
/// This struct may be changed in the future.
pub struct InterruptManager {
    lock: IrqSaveSpinLockFlag,
    kernel_cs: u16,
    user_cs: u16,
    local_apic: LocalApicManager,
    tss_manager: TssManager,
}

/// Interrupt Number
///
/// This enum is used to decide which index the specific device should use.
#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(usize)]
pub enum InterruptIndex {
    LocalApicTimer = 0xef,
    RescheduleIpi = 0xf8,
}

/// IST index for each interrupt.
//#[derive(Clone, Copy, Eq, PartialEq)]
enum IstIndex {
    //NormalInterrupt = 0,
    TaskSwitch = 1,
}

impl InterruptManager {
    pub const LIMIT_IDT: u16 = 0x100 * (size_of::<GateDescriptor>() as u16) - 1;

    /// Create InterruptManager with invalid data.
    ///
    /// Before use, **you must call [`init`]**.
    ///
    /// [`init`]: #method.init
    pub const fn new() -> InterruptManager {
        InterruptManager {
            lock: IrqSaveSpinLockFlag::new(),
            kernel_cs: 0,
            user_cs: 0,
            local_apic: LocalApicManager::new(),
            tss_manager: TssManager::new(),
        }
    }

    /// Initialize Gate Descriptors.
    ///
    /// This function sets valid address into the descriptors between IDT_DEVICE_MIN and IDT_MAX.
    /// This function is not set them as a valid descriptor.
    fn init_idt(&mut self) {
        unsafe extern "C" {
            fn irq_handler_list();
            fn irq_handler_list_end();
        }
        let irq_handler_list_address = irq_handler_list as *const fn() as usize;
        let irq_handler_entry_size = (irq_handler_list_end as *const fn() as usize
            - irq_handler_list_address)
            / (IDT_MAX - IDT_DEVICE_MIN + 1);
        let _lock = unsafe { IDT_LOCK.lock() };
        for i in IDT_DEVICE_MIN..=IDT_MAX {
            unsafe {
                IDT[i] = GateDescriptor::new(
                    irq_handler_list_address + irq_handler_entry_size * (i - IDT_DEVICE_MIN),
                    self.kernel_cs,
                    IstIndex::TaskSwitch as u8,
                    0,
                )
            };
        }
        drop(_lock);
    }

    /// Setup Interrupt Stack Table.
    ///
    /// This function allocates stack and set rsp into TSS.
    fn init_ist(&mut self) {
        let stack_size = ContextManager::DEFAULT_INTERRUPT_STACK_SIZE;
        let stack =
            alloc_non_linear_pages!(stack_size).expect("Cannot allocate stack for interrupts.");
        assert!(
            self.tss_manager
                .set_ist(IstIndex::TaskSwitch as u8, (stack + stack_size).to_usize())
        );
    }

    /// Setup RSP(for privilege level 0~2)
    ///
    /// This function allocates stack and set rsp into TSS.
    /// If allocating the stack is failed, this function will panic.
    /// The rsp must be in the range 0 ~ 2.
    #[allow(dead_code)]
    fn set_rsp(&mut self, rsp: u8, stack_size: MSize) -> bool {
        let stack = alloc_pages!(stack_size.to_order(None).to_page_order())
            .expect("Cannot allocate pages for rsp.");

        let _lock = self.lock.lock();
        self.tss_manager
            .set_rsp(rsp, (stack + stack_size).to_usize())
    }

    /// Init this manager.
    ///
    /// This function alloc page from memory manager and
    /// fills all of IDT converted from the allocated page with an invalid handler.
    /// After that, this also init LocalApicManager.
    pub fn init(&mut self, kernel_code_segment: u16, user_code_segment: u16) {
        let _lock = self.lock.lock();
        self.kernel_cs = kernel_code_segment;
        self.user_cs = user_code_segment;
        self.init_idt();
        self.tss_manager.load_current_tss();
        self.init_ist();
        self.init_syscall();
        self.local_apic.init();
        unsafe { self.flush() };
        drop(_lock);
        self.init_ipi();
    }

    /// Init this manager by copying some data from given manager.
    ///
    /// This function alloc page from memory manager and
    /// fills all of IDT converted from the allocated page with an invalid handler.
    /// After that, this also init LocalApicManager.
    /// This will be used to init the application processors.
    /// GDT and TSS Descriptor must be valid.
    pub fn init_ap(&mut self, original: &Self) {
        let _lock = self.lock.lock();
        self.kernel_cs = original.kernel_cs;
        self.user_cs = original.user_cs;
        self.tss_manager.load_current_tss();
        self.init_ist();
        self.init_syscall();
        self.local_apic
            .init_from_other_manager(original.get_local_apic_manager());
        unsafe { self.flush() };
        drop(_lock);
    }

    /// Init the Inter-Processors-Interrupt.
    ///
    /// This function makes interrupt handler for ipi.
    pub fn init_ipi(&mut self) {
        self.set_device_interrupt_function(
            InterruptManager::reschedule_ipi_handler,
            None,
            Some(InterruptIndex::RescheduleIpi as _),
            0,
            false,
        )
        .expect("Failed to setup IPI");
    }

    /// Flush IDT to cpu and apply it.
    ///
    /// This function sets the address of IDT into CPU.
    /// Unless you change the address of IDT, you don't have to call it.
    unsafe fn flush(&self) {
        let idtr = idt::DescriptorTableRegister {
            limit: InterruptManager::LIMIT_IDT,
            offset: core::ptr::addr_of!(IDT) as u64,
        };
        unsafe { cpu::lidt(&idtr as *const _ as usize) };
    }

    /// Return using selector.
    pub fn get_kernel_code_segment(&self) -> u16 {
        self.kernel_cs
    }

    /// Register interrupt handler.
    ///
    /// This function sets the function into IDT and
    ///  redirects the target interruption into this CPU (I/O APIC).
    ///
    ///  * function: the handler to call when the interruption occurs
    ///  * irq: If the target device interrupts by irq, set this argument.
    ///         If this is some(irq), this function will call [`super::device::io_apic::IoApicManager::set_redirect`].
    ///  * index: the index of IDT to connect handler
    ///  * privilege_level: the ring level to allow interrupt. If you want to allow user interrupt,
    ///                     set this to 3.
    ///
    ///  If index <= 32(means CPU internal exception) or index > 0xFF(means intel reserved area),
    ///  this function will return false.
    pub fn set_device_interrupt_function(
        &self,
        function: fn(usize) -> bool,
        irq: Option<u8>,
        index: Option<usize>,
        privilege_level: u8,
        is_level_trigger: bool,
    ) -> Result<usize, ()> {
        if let Some(index) = index {
            if index <= IDT_DEVICE_MIN || index > IDT_MAX {
                /* CPU exception interrupt */
                /* intel reserved */
                return Err(());
            }
            if let Some(irq) = irq {
                if Self::irq_to_index(irq) != index {
                    return Err(());
                }
            } else if index < IDT_AVAILABLE_MIN {
                /* To avoid conflict legacy IRQ Numbers */
                return Err(());
            }
        }
        let _self_lock = self.lock.lock();
        let _lock = unsafe { IDT_LOCK.lock() };
        let handler_index = if let Some(i) = index {
            i - IDT_DEVICE_MIN
        } else if let Some(irq) = irq {
            Self::irq_to_index(irq)
        } else if let Some(i) = Self::search_available_handler_index() {
            i
        } else {
            pr_err!("No available interrupt vector");
            return Err(());
        };
        let index = handler_index + IDT_DEVICE_MIN;
        let handler_address = unsafe { INTERRUPT_HANDLER[handler_index] };
        if handler_address != 0 {
            drop(_lock);
            drop(_self_lock);
            if handler_address == function as *const fn(usize) as usize {
                return Ok(index);
            }
            pr_err!("Index is in use.");
            return Err(());
        }
        unsafe { INTERRUPT_HANDLER[handler_index] = function as *const fn(usize) as usize };
        let type_attr: u8 = 0xe | (privilege_level & 0x3) << 5 | 1 << 7;
        unsafe { IDT[index].set_type_attributes(type_attr) };
        if let Some(irq) = irq {
            let irq_index = irq >> 3;
            let irq_offset = irq & 0b111;
            unsafe {
                IRQ_IS_LEVEL_TRIGGER[irq_index as usize] =
                    (IRQ_IS_LEVEL_TRIGGER[irq_index as usize] & !(1 << irq_offset))
                        | ((is_level_trigger as u8) << irq_offset)
            };
            drop(_lock);
            drop(_self_lock);
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
        } else {
            drop(_lock);
            drop(_self_lock);
        }
        Ok(index)
    }

    pub fn setup_msi_interrupt(
        &self,
        function: fn(usize) -> bool,
        _priority_level: Option<u8>,
        is_level_trigger: bool,
    ) -> Result<MsiInfo, ()> {
        let interrupt_id =
            self.set_device_interrupt_function(function, None, None, 0, is_level_trigger)?;
        let destination_id = self.local_apic.get_apic_id();
        let message_address = 0xfee00000u64 | ((destination_id as u64) << 12);
        let message_data = ((is_level_trigger as u64) << 15) | (1u64 << 14) | (interrupt_id as u64);
        Ok(MsiInfo {
            message_address,
            message_data,
            interrupt_id,
        })
    }

    fn search_available_handler_index() -> Option<usize> {
        for (index, e) in unsafe { INTERRUPT_HANDLER.iter().enumerate() } {
            if index + IDT_DEVICE_MIN < IDT_AVAILABLE_MIN {
                continue;
            }
            if *e == 0 {
                return Some(index);
            }
        }
        None
    }

    /// Save current the interrupt status and disable interrupt
    ///
    /// This function disables interrupt and return interrupt status before disable interrupt.
    /// The return value will be used by [`restore_local_irq`].
    /// This can be nested called.
    pub fn save_and_disable_local_irq() -> StoredIrqData {
        let r_flags = cpu::get_r_flags();
        unsafe { cpu::disable_interrupt() };
        StoredIrqData { r_flags }
    }

    /// Restore the interrupt status before calling [`save_and_disable_local_irq`]
    ///
    /// If the interrupt was enabled before calling [`save_and_disable_local_irq`],
    /// this will enable interrupt, otherwise this will not change the interrupt status.
    pub fn restore_local_irq(original: StoredIrqData) {
        unsafe { cpu::set_r_flags(original.r_flags) };
    }

    /// Restore the interrupt status with StoredIrqData reference.
    pub unsafe fn restore_local_irq_by_reference(original: &StoredIrqData) {
        unsafe { cpu::set_r_flags(original.r_flags) };
    }

    /// Send the end of interrupt to Local APIC.
    pub fn send_eoi(&self) {
        self.local_apic.send_eoi();
    }

    /// Send the end of interrupt to Local APIC and also send to I/O APIC.
    pub fn send_eoi_level_trigger(&self, vector: u8) {
        get_kernel_manager_cluster()
            .arch_depend_data
            .io_apic_manager
            .lock()
            .unwrap()
            .send_eoi(vector);
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
            InterruptIndex::RescheduleIpi as _,
        );
    }

    /// Setup syscall
    ///
    /// write syscall settings into MSRs
    pub fn init_syscall(&self) {
        unsafe extern "C" {
            fn syscall_handler_entry();
        }
        unsafe { cpu::wrmsr(MSR_EFER, cpu::rdmsr(MSR_EFER) | MSR_EFER_SYSCALL_ENABLE) };
        unsafe {
            cpu::wrmsr(
                MSR_LSTAR,
                syscall_handler_entry as *const fn() as usize as u64,
            )
        };
        unsafe {
            cpu::wrmsr(
                MSR_STAR,
                ((self.kernel_cs as u64) << 32) | (((self.user_cs - 16) as u64) << 48),
            )
        };
    }

    /// Convert IRQ to Interrupt Index
    pub const fn irq_to_index(irq: u8) -> usize {
        irq as usize + IDT_DEVICE_MIN
    }

    /// Convert Interrupt Index to IRQ if index is between IDT_DEVICE_MIN and IDT_AVAILABLE_MIN
    pub const fn index_to_irq(index: usize) -> Option<u8> {
        if index >= IDT_DEVICE_MIN && index < IDT_AVAILABLE_MIN {
            Some((index - IDT_DEVICE_MIN) as u8)
        } else {
            None
        }
    }

    fn reschedule_ipi_handler(_: usize) -> bool {
        /* Do nothing */
        true
    }

    /// Main handler for interrupt
    ///
    /// This function calls `schedule` if needed.
    extern "C" fn main_interrupt_handler(context_data: u64, index: usize) {
        let address = unsafe { INTERRUPT_HANDLER[index - IDT_DEVICE_MIN] };

        if address != 0 {
            if unsafe { core::mem::transmute::<usize, fn(usize) -> bool>(address)(index) } {
                if let Some(irq) = Self::index_to_irq(index) {
                    let irq_index = irq >> 3;
                    let irq_offset = irq & 0b111;
                    if unsafe { IRQ_IS_LEVEL_TRIGGER[irq_index as usize] & (1 << irq_offset) } != 0
                    {
                        get_cpu_manager_cluster()
                            .interrupt_manager
                            .send_eoi_level_trigger(index as u8);
                    }
                }
                get_cpu_manager_cluster().interrupt_manager.send_eoi();
            } else {
                pr_err!("Failed to process interrupt.");
            }
        } else {
            pr_err!("Invalid Interrupt: {:#X}", index);
        }
        if get_cpu_manager_cluster().run_queue.should_call_schedule() {
            get_cpu_manager_cluster()
                .run_queue
                .schedule(Some(unsafe { &*(context_data as *const ContextData) }));
        }
    }

    /// Main handler for syscall
    extern "C" fn main_syscall_handler(context_data: u64) {
        let context_data = unsafe { &mut *(context_data as *mut ContextData) };
        context_data.registers.gs_base = unsafe { cpu::rdmsr(0xC0000102) };
        let user_segment_base = (unsafe { cpu::rdmsr(MSR_STAR) } >> 48) & 0xffff;
        context_data.registers.cs = user_segment_base + 16;
        context_data.registers.ss = user_segment_base + 8;
        crate::kernel::system_call::system_call_handler(context_data);
    }
}

global_asm!("
.macro  handler index, max
sub     rsp, ({0} + 1) * 8 // +1 is for stack alignment
mov     [rsp +  5 * 8], rsi
mov     rsi, \\index
jmp     handler_entry
.align  8
.if     \\max - \\index - 1
handler \"(\\index+1)\",\\max
.endif
.endm

.macro handler_block base, end
handler \\base, (\\base + 0x10)
.if     \\end - \\base
handler_block \"(\\base + 0x10)\",\\end
.endif
.endm 

.section    .text
.type       irq_handler_list, %function
irq_handler_list:
handler_block  0x20, 0x40
handler_block  0x50, 0x70
handler_block  0x80, 0xa0
handler_block  0xb0, 0xd0
handler_block  0xe0, 0xf0
irq_handler_list_end:
.size   irq_handler_list, irq_handler_list_end - irq_handler_list

",
 const crate::arch::target_arch::context::context_data::ContextData::NUM_OF_REGISTERS,
);

global_asm!("
.section    .text
.type       handler_entry, %function
handler_entry:
    mov     [rsp +  0 * 8] ,rax
    mov     [rsp +  1 * 8], rdx
    mov     [rsp +  2 * 8], rcx
    mov     [rsp +  3 * 8], rbx
    mov     [rsp +  4 * 8], rbp
    //mov     [rsp +  5 * 8], rsi
    mov     [rsp +  6 * 8], rdi
    mov     [rsp +  7 * 8], r8
    mov     [rsp +  8 * 8], r9
    mov     [rsp +  9 * 8], r10
    mov     [rsp + 10 * 8], r11
    mov     [rsp + 11 * 8], r12
    mov     [rsp + 12 * 8], r13
    mov     [rsp + 13 * 8], r14
    mov     [rsp + 14 * 8], r15     
    xor     rax, rax
    mov     ax, ds
    mov     [rsp + 15 * 8], rax            
    mov     ax, fs
    mov     [rsp + 16 * 8], rax
    rdfsbase rax
    mov     [rsp + 17 * 8], rax
    mov     ax, gs
    mov     [rsp + 18 * 8], rax
    rdgsbase rax
    mov     [rsp + 19 * 8], rax
    mov     ax, es
    mov     [rsp + 20 * 8], rax
    mov     ax, ss
    mov     [rsp + 21 * 8], rax
    mov     rax, [rsp + (3 + ({0} + 1)) * 8]   // RSP
    mov     [rsp + 22 * 8], rax
    mov     rax, [rsp + (2 + ({0} + 1)) * 8]   // RFLAGS
    mov     [rsp + 23 * 8], rax
    mov     rax, [rsp + (1 + ({0} + 1)) * 8]   // CS
    mov     [rsp + 24 * 8], rax
    mov     rax, [rsp + (0 + ({0} + 1)) * 8]   // RIP
    mov     [rsp + 25 * 8], rax
    mov     rax, cr3
    mov     [rsp + 26 * 8], rax
    sub     rsp, 512
    fxsave  [rsp]
    mov     rax, cs
    cmp     [rsp + 512 +  ({0} + 1) * 8 + 8], rax
    je      1f
    swapgs
1:
    mov     rbp, rsp
    mov     rdi, rsp
    call    {1}
    mov     rsp, rbp
    mov     rax, cs
    cmp     [rsp + 512 +  ({0} + 1) * 8 + 8], rax
    je      2f
    swapgs
2:
    fxrstor [rsp]
    add     rsp, 512
    // Ignore CR3, RIP, CS, RFLAGS, RSP, DS, SS, GS, ES, FS
    mov     rax, [rsp +  0 * 8]
    mov     rdx, [rsp +  1 * 8]
    mov     rcx, [rsp +  2 * 8]
    mov     rbx, [rsp +  3 * 8]
    mov     rbp, [rsp +  4 * 8]
    mov     rsi, [rsp +  5 * 8]
    mov     rdi, [rsp +  6 * 8]
    mov     r8,  [rsp +  7 * 8]
    mov     r9,  [rsp +  8 * 8]
    mov     r10, [rsp +  9 * 8]
    mov     r11, [rsp + 10 * 8]
    mov     r12, [rsp + 11 * 8]
    mov     r13, [rsp + 12 * 8]
    mov     r14, [rsp + 13 * 8]
    mov     r15, [rsp + 14 * 8] 
    add     rsp, ({0} + 1) * 8
    iretq
.size   handler_entry, . - handler_entry
",
    const crate::arch::target_arch::context::context_data::ContextData::NUM_OF_REGISTERS,
    sym InterruptManager::main_interrupt_handler
);

global_asm!("
.section    .text
.type       syscall_handler_entry, %function
syscall_handler_entry:
    swapgs
    sub     rsp, ({0} + 1) * 8 // +1 is for stack alignment
    mov     [rsp +  0 * 8] ,rax
    mov     [rsp +  1 * 8], rdx
    mov     [rsp +  2 * 8], rcx
    mov     [rsp +  3 * 8], rbx
    mov     [rsp +  4 * 8], rbp
    mov     [rsp +  5 * 8], rsi
    mov     [rsp +  6 * 8], rdi
    mov     [rsp +  7 * 8], r8
    mov     [rsp +  8 * 8], r9
    mov     [rsp +  9 * 8], r10
    mov     [rsp + 10 * 8], r11
    mov     [rsp + 11 * 8], r12
    mov     [rsp + 12 * 8], r13
    mov     [rsp + 13 * 8], r14
    mov     [rsp + 14 * 8], r15     
    xor     rax, rax
    mov     ax, ds
    mov     [rsp + 15 * 8], rax            
    mov     ax, fs
    mov     [rsp + 16 * 8], rax
    rdfsbase rax
    mov     [rsp + 17 * 8], rax
    mov     ax, gs
    mov     [rsp + 18 * 8], rax
    mov     ax, es
    mov     [rsp + 20 * 8], rax
    mov     ax, ss
    mov     [rsp + 21 * 8], rax
    mov     [rsp + 23 * 8], r11                // RFLAGS
    mov     [rsp + 25 * 8], rcx                // RIP
    mov     rax, cr3
    mov     [rsp + 26 * 8], rax
    sub     rsp, 512
    //fxsave  [rsp]

    mov     rbp, rsp
    mov     rdi, rsp
    call    {1}
    mov     rsp, rbp

    //fxrstor [rsp]
    add     rsp, 512
    // Ignore CR3, RIP, CS, RSP, DS, SS, GS, ES, FS
    mov     rax, [rsp +  0 * 8]
    mov     rdx, [rsp +  1 * 8]
    mov     rcx, [rsp +  2 * 8]
    mov     rbx, [rsp +  3 * 8]
    mov     rbp, [rsp +  4 * 8]
    mov     rsi, [rsp +  5 * 8]
    mov     rdi, [rsp +  6 * 8]
    mov     r8,  [rsp +  7 * 8]
    mov     r9,  [rsp +  8 * 8]
    mov     r10, [rsp +  9 * 8]
    mov     r11, [rsp + 10 * 8]
    mov     r12, [rsp + 11 * 8]
    mov     r13, [rsp + 12 * 8]
    mov     r14, [rsp + 13 * 8]
    mov     r15, [rsp + 14 * 8] 
    add     rsp, ({0} + 1) * 8
    swapgs
    sysretq
.size   syscall_handler_entry, . - syscall_handler_entry
",
    const crate::arch::target_arch::context::context_data::ContextData::NUM_OF_REGISTERS,
    sym InterruptManager::main_syscall_handler
);
