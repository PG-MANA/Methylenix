#![feature(const_convert)]
#![feature(const_ops)]
#![feature(const_trait_impl)]
#![feature(step_trait)]
#![no_std]
#![no_main]

#[macro_use]
mod print;

#[cfg(target_os = "uefi")]
mod efi;

#[cfg(target_os = "uefi")]
pub use efi::memory::allocate_pages;

#[cfg(target_os = "none")]
mod baremetal;

#[cfg(target_os = "none")]
pub use baremetal::memory::allocate_pages;

/// Those modules are imported from the kernel source code.
/// The symbolic link may be invalid on some environments,
/// therefore [`include!`] macro must be used.
#[allow(dead_code)]
pub mod kernel {
    pub mod collections {
        pub mod guid {
            include!("../../src/kernel/collections/guid.rs");
        }
    }

    pub mod drivers {
        pub mod boot_information {
            include!("../../src/kernel/drivers/boot_information.rs");
        }
        /// Currently, there is some differences
        /// TODO: Link to the kernel one
        pub mod dtb;
        pub mod efi {
            include!("../../src/kernel/drivers/efi/mod.rs");
        }
    }

    pub mod file_manager {
        pub mod elf {
            include!("../../src/kernel/file_manager/elf.rs");
        }
    }
    pub mod memory_manager {
        pub mod data_type {
            include!("../../src/kernel/memory_manager/data_type.rs");
        }
        /// The loader's Physical Memory Manager is different from the kernel one.
        pub mod physical_memory_manager;
    }
}

pub mod arch {
    #[cfg(target_arch = "aarch64")]
    pub mod aarch64;

    #[cfg(target_arch = "aarch64")]
    pub use aarch64 as target_arch;

    #[cfg(target_arch = "riscv64")]
    pub mod riscv;

    #[cfg(target_arch = "riscv64")]
    pub use riscv as target_arch;

    #[cfg(target_arch = "x86_64")]
    pub mod x86_64;

    #[cfg(target_arch = "x86_64")]
    pub use x86_64 as target_arch;
}

const KERNEL_STACK_PAGES: usize = 64;
const LOADER_STACK_PAGES: usize = 4;

use arch::target_arch::ELF_MACHINE_NATIVE;
use arch::target_arch::context::memory_layout;
use arch::target_arch::device::cpu;
use arch::target_arch::paging::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE, PAGE_SIZE_USIZE, PageManager};

use kernel::drivers::boot_information::BootInformation;
use kernel::file_manager::elf;
use kernel::memory_manager::{data_type::*, physical_memory_manager::PhysicalMemoryManager};

#[cfg(target_os = "none")]
use kernel::drivers::dtb;

#[cfg(target_os = "uefi")]
use kernel::drivers::efi::{EfiHandle, EfiStatus, EfiSystemTable};

#[cfg(target_os = "none")]
#[unsafe(link_section = ".kernel")]
static KERNEL: &[u8] = include_bytes!("../../bin/kernel.elf");

#[cfg(target_os = "uefi")]
const KERNEL_PATH: &str = "\\kernel.elf";

#[cfg(target_os = "uefi")]
const FONT_PATH: &str = "\\font";

/// The main function booted without UEFI
///
/// # Function's Argument
/// - argc: The number of arguments, must be more than one.
/// - argv: The array of arguments, each points the null terminated string.
/// - loader_base_address: The base addres of the loader which will be set by `_start`
/// - loader_end_address : The end address of the loader which will be set by `_start`
///
/// # Boot Arguments
/// - argv\[0\] : Ignored
/// - argv\[1\] : The device tree address
/// - argv\[2\] : (Optional) The UART address to write
/// - argv\[3\] : (Optional) The offset of TX Empty Register
/// - argv\[4\] : (Optional) The value to wait UART FIFO
///
/// # UART control
/// `println!` will write chars into `*(argv[2] as *mut u32)`.
/// If argv\[3\] and argv\[4\] are specified, the printer will wait
/// while `(*((argv[2] + argv[3]) as *const u32) & argv[4]) == 0`
#[cfg(target_os = "none")]
extern "C" fn baremetal_main(
    argc: usize,
    argv: *const *const u8,
    loader_base_address: usize,
    loader_end_address: usize,
) -> ! {
    use core::ffi::CStr;
    let stack_end_address = (cpu::get_stack_pointer() & PAGE_MASK) + PAGE_SIZE_USIZE * 2;
    let stack_base_address = stack_end_address - PAGE_SIZE_USIZE * (LOADER_STACK_PAGES + 1);
    let loader_end_address = (loader_end_address & PAGE_MASK) + PAGE_SIZE_USIZE;

    /* Parse arguments */
    let args = unsafe { core::slice::from_raw_parts(argv, argc) };
    let get_arg = |n: usize| {
        if argc > n {
            unsafe { CStr::from_ptr(args[n]) }
                .to_str()
                .ok()
                .and_then(baremetal::str_to_usize)
        } else {
            None
        }
    };

    let dtb_address = get_arg(1).expect("Invalid arguments: expected dtb_address");
    assert_ne!(dtb_address, 0);

    /* Detect UART */
    let mut uart_address = None;
    if argc >= 3 {
        let address = get_arg(2).expect("Failed to get the UART address");
        assert_ne!(address, 0);
        let wait_offset = get_arg(3).map(|o| o as u32);
        let wait_value = get_arg(4).map(|v| v as u32);

        print::serial_port::init(address, wait_offset, wait_value);
        uart_address = Some(address);
    }

    println!("Boot Loader version {}", env!("CARGO_PKG_VERSION"));
    println!("Loader range:\t[{loader_base_address:#18X} ~ {loader_end_address:#18X}]");
    println!("Stack  range:\t[{stack_base_address:#18X} ~ {stack_end_address:#18X}]");
    arch::target_arch::dump_system();

    let dtb = dtb::DtbManager::new(dtb_address).expect("Failed to get DTB");

    /* Initialize memory allocator */
    let loader_area = [
        (
            loader_base_address,
            loader_end_address - loader_base_address,
        ),
        (stack_base_address, stack_end_address - stack_base_address),
        (
            dtb_address & PAGE_MASK,
            ((dtb.get_total_size() as usize + (dtb_address & !PAGE_MASK) - 1) & PAGE_MASK)
                + PAGE_SIZE_USIZE,
        ),
    ];
    baremetal::memory::init_memory_allocator(&dtb, loader_area.as_slice());
    let mut pm_manager = PhysicalMemoryManager::new();

    /* Set up BootInformation */
    const { assert!(size_of::<BootInformation>() <= PAGE_SIZE_USIZE) };
    let boot_information_address =
        allocate_pages(1).expect("Failed to allocate the memory for the boot information");
    let boot_information = boot_information_address as *mut BootInformation;
    unsafe { boot_information.write(BootInformation::default()) };
    let boot_information = unsafe { boot_information.as_mut().unwrap() };
    boot_information.dtb_address = core::num::NonZeroUsize::new(dtb_address);

    /* Load kernel ELF and map them */
    let elf_address = KERNEL.as_ptr() as usize;
    let read_func = |offset: usize, size: usize, dst: *mut u8| {
        unsafe { core::ptr::copy_nonoverlapping((elf_address + offset) as *const u8, dst, size) };
    };
    println!("Load the kernel...");
    let entry_point = load_kernel(read_func, &mut pm_manager, boot_information);
    println!("Kernel's entry point: {entry_point:#X}");

    /* Set up the page table */
    let page_manager =
        init_paging(&mut pm_manager).expect("Failed to allocate a page for page tables");
    map_kernel(
        &mut pm_manager,
        &page_manager,
        boot_information.elf_program_headers.as_slice(),
    );
    map_direct_area(&mut pm_manager, &page_manager);
    map_loader(&mut pm_manager, &page_manager, loader_area.as_slice());
    if let Some(uart_address) = uart_address {
        map_device(
            &mut pm_manager,
            &page_manager,
            uart_address,
            PAGE_SIZE_USIZE,
        );
    }

    println!("Dump the initial page table for the kernel");
    page_manager.dump_table(None, None);

    /* Allocate the kernel stack */
    let kernel_stack = allocate_pages(KERNEL_STACK_PAGES).expect("Failed to allocate the stack")
        + (KERNEL_STACK_PAGES * PAGE_SIZE_USIZE)
        + memory_layout::get_direct_map_start_address().to_usize();

    /* Store the memory map and freeze the memory allocator */
    baremetal::memory::store_memory_map(&mut boot_information.memory_map);

    /* Set up the system registers if necessary */
    arch::target_arch::setup_environment();

    /* Set the page table and jump to the kernel*/
    println!("Jump to the kernel...");
    cpu::flush_all_cache();
    unsafe { cpu::disable_interrupt() };
    unsafe {
        arch::target_arch::jump_to_kernel(
            entry_point,
            memory_layout::physical_address_to_direct_map_loader(PAddress::new(
                boot_information_address,
            ))
            .to_usize(),
            kernel_stack,
            page_manager,
        )
    }
}

#[cfg(target_os = "uefi")]
#[unsafe(no_mangle)]
extern "efiapi" fn efi_main(
    main_handle: EfiHandle,
    system_table: *const EfiSystemTable,
) -> EfiStatus {
    if system_table.is_null() {
        return EfiStatus::Aborted;
    }
    let system_table = unsafe { &*system_table };
    if !system_table.verify() {
        return EfiStatus::Aborted;
    }
    let boot_services = unsafe { &*system_table.get_boot_services() };
    unsafe {
        efi::BOOT_SERVICES = boot_services;
        efi::MAIN_HANDLE = main_handle;
    }

    let stack_end_address = (cpu::get_stack_pointer() & PAGE_MASK) + PAGE_SIZE_USIZE * 2;
    let stack_base_address = stack_end_address - PAGE_SIZE_USIZE * (LOADER_STACK_PAGES + 1);
    let trampoline_base_address =
        (arch::target_arch::jump_to_kernel as *const () as usize) & PAGE_MASK;
    let loader_area = [
        (trampoline_base_address, PAGE_SIZE_USIZE),
        (stack_base_address, stack_end_address - stack_base_address),
    ];

    /* Set up println */
    let output_protocol = system_table.get_console_output_protocol();
    if !output_protocol.is_null() {
        print::efi_text_output::init(unsafe { &*output_protocol });
    }

    println!("Boot Loader version {}", env!("CARGO_PKG_VERSION"));
    arch::target_arch::dump_system();

    /* Initialize memory allocator */
    let mut pm_manager = PhysicalMemoryManager::new();

    /* Set up BootInformation */
    const { assert!(size_of::<BootInformation>() <= PAGE_SIZE_USIZE) };
    let boot_information_address =
        allocate_pages(1).expect("Failed to allocate the memory for the boot information");
    let boot_information = boot_information_address as *mut BootInformation;
    unsafe { boot_information.write(BootInformation::default()) };
    let boot_information = unsafe { boot_information.as_mut().unwrap() };

    /* Load kernel ELF and map them */
    let file_handler = match efi::open_file(main_handle, boot_services, KERNEL_PATH) {
        Ok(h) => h,
        Err(e) => {
            pr_err!("Failed to open the kernel file: {e:?}");
            return e;
        }
    };

    let read_func =
        |offset: usize, size: usize, dst: *mut u8| efi::read_file(&file_handler, offset, size, dst);
    println!("Load the kernel...");
    let entry_point = load_kernel(read_func, &mut pm_manager, boot_information);
    efi::close_file(file_handler);
    println!("Kernel's entry point: {entry_point:#X}");

    /* Set up the page table */
    let page_manager =
        init_paging(&mut pm_manager).expect("Failed to allocate a page for page tables");
    map_kernel(
        &mut pm_manager,
        &page_manager,
        boot_information.elf_program_headers.as_slice(),
    );
    map_direct_area(&mut pm_manager, &page_manager);
    map_loader(&mut pm_manager, &page_manager, loader_area.as_slice());

    /* Set up graphic */
    if let Some(i) = efi::detect_graphics(boot_services) {
        map_device(
            &mut pm_manager,
            &page_manager,
            i.frame_buffer_address.to_usize(),
            i.frame_buffer_size.get(),
        );
        boot_information.graphic_info = Some(i);
        if let Some(font_info) = efi::load_font_file(main_handle, boot_services) {
            boot_information.font_info = Some(font_info);
        }
    }

    println!("Dump the initial page table for the kernel");
    page_manager.dump_table(None, None);

    /* Allocate the kernel stack */
    let kernel_stack = allocate_pages(KERNEL_STACK_PAGES).expect("Failed to allocate the stack")
        + (KERNEL_STACK_PAGES * PAGE_SIZE_USIZE)
        + memory_layout::get_direct_map_start_address().to_usize();

    /* Store the memory map and freeze the memory allocator */
    let mut memory_map_key = efi::memory::store_memory_map(boot_services, boot_information);

    /* Set up the system registers if necessary */
    arch::target_arch::setup_environment();

    /* Exit Boot Services */
    println!("Exit boot services");
    let mut r = EfiStatus::Success;
    for _ in 0..5 {
        r = (boot_services.exit_boot_services)(main_handle, memory_map_key);
        if r != EfiStatus::Success {
            memory_map_key = efi::memory::store_memory_map(boot_services, boot_information);
        } else {
            break;
        }
    }
    assert_eq!(r, EfiStatus::Success, "Failed to exit boot services: {r:?}");
    boot_information.efi_system_table = Some((&*system_table).clone());

    cpu::flush_all_cache();
    unsafe { cpu::disable_interrupt() };
    unsafe {
        arch::target_arch::jump_to_kernel(
            entry_point,
            memory_layout::physical_address_to_direct_map_loader(PAddress::new(
                boot_information_address,
            ))
            .to_usize(),
            kernel_stack,
            page_manager,
        )
    }
}

fn load_kernel<F>(
    read: F,
    pm_manager: &mut PhysicalMemoryManager,
    boot_information: &mut BootInformation,
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

        if (segment_type != elf::ELF_PROGRAM_HEADER_SEGMENT_LOAD
            && segment_type != elf::ELF_PROGRAM_HEADER_SEGMENT_RELRO)
            || memory_size == 0
        {
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
        println!("Initializing the page table was failed: {:?}", e);
    })?;
    Ok(page_manager)
}

fn map_kernel(
    pm_manager: &mut PhysicalMemoryManager,
    page_manager: &PageManager,
    program_headers: &[elf::Elf64ProgramHeader],
) {
    for entry in program_headers.iter() {
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

#[cfg(target_arch = "aarch64")]
fn map_loader(_: &mut PhysicalMemoryManager, _: &PageManager, _: &[(usize, usize)]) {}

#[cfg(not(target_arch = "aarch64"))]
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

#[cfg(target_arch = "aarch64")]
fn map_device(_: &mut PhysicalMemoryManager, _: &PageManager, _: usize, _: usize) {}

#[cfg(not(target_arch = "aarch64"))]
fn map_device(
    pm_manager: &mut PhysicalMemoryManager,
    page_manager: &PageManager,
    address: usize,
    size: usize,
) {
    assert_ne!(size, 0);
    let aligned_address = address & PAGE_MASK;
    let aligned_size = ((size - 1 + (address - aligned_address)) & PAGE_MASK) + PAGE_SIZE_USIZE;
    page_manager
        .associate_address(
            pm_manager,
            PAddress::new(aligned_address),
            VAddress::new(aligned_address),
            MSize::new(aligned_size),
            MemoryPermissionFlags::new(true, true, false, false),
            MemoryOptionFlags::KERNEL
                | MemoryOptionFlags::IO_MAP
                | MemoryOptionFlags::DEVICE_MEMORY,
        )
        .expect("Failed to map the device");
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
    #[cfg(target_os = "uefi")]
    if !unsafe { efi::BOOT_SERVICES.is_null() } {
        (unsafe { &*efi::BOOT_SERVICES }.exit)(
            unsafe { efi::MAIN_HANDLE },
            EfiStatus::Aborted,
            0,
            core::ptr::null(),
        );
    }
    loop {
        unsafe { cpu::idle() };
    }
}
