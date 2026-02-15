//!
//! Starfive JH7110 Timer
//!
//! compatible: "starfive,jh7110-timers"
//!

use crate::arch::target_arch::device::cpu;
use crate::arch::target_arch::get_hartid;

use crate::kernel::drivers::dtb::{DtbManager, DtbNodeInfo};
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{MSize, MemoryPermissionFlags, PAddress, VAddress};
use crate::kernel::memory_manager::io_remap;
use crate::kernel::timer_manager::GlobalTimerManager;

use core::ptr::{read_volatile, write_volatile};

#[repr(C)]
struct Channel {
    int_status: u32,
    control: u32, /* [0]: 1 => one shot, 0 => interval */
    load_value: u64,
    enable: u32,         /* Actually, bool */
    reload_counter: u32, /* writing 1 triggers reload */
    timer_value: u64,
    timer_interrupt_clear: u32,
    timer_interrupt_mask: u32,
}

pub struct Jh7110Timer {
    base_address: VAddress,
    range: MSize,
    frequency: u32,
    int_id: [u32; Self::NUMBER_OF_CHANNEL],
}

macro_rules! read_register {
    ($s:expr, $reg:ident) => {
        unsafe {
            read_volatile(
                &((&*($s.base_address + MSize::new(Self::CHANNEL_SIZE * (get_hartid() as usize)))
                    .to::<Channel>())
                    .$reg) as *const _,
            )
        }
    };
}

macro_rules! write_register {
    ($s:expr, $reg:ident, $val:expr) => {
        unsafe {
            write_volatile(
                &mut ((&mut *($s.base_address
                    + MSize::new(Self::CHANNEL_SIZE * (get_hartid() as usize)))
                .to::<Channel>())
                    .$reg) as *mut _,
                $val,
            )
        }
    };
}

impl Jh7110Timer {
    pub const DTB_COMPATIBLE: &[u8] = b"starfive,jh7110-timers";
    const NUMBER_OF_CHANNEL: usize = 4;
    const CHANNEL_SIZE: usize = 0x40;

    pub const fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
            range: MSize::new(0),
            frequency: 0,
            int_id: [0; Self::NUMBER_OF_CHANNEL],
        }
    }

    fn init_timer(&self) {
        /* Set One shot Mode(1) */
        write_register!(self, control, 1u32);
        self.clear_interrupt();
    }

    pub fn init_with_dtb(&mut self, dtb_manager: &DtbManager, node: &DtbNodeInfo) -> bool {
        let Some((address, size)) = dtb_manager.read_reg_property(node, 0) else {
            pr_err!("Failed to get base address");
            return false;
        };

        /* Interrupts */
        let Some(interrupts) = dtb_manager.get_property(node, b"interrupts") else {
            pr_err!("Failed to get interrupts");
            return false;
        };
        let interrupts = dtb_manager.read_property_as_u8_array(&interrupts);
        pr_info!("Interrupts: {interrupts:#X?}");
        if interrupts.len() != Self::NUMBER_OF_CHANNEL {
            pr_err!("Invalid interrupts source: {interrupts:#X?}");
            return false;
        }
        for (d, s) in self.int_id.iter_mut().zip(interrupts.iter()) {
            *d = *s as _;
        }

        let Some(frequency) = dtb_manager
            .get_property(node, b"frequency")
            .and_then(|p| dtb_manager.read_property_as_u32(&p, 0))
        else {
            pr_err!("Failed to get frequency");
            return false;
        };
        self.frequency = frequency;
        let Ok(address) = io_remap!(
            PAddress::new(address),
            MSize::new(size),
            MemoryPermissionFlags::data()
        ) else {
            pr_err!("Failed to map MMIO");
            return false;
        };
        self.base_address = address;
        self.range = MSize::new(size);
        if let Some(interrupt_id) = interrupts.get(get_hartid() as usize) {
            if get_cpu_manager_cluster()
                .interrupt_manager
                .set_device_interrupt_function(Self::interrupt_handler, *interrupt_id as _, 0, true)
                .is_err()
            {
                pr_err!("Failed to set up the timer interrupt");
            }
        } else {
            pr_err!("Timer interrupt is not found");
        }
        self.init_timer();
        true
    }

    pub fn init_ap(&mut self, original: &Self) {
        assert!(get_hartid() < Self::NUMBER_OF_CHANNEL as u64);
        self.base_address = original.base_address;
        self.range = original.range;
        self.frequency = original.frequency;
        self.int_id = original.int_id;
        self.init_timer();
    }

    pub fn start_interrupt(&self) {
        /* Disable Timer Interrupt */
        write_register!(self, timer_interrupt_mask, 1u32);
        self.clear_interrupt();
        self.reload_timeout_value();
        /* Enable Interrupt and unmask */
        write_register!(self, enable, 1u32);
        write_register!(self, timer_interrupt_clear, 0u32);
    }

    pub fn reload_timeout_value(&self) {
        let reset_value = (GlobalTimerManager::TIMER_INTERVAL_MS * self.frequency as u64) / 1000;
        write_register!(self, load_value, reset_value);
        cpu::memory_barrier();
        write_register!(self, timer_interrupt_clear, 1u32);
    }

    fn clear_interrupt(&self) {
        write_register!(self, timer_interrupt_clear, 1u32);
    }

    fn interrupt_handler(_interrupt_id: usize) -> bool {
        // TODO: get dynamically
        let timer = &mut get_cpu_manager_cluster().arch_depend_data.jh7110_timer;
        if (read_register!(timer, int_status) & (1u32 << get_hartid() as u8)) == 0u32 {
            pr_warn!("Timer interrupt is not fired...");
            return false;
        }

        get_cpu_manager_cluster()
            .local_timer_manager
            .local_timer_handler();
        if get_kernel_manager_cluster().boot_strap_cpu_manager.cpu_id
            == get_cpu_manager_cluster().cpu_id
        {
            get_kernel_manager_cluster()
                .global_timer_manager
                .global_timer_handler();
        }
        timer.clear_interrupt();
        timer.reload_timeout_value();
        true
    }
}
