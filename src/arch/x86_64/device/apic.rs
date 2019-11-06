/*
APIC
*/


use arch::target_arch::device::cpu;

const LOCAL_APIC_MSR_INDEX: u32 = 0x1b;
const LOCAL_APIC_BASE_ADDR_MASK: u64 = 0xffffffffff000;
const LOCAL_APIC_ENABLED_MASK: u64 = 0x800;
const LOCAL_APIC_X2APIC_ENABLED_MASK: u64 = 0x400;

pub fn init_local_apic() {
    let mut local_apic_msr = unsafe {
        cpu::rdmsr(LOCAL_APIC_MSR_INDEX)
    };
    let base_addr = local_apic_msr & LOCAL_APIC_BASE_ADDR_MASK;
    let is_x2apic_supported = unsafe {
        let mut eax = 1u32;
        let mut ebx = 0u32;
        let mut ecx = 0u32;
        let mut edx = 0u32;
        cpu::cpuid(&mut eax, &mut ebx, &mut ecx, &mut edx);
        ecx & (1 << 21) != 0
    };
    if local_apic_msr & LOCAL_APIC_X2APIC_ENABLED_MASK == 0 && is_x2apic_supported {
        unsafe {
            cpu::wrmsr(LOCAL_APIC_MSR_INDEX,
                       local_apic_msr | LOCAL_APIC_X2APIC_ENABLED_MASK);
            local_apic_msr = cpu::rdmsr(LOCAL_APIC_MSR_INDEX);
        }
    }
    println!("Local APIC is enabled:{},and x2APIC is enabled:{}",
             (local_apic_msr & LOCAL_APIC_ENABLED_MASK != 0),
             (local_apic_msr & LOCAL_APIC_X2APIC_ENABLED_MASK != 0));
    println!("Local APIC address:{:x}", base_addr);
}