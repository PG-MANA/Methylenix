/*
Local APIC
*/


use arch::target_arch::device::cpu;

const LOCAL_APIC_MSR_INDEX: u32 = 0x1b;
const LOCAL_APIC_BASE_ADDR_MASK: u64 = 0xffffffffff000;
const LOCAL_APIC_ENABLED_MASK: u64 = 0x800;
const LOCAL_APIC_X2APIC_ENABLED_MASK: u64 = 0x400;
const CPUID_X2APIC_MASK: u32 = 1 << 21;
const LOCAL_X2APIC_MSR_INDEX: u32 = 0x800;
const LOCAL_APIC_SIR: u32 = 0x0f;

pub fn init_local_apic() -> u32 {
    let mut local_apic_msr = unsafe {
        cpu::rdmsr(LOCAL_APIC_MSR_INDEX)
    };
    let base_addr = (local_apic_msr & LOCAL_APIC_BASE_ADDR_MASK) as usize;
    let is_x2apic_supported = unsafe {
        let mut eax = 1u32;
        let mut ebx = 0u32;
        let mut ecx = 0u32;
        let mut edx = 0u32;
        cpu::cpuid(&mut eax, &mut ebx, &mut ecx, &mut edx);
        ecx & CPUID_X2APIC_MASK != 0
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
    write_apic_register(LOCAL_APIC_SIR,
                        read_apic_register(LOCAL_APIC_SIR,
                                           is_x2apic_supported, base_addr) | 0x100,
                        is_x2apic_supported, base_addr);

    read_apic_register(2, is_x2apic_supported, base_addr)
}

pub fn send_eoi() {
    //Super Temporary
    let mut local_apic_msr = unsafe {
        cpu::rdmsr(LOCAL_APIC_MSR_INDEX)
    };
    let base_addr = (local_apic_msr & LOCAL_APIC_BASE_ADDR_MASK) as usize;
    write_apic_register(0xb, 0, local_apic_msr & LOCAL_APIC_X2APIC_ENABLED_MASK != 0, base_addr);
}

fn read_apic_register(index: u32/*TODO:Enum*/, is_x2apic: bool, base_address: usize) -> u32 {
    if is_x2apic {
        unsafe {
            cpu::rdmsr(LOCAL_X2APIC_MSR_INDEX + index) as u32
        }
    } else {
        unsafe {
            *((base_address + (index as usize) * 0x10) as *const u32)
        }
    }
}

fn write_apic_register(index: u32/*TODO:Enum*/, data: u32, is_x2apic: bool, base_address: usize) {
    if is_x2apic {
        unsafe {
            cpu::wrmsr(LOCAL_X2APIC_MSR_INDEX + index, data as u64);
        }
    } else {
        unsafe {
            *((base_address + (index as usize) * 0x10) as *mut u32) = data;
        }
    }
}