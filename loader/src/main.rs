#![feature(const_convert)]
#![feature(const_ops)]
#![feature(const_trait_impl)]
#![feature(step_trait)]
#![no_std]
#![no_main]

#[macro_use]
mod print;

mod memory;

#[allow(dead_code)]
pub mod kernel {
    pub mod drivers {
        #[allow(dead_code)]
        pub mod boot_information; // TODO: copied from the kernel
        #[allow(dead_code)]
        pub mod dtb; // TODO: Copy from the kernel
    }

    pub mod file_manager {
        pub mod elf; // Copied from the kernel
    }
    pub mod memory_manager {
        pub mod data_type; // Copied from the kernel
        pub mod physical_memory_manager;
    }
}

pub mod arch {
    #![allow(dead_code)]
    #[cfg(target_arch = "riscv64")]
    pub mod riscv;

    #[cfg(target_arch = "riscv64")]
    pub use riscv as target_arch;
}

const KERNEL_STACK_PAGES: usize = 64;
const LOADER_STACK_PAGES: usize = 4;

use arch::target_arch::ELF_MACHINE_NATIVE;
use arch::target_arch::context::memory_layout;
use arch::target_arch::device::cpu;
use arch::target_arch::paging::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE, PAGE_SIZE_USIZE, PageManager};

use kernel::drivers::boot_information;
use kernel::drivers::dtb;
use kernel::file_manager::elf;
use kernel::memory_manager::{data_type::*, physical_memory_manager::PhysicalMemoryManager};

use core::mem::MaybeUninit;

#[cfg(target_os = "none")]
#[unsafe(link_section = ".kernel")]
static KERNEL: &[u8] = include_bytes!("../../bin/kernel.elf");

static mut BOOT_INFO: MaybeUninit<boot_information::BootInformation> = MaybeUninit::uninit();

/// The main function booted without UEFI
///
/// # Function's Argument
/// - argc: the number of arguments, must be more than one.
/// - argv: the array of arguments, each points the null terminated string.
///
/// # Boot Arguments
/// - argv[0] : Ignored
/// - argv[1] : The device tree address
/// - argv[2] : (Optional) The UART address to write
/// - argv[3] : (Optional) The offset of TX Empty Register
/// - argv[4] : (Optional) The value to wait UART FIFO
///
/// # UART control
/// `println!` will write *(argv[2] as *mut u32).
/// If argcv[3] and argv[4] are specfied, the printer will wait
/// while `(*((argv[2] + argv[3]) as *const u32) & argv[4]) == 0`
#[cfg(target_os = "none")]
extern "C" fn baremetal_main(argc: usize, argv: *const *const u8) -> ! {
    use core::ffi::CStr;
    unsafe extern "C" {
        static __LOADER_END: usize;
    }
    let stack_base_address =
        (cpu::get_stack_pointer() & PAGE_MASK) - PAGE_SIZE_USIZE * LOADER_STACK_PAGES;
    let stack_end_address = stack_base_address + PAGE_SIZE_USIZE * LOADER_STACK_PAGES;
    let loader_base_address = cpu::get_instruction_pointer() & PAGE_MASK;
    let loader_end_address = loader_base_address
        + ((&raw const __LOADER_END as *const _ as usize - 1) & PAGE_MASK)
        + PAGE_SIZE_USIZE;
    let loader_area = [
        (
            loader_base_address,
            loader_end_address - loader_base_address,
        ),
        (stack_base_address, stack_end_address - stack_base_address),
    ];

    /* Parse arguments */
    let args = unsafe { core::slice::from_raw_parts(argv, argc) };
    let get_arg = |n: usize| {
        if argc > n {
            unsafe { CStr::from_ptr(args[n]) }
                .to_str()
                .ok()
                .and_then(|s| str_to_usize(s))
        } else {
            None
        }
    };

    let dtb_address = get_arg(1).expect("Invalid arguments: expected dtb_address");
    assert_ne!(dtb_address, 0);

    if argc >= 3 {
        let uart_address = get_arg(2).expect("Failed to get the UART address");
        assert_ne!(uart_address, 0);
        let wait_offset = get_arg(3).map(|o| o as u32);
        let wait_value = get_arg(4).map(|v| v as u32);

        print::set_uart_address(uart_address, wait_offset, wait_value);
    }

    println!("Boot Loader version {}", env!("CARGO_PKG_VERSION"));
    println!("Loader range:\t[{loader_base_address:#18X} ~ {loader_end_address:#18X}]");
    println!("Stack  range:\t[{stack_base_address:#18X} ~ {stack_end_address:#18X}]");

    let boot_information = unsafe { (&raw mut BOOT_INFO).as_mut().unwrap() };
    let dtb = dtb::DtbManager::new(dtb_address).expect("Failed to get DTB");

    /* Initialize memory allocator */

    memory::init_memory_allocator(&dtb, loader_area.as_slice(), unsafe {
        &mut boot_information.assume_init_mut().ram_map
    });
    let mut pm_manager = PhysicalMemoryManager::new();

    /* Load kernel ELF and map them */
    let elf_address = KERNEL.as_ptr() as usize;
    let read_func = |offset: usize, size: usize, dst: *mut u8| {
        unsafe { core::ptr::copy_nonoverlapping((elf_address + offset) as *const u8, dst, size) };
    };
    println!("Load the kernel...");
    let entry_point = load_kernel(read_func, &mut pm_manager, unsafe {
        boot_information.assume_init_mut()
    });
    println!("Kernel's entry point: {entry_point:#X}");

    /* Set up the page table */
    let page_manager =
        init_paging(&mut pm_manager).expect("Failed to allocate a page for page tables");
    map_kernel(&mut pm_manager, &page_manager, unsafe {
        boot_information
            .assume_init_ref()
            .elf_program_headers
            .as_slice()
    });
    map_direct_area(&mut pm_manager, &page_manager);
    map_loader(&mut pm_manager, &page_manager, loader_area.as_slice());

    println!("Dump the initial page table for the kernel");
    page_manager.dump_table(None, None);

    /* Allocate the kernel stack */
    let kernel_stack = memory::allocate_pages(KERNEL_STACK_PAGES)
        .expect("Failed to allocate the stack")
        + (KERNEL_STACK_PAGES * PAGE_SIZE_USIZE)
        + memory_layout::get_direct_map_start_address().to_usize();

    /* Adjust the address to the direct mapped address */
    adjust_boot_info(unsafe { boot_information.assume_init_mut() });

    /* Set up the system registers if necessary */
    arch::target_arch::setup_environment();

    /* Set the page table and jump to the kernel*/
    println!("Jump to the kernel...");
    cpu::flush_all_cache();
    unsafe { cpu::disable_interrupt() };
    arch::target_arch::jump_to_kernel(
        entry_point,
        boot_information.as_mut_ptr() as usize,
        kernel_stack,
        page_manager,
    )
}

fn load_kernel<F>(
    read: F,
    pm_manager: &mut PhysicalMemoryManager,
    boot_information: &mut boot_information::BootInformation,
) -> usize
where
    F: Fn(usize, usize, *mut u8),
{
    read(
        0,
        size_of::<elf::Elf64Header>(),
        &mut boot_information.elf_header_buffer as *mut u8,
    );

    let elf_header =
        unsafe { elf::Elf64Header::from_ptr_mut(&mut boot_information.elf_header_buffer) }
            .expect("Invalid ELF file");
    assert!(
        elf_header.is_executable_file() && elf_header.get_machine_type() == ELF_MACHINE_NATIVE,
        "ELF file is not for this computer."
    );

    /* Read ELF Program Header */
    let elf_program_headers = &mut boot_information.elf_program_headers;
    let elf_program_headers_size = elf_header.get_program_headers_array_size() as usize;

    assert_ne!(elf_program_headers_size, 0, "Invalid ELF file");
    assert!(
        elf_program_headers_size <= size_of_val(elf_program_headers),
        "The array pf program headers is too big"
    );

    read(
        elf_header.get_program_header_offset() as usize,
        elf_program_headers_size,
        elf_program_headers as *mut _ as usize as *mut u8,
    );

    println!(
        "{:^18} | {:^18} | {:^12} | {:^12} | {:^12} | {:^12} | {:^5} | {:^5} | {:^5}",
        "Physical Address",
        "Virtual Address",
        "Memory Size",
        "File Size",
        "File Offset",
        "Alignment",
        "Read",
        "Write",
        "Exec"
    );

    /* Load and map segments */
    for entry in elf_header.get_program_headers_iter_mut(elf_program_headers as *const _ as usize) {
        let segment_type = entry.get_segment_type();
        let virtual_address = entry.get_virtual_address() as usize;
        let memory_size = entry.get_memory_size() as usize;
        let file_size = entry.get_file_size() as usize;
        let file_offset = entry.get_file_offset() as usize;
        let alignment = entry.get_align().max(1) as usize;

        if segment_type != elf::ELF_PROGRAM_HEADER_SEGMENT_LOAD || memory_size == 0 {
            continue;
        }

        let aligned_memory_size = ((memory_size - 1) & PAGE_MASK) + PAGE_SIZE_USIZE;
        let physical_address = pm_manager
            .alloc(MSize::new(aligned_memory_size), MOrder::new(PAGE_SHIFT))
            .expect("Failed to allocate memory")
            .to_usize();

        println!(
            "{physical_address:#018X} | {virtual_address:#018X} | {memory_size:#012X} | \
             {file_size:#012X} | {file_offset:#012X} | {alignment:#012X} | {:>5} | {:>5} | {:>5}",
            entry.is_segment_readable(),
            entry.is_segment_writable(),
            entry.is_segment_executable()
        );

        if file_size > 0 {
            read(
                entry.get_file_offset() as usize,
                entry.get_file_size() as usize,
                physical_address as *mut _,
            );
        }
        if memory_size > file_size {
            unsafe {
                core::ptr::write_bytes(
                    (physical_address + file_size) as *mut u8,
                    0,
                    memory_size - file_size,
                )
            };
        }
        entry.set_physical_address(physical_address as u64);
    }

    // Adjust program headers to `[Elf64ProgramHeader; N]`
    if size_of::<elf::Elf64ProgramHeader>() != elf_header.get_program_headers_entry_size() as usize
    {
        let original_entry_size = elf_header.get_program_headers_entry_size() as usize;
        let num_of_entries = elf_header.get_num_of_program_headers() as usize;
        let array_size = elf_program_headers.len();

        for i in 0..num_of_entries {
            unsafe {
                core::ptr::copy(
                    (elf_program_headers as *mut _ as usize + i * original_entry_size)
                        as *const elf::Elf64ProgramHeader,
                    &mut elf_program_headers[i],
                    1,
                );
            }
        }
        if num_of_entries < array_size {
            unsafe {
                core::ptr::write_bytes(
                    &mut elf_program_headers[num_of_entries],
                    0,
                    array_size - num_of_entries,
                )
            };
        }
    }

    elf_header.get_entry_point() as usize
}

fn init_paging(pm_manager: &mut PhysicalMemoryManager) -> Result<PageManager, ()> {
    let mut page_manager = PageManager::new();
    page_manager.init(pm_manager).map_err(|e| {
        println!("Initializiting the page table was failed: {:?}", e);
    })?;
    Ok(page_manager)
}

fn map_kernel(
    pm_manager: &mut PhysicalMemoryManager,
    page_manager: &PageManager,
    progmram_headers: &[elf::Elf64ProgramHeader],
) {
    for entry in progmram_headers.iter() {
        let segment_type = entry.get_segment_type();
        let virtual_address = VAddress::new(entry.get_virtual_address() as usize);
        let physical_address = PAddress::new(entry.get_physical_address() as usize);
        let memory_size = entry.get_memory_size() as usize;

        if segment_type != elf::ELF_PROGRAM_HEADER_SEGMENT_LOAD || memory_size == 0 {
            continue;
        }

        let aligned_memory_size = MSize::new((memory_size - 1) & PAGE_MASK) + PAGE_SIZE;

        page_manager
            .associate_address(
                pm_manager,
                physical_address,
                virtual_address,
                aligned_memory_size,
                MemoryPermissionFlags::new(
                    entry.is_segment_readable() && !entry.is_segment_executable(),
                    entry.is_segment_writable(),
                    entry.is_segment_executable(),
                    false,
                ),
                MemoryOptionFlags::KERNEL | MemoryOptionFlags::ALLOW_HUGE,
            )
            .expect("Failed to map kernel");
    }
}

fn map_direct_area(pm_manager: &mut PhysicalMemoryManager, page_manager: &PageManager) {
    let start = memory_layout::get_direct_map_start_address();
    let size = memory_layout::get_direct_map_size();
    let base = memory_layout::get_direct_map_base_address();

    page_manager
        .associate_address(
            pm_manager,
            base,
            start,
            size,
            MemoryPermissionFlags::new(true, true, false, false),
            MemoryOptionFlags::KERNEL | MemoryOptionFlags::ALLOW_HUGE,
        )
        .expect("Failed to setup direct map");
}

fn map_loader(
    pm_manager: &mut PhysicalMemoryManager,
    page_manager: &PageManager,
    loader_area: &[(usize, usize)],
) {
    for e in loader_area {
        let start = PAddress::new(e.0);
        let size = MSize::new(e.1);

        page_manager
            .associate_address(
                pm_manager,
                start,
                unsafe { start.to_direct_mapped_v_address() },
                size,
                MemoryPermissionFlags::new(true, true, true, false),
                MemoryOptionFlags::KERNEL | MemoryOptionFlags::ALLOW_HUGE,
            )
            .expect("Failed to map the loader area");
    }
}

fn adjust_boot_info(_boot_info: &mut boot_information::BootInformation) {}

fn str_to_usize(s: &str) -> Option<usize> {
    let radix;
    let start;
    match s.get(0..2) {
        Some("0x") => {
            radix = 16;
            start = s.get(2..);
        }
        Some("0o") => {
            radix = 8;
            start = s.get(2..);
        }
        Some("0b") => {
            radix = 2;
            start = s.get(2..);
        }
        _ => {
            radix = 10;
            start = Some(s);
        }
    }
    usize::from_str_radix(start?, radix).ok()
}

#[panic_handler]
pub fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("\n\nPanic");
    if let Some(location) = info.location() {
        println!(
            "{}:{}: {}",
            location.file(),
            location.line(),
            info.message()
        );
    } else {
        println!("Message: {}", info.message());
    }
    loop {
        unsafe { crate::arch::target_arch::device::cpu::idle() };
    }
}
