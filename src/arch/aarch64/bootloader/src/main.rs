#![no_std]
#![no_main]

#[macro_use]
mod print;
mod boot_information;
mod cpu;
mod efi;
mod elf;
mod guid;
mod paging;

use self::boot_information::*;
use self::efi::{
    EFI_PAGE_MASK, EFI_PAGE_SIZE, EfiBootServices, EfiHandle, EfiStatus,
    EfiStatus::Success,
    EfiSystemTable,
    protocol::{file_protocol::*, graphics_output_protocol::*, loaded_image_protocol::*},
};
use self::elf::{ELF_MACHINE_AA64, ELF_PROGRAM_HEADER_SEGMENT_LOAD, Elf64Header};
use self::paging::*;

use core::arch::asm;
use core::mem::MaybeUninit;
use core::panic;

static mut BOOT_SERVICES: *const EfiBootServices = core::ptr::null();
static mut MAIN_HANDLE: EfiHandle = 0;

const KERNEL_PATH: &str = "\\kernel.elf";
const FONT_PATH: &str = "\\font";

const KERNEL_STACK_PAGES: usize = 64;

static mut BOOT_INFO: MaybeUninit<BootInformation> = MaybeUninit::zeroed();
static mut DIRECT_MAP_START_ADDRESS: usize = 0;
const DIRECT_MAP_END_ADDRESS: usize = 0xffff_ff1f_ffff_ffff;

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
        BOOT_SERVICES = boot_services;
        MAIN_HANDLE = main_handle;
    }

    /* Set up println */
    print::init(system_table.get_console_output_protocol());
    println!("AArch64 Loader version {}", env!("CARGO_PKG_VERSION"));
    dump_system();

    /* Setup BootInformation */
    let boot_info = unsafe { (&raw mut BOOT_INFO).as_mut().unwrap().assume_init_mut() };
    unsafe {
        (&mut boot_info.efi_system_table as *mut EfiSystemTable)
            .copy_from_nonoverlapping(system_table, 1)
    };

    /* Set up page table */
    assert_eq!(EFI_PAGE_SIZE, PAGE_SIZE);
    let top_level_page_table = alloc_pages(1).expect("Failed to allocate a page for page tables");
    init_paging(top_level_page_table);

    let entry_point = load_kernel(main_handle, boot_services, boot_info);

    /* Set up the direct mapping */
    unsafe { DIRECT_MAP_START_ADDRESS = get_direct_map_start_address() };
    associate_address(
        0,
        unsafe { DIRECT_MAP_START_ADDRESS },
        MemoryPermissionFlags::new(true, true, false, false),
        (DIRECT_MAP_END_ADDRESS - unsafe { DIRECT_MAP_START_ADDRESS } + 1) >> PAGE_SHIFT,
        alloc_pages,
    )
    .expect("Failed to setup direct map");
    println!(
        "DIRECT_MAP: VA: [{:#016X}~{:#016X}] => PA: [{:#016X}~{:#016X}]",
        unsafe { DIRECT_MAP_START_ADDRESS },
        DIRECT_MAP_END_ADDRESS,
        0,
        DIRECT_MAP_END_ADDRESS - unsafe { DIRECT_MAP_START_ADDRESS },
    );

    /* Set up the graphic */
    boot_info.graphic_info = detect_graphics(unsafe { &*BOOT_SERVICES });
    if boot_info.graphic_info.is_some() {
        load_font_file(main_handle, unsafe { &*BOOT_SERVICES }, boot_info);
    }

    /* Allocate the kernel stack */
    let kernel_stack = alloc_pages(KERNEL_STACK_PAGES).expect("Failed to allocate the stack")
        + (KERNEL_STACK_PAGES * EFI_PAGE_SIZE);

    /* Store the memory map*/
    let memory_map_address = alloc_pages(1).expect("Failed to allocate memory for memory maps");
    let mut memory_map_key = 0;
    let mut memory_map_size = EFI_PAGE_SIZE;
    let mut descriptor_size = 0;
    let mut descriptor_version = 0;
    let r = (unsafe { &*BOOT_SERVICES }.get_memory_map)(
        &mut memory_map_size,
        memory_map_address,
        &mut memory_map_key,
        &mut descriptor_size,
        &mut descriptor_version,
    );
    assert_eq!(r, Success, "Failed to get memory map: {r:?}");

    /* Set up the BootInfo */
    boot_info.memory_info = MemoryInfo {
        efi_descriptor_version: descriptor_version,
        efi_descriptor_size: descriptor_size,
        efi_memory_map_size: memory_map_size,
        efi_memory_map_address: memory_map_address,
    };
    adjust_boot_info(boot_info);

    /* Exit Boot Services */
    println!("Exit boot services");
    let r = (unsafe { &*system_table.get_boot_services() }.exit_boot_services)(
        main_handle,
        memory_map_key,
    );
    assert_eq!(r, Success, "Failed to exit boot services: {r:?}");

    match cpu::get_current_el() >> 2 {
        1 => {
            set_page_table();
        }
        2 => {
            if (cpu::get_id_aa64mmfr1_el1() & (0b1111 << 8)) != 0 {
                /* FEAT_VHE is supported */
                unsafe {
                    /* Enable FP accesses */
                    cpu::set_cptr_el2(
                        (0b11 << 24) /* SMEN */ | (0b11 << 20) /* FPEN */ | (0b11 << 16), /* ZEN */
                    );
                    /* Disable the paging to enable E2H */
                    cpu::set_sctlr_el2(cpu::get_sctlr_el2() & !1);
                    /* Enable E2H */
                    cpu::set_hcr_el2(
                        (1 << 34) /* E2H */ | (1 << 31) /* RW */ | (1 << 27), /* TGE */
                    );
                }
                /* Set page table and enable it */
                set_page_table();
            } else {
                /* Enable FP accesses */
                unsafe {
                    cpu::set_cptr_el2(
                        (0b11 << 24)/* SMEN */|(0b11 << 20)/* FPEN */ | (0b11 << 16), /* ZEN */
                    )
                };
                /* Set page table and enable it */
                set_page_table();
                unsafe { cpu::jump_to_el1() };
            }
        }
        _ => unreachable!(),
    }

    /* Jump to the kernel */
    cpu::flush_data_cache();
    cpu::flush_instruction_cache();
    unsafe { cpu::cli() };
    unsafe {
        asm!("
        isb
        mov x0, {arg}
        mov sp, {stack}
        br {entry}",
        arg = in(reg) boot_info as *mut _ as usize + DIRECT_MAP_START_ADDRESS,
        stack = in(reg) kernel_stack + DIRECT_MAP_START_ADDRESS,
        entry = in(reg) entry_point,
        options(noreturn))
    }
}

fn dump_system() {
    let el = cpu::get_current_el() >> 2;
    println!("CurrentEL: {el}");
    println!("SCTLR_EL1: {:#X}", cpu::get_sctlr_el1());
    println!("ID_AA64MMFR0_EL1: {:#X}", cpu::get_id_aa64mmfr0_el1());
    println!("ID_AA64MMFR1_EL1: {:#X}", cpu::get_id_aa64mmfr1_el1());
    if el == 2 {
        println!("TCR_EL2: {:#X}", cpu::get_tcr_el2());
        println!("MAIR_EL2: {:#X}", cpu::get_mair_el2());
    } else {
        println!("TCR_EL1: {:#X}", cpu::get_tcr_el1());
        println!("MAIR_EL1: {:#X}", cpu::get_mair_el1());
    }
}

fn adjust_boot_info(boot_info: &mut BootInformation) {
    /* Convert physical address to direct-mapped address */
    fn to_direct_mapped_address(address: &mut usize) {
        let virtual_address = *address + unsafe { DIRECT_MAP_START_ADDRESS };
        assert!(virtual_address <= DIRECT_MAP_END_ADDRESS);
        *address = virtual_address;
    }

    to_direct_mapped_address(&mut boot_info.elf_program_headers_address);
    to_direct_mapped_address(&mut boot_info.memory_info.efi_memory_map_address);
}

fn alloc_pages(num_of_pages: usize) -> Option<usize> {
    let mut address: usize = 0;
    let result = (unsafe { &*BOOT_SERVICES }.allocate_pages)(
        efi::memory_map::EfiAllocateType::AllocateAnyPages,
        efi::memory_map::EfiMemoryType::LoaderData,
        num_of_pages,
        &mut address,
    );
    if result != Success {
        println!("Failed to allocate memory: {result:?}");
        None
    } else {
        Some(address)
    }
}

fn load_kernel(
    main_handle: EfiHandle,
    boot_service: &EfiBootServices,
    boot_info: &mut BootInformation,
) -> usize {
    const ELF_64_HEADER_SIZE: usize = size_of::<Elf64Header>();
    let mut root_directory: *const EfiFileProtocol = core::ptr::null();
    let mut loaded_image_protocol: *const EfiLoadedImageProtocol = core::ptr::null();
    let mut simple_file_protocol: *const EfiSimpleFileProtocol = core::ptr::null();
    let mut file_protocol: *const EfiFileProtocol = core::ptr::null();
    let mut kernel_path: [u16; KERNEL_PATH.len() + 1] = [0; KERNEL_PATH.len() + 1];

    /* Open loaded_image_protocol */
    let r = (boot_service.open_protocol)(
        main_handle,
        &EFI_LOADED_IMAGE_PROTOCOL_GUID,
        &mut loaded_image_protocol as *mut _ as usize,
        main_handle,
        0,
        EFI_OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
    );
    assert_eq!(r, Success, "Failed to open LOADED_IMAGE_PROTOCOL: {r:?}");

    /* Open simple_file_system_protocol */
    let r = (boot_service.open_protocol)(
        unsafe { (*loaded_image_protocol).device_handle },
        &EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID,
        &mut simple_file_protocol as *mut _ as usize,
        main_handle,
        0,
        EFI_OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
    );
    assert_eq!(r, Success, "Failed to open Protocol: {r:?}");
    let simple_file_protocol = unsafe { &*simple_file_protocol };

    /* Open root directory */
    let r = (simple_file_protocol.open_volume)(simple_file_protocol, &mut root_directory);
    assert_eq!(r, Success, "Failed to open the volume: {r:?}");
    let root_directory = unsafe { &*root_directory };

    /* Open the kernel file */
    for (i, e) in kernel_path.iter_mut().zip(KERNEL_PATH.encode_utf16()) {
        *i = e;
    }
    let r = (root_directory.open)(
        root_directory,
        &mut file_protocol,
        kernel_path.as_ptr(),
        EFI_FILE_MODE_READ,
        0,
    );
    assert_eq!(r, Success, "Failed to open \"{KERNEL_PATH}\": {r:?}");
    let file_protocol = unsafe { &*file_protocol };

    /* Read ELF Header */
    let mut read_size = ELF_64_HEADER_SIZE;
    let r = (file_protocol.read)(
        file_protocol,
        &mut read_size,
        boot_info.elf_header_buffer.as_mut_ptr(),
    );
    assert_eq!(r, Success, "Failed to read the ELF header: {r:?}");
    assert_eq!(ELF_64_HEADER_SIZE, read_size);
    cpu::flush_data_cache();
    let elf_header =
        unsafe { Elf64Header::from_ptr(&boot_info.elf_header_buffer) }.expect("Invalid ELF file");
    assert!(
        elf_header.is_executable_file() && elf_header.get_machine_type() == ELF_MACHINE_AA64,
        "ELF file is not for this computer."
    );

    /* Read ELF Program Header */
    let mut elf_program_headers_size = elf_header.get_program_headers_array_size() as usize;
    assert_ne!(elf_program_headers_size, 0, "Invalid ELF file");

    boot_info.elf_program_headers_address =
        alloc_pages(((elf_program_headers_size - 1) & EFI_PAGE_MASK) / EFI_PAGE_SIZE + 1)
            .expect("Failed to allocate memory for ELF Program Header");
    let r = (file_protocol.set_position)(file_protocol, elf_header.get_program_headers_offset());
    assert_eq!(r, Success, "Failed to seek: {r:?}");
    let r = (file_protocol.read)(
        file_protocol,
        &mut elf_program_headers_size,
        boot_info.elf_program_headers_address as *mut u8,
    );
    assert_eq!(r, Success, "Failed to read the ELF program headers: {r:?}");
    assert_eq!(
        elf_program_headers_size,
        elf_header.get_program_headers_array_size() as usize
    );
    cpu::flush_data_cache();

    /* Load and map segments */
    for entry in elf_header.get_program_header_iter_mut(unsafe {
        core::slice::from_raw_parts_mut(
            boot_info.elf_program_headers_address as *mut u8,
            elf_program_headers_size,
        )
    }) {
        let segment_type = entry.get_segment_type();
        let virtual_address = entry.get_virtual_address() as usize;
        let memory_size = entry.get_memory_size() as usize;
        let file_size = entry.get_file_size() as usize;
        let file_offset = entry.get_file_offset() as usize;
        let alignment = entry.get_align().max(1) as usize;

        if segment_type != ELF_PROGRAM_HEADER_SEGMENT_LOAD || memory_size == 0 {
            continue;
        }
        assert!(virtual_address >= TTBR1_EL1_START_ADDRESS);
        assert_eq!(virtual_address & !EFI_PAGE_MASK, 0);

        let aligned_memory_size = ((memory_size - 1) & EFI_PAGE_MASK) + EFI_PAGE_SIZE;
        let num_of_pages = aligned_memory_size / EFI_PAGE_SIZE;
        let physical_address =
            alloc_pages(num_of_pages).expect("Failed to allocate memory for the kernel");

        if file_size > 0 {
            let r = (file_protocol.set_position)(file_protocol, entry.get_file_offset());
            assert_eq!(r, Success, "Failed to seek: {r:?}");
            let mut read_size = file_size;
            let r =
                (file_protocol.read)(file_protocol, &mut read_size, physical_address as *mut u8);
            assert_eq!(r, Success, "Failed to read the kernel: {r:?}");
            assert_eq!(file_size, read_size);
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
        cpu::flush_data_cache();

        assert_eq!(EFI_PAGE_SIZE, PAGE_SIZE);
        associate_address(
            physical_address,
            virtual_address,
            MemoryPermissionFlags::new(
                entry.is_segment_readable(),
                entry.is_segment_writable(),
                entry.is_segment_executable(),
                false,
            ),
            num_of_pages,
            alloc_pages,
        )
        .expect("Failed to map kernel");

        println!(
            "PA: {physical_address:#016X}, VA: {virtual_address:#016X}, MS: {memory_size:#04X},\
             FS: {file_size:#04X}, FO: {file_offset:#04X}, AL: {alignment:#04X}, R: {}, W: {}, E: {}",
            entry.is_segment_readable(),
            entry.is_segment_writable(),
            entry.is_segment_executable()
        );
    }

    let entry_point = elf_header.get_entry_point() as usize;
    println!("Entry Point: {entry_point:#016X}");

    /* Close handlers */
    let _ = (file_protocol.close)(file_protocol);
    let _ = (root_directory.close)(root_directory);

    entry_point
}

fn load_font_file(
    main_handle: EfiHandle,
    boot_service: &EfiBootServices,
    boot_info: &mut BootInformation,
) {
    /* Open root directory */
    let mut root_directory: *const EfiFileProtocol = core::ptr::null();
    let mut loaded_image_protocol: *const EfiLoadedImageProtocol = core::ptr::null();
    let mut simple_file_protocol: *const EfiSimpleFileProtocol = core::ptr::null();
    let mut file_protocol: *const EfiFileProtocol = core::ptr::null();
    let mut font_path: [u16; FONT_PATH.len() + 1] = [0; FONT_PATH.len() + 1];

    boot_info.font_address = None;

    /* Open loaded_image_protocol */
    let r = (boot_service.open_protocol)(
        main_handle,
        &EFI_LOADED_IMAGE_PROTOCOL_GUID,
        &mut loaded_image_protocol as *mut _ as usize,
        main_handle,
        0,
        EFI_OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
    );
    assert_eq!(r, Success, "Failed to open the protocol: {r:?}");

    /* Open simple_file_system_protocol */
    let r = (boot_service.open_protocol)(
        unsafe { (*loaded_image_protocol).device_handle },
        &EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID,
        &mut simple_file_protocol as *mut _ as usize,
        main_handle,
        0,
        EFI_OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
    );
    assert_eq!(r, Success, "Failed to open the protocol: {r:?}");
    let simple_file_protocol = unsafe { &*simple_file_protocol };

    /* Open root directory */
    let r = (simple_file_protocol.open_volume)(simple_file_protocol, &mut root_directory);
    assert_eq!(r, Success, "Failed to open the volume: {r:?}");
    let root_directory = unsafe { &*root_directory };

    /* Open the font file */
    for (i, e) in font_path.iter_mut().zip(FONT_PATH.encode_utf16()) {
        *i = e;
    }

    let r = (root_directory.open)(
        root_directory,
        &mut file_protocol,
        font_path.as_ptr(),
        EFI_FILE_MODE_READ,
        0,
    );
    if r != Success {
        println!("Failed to open \"{FONT_PATH}\": {r:?}");
        (root_directory.close)(root_directory);
        return;
    };
    let file_protocol = unsafe { &*file_protocol };

    /* Get the file size */
    let r = (file_protocol.set_position)(file_protocol, u64::MAX);
    assert_eq!(r, Success, "Failed to seek \"{FONT_PATH}\": {r:?}");
    let mut file_size: u64 = 0;
    let r = (file_protocol.get_position)(file_protocol, &mut file_size);
    assert_eq!(r, Success, "Failed to seek \"{FONT_PATH}\": {r:?}");
    if file_size == 0 {
        println!("Invalid file size");
        (file_protocol.close)(file_protocol);
        (root_directory.close)(root_directory);
        return;
    }

    /* Load the font file */
    let allocated_memory =
        alloc_pages((((file_size as usize - 1) & EFI_PAGE_MASK) / EFI_PAGE_SIZE) + 1)
            .expect("Failed to allocate memory for the font");
    let mut read_size = file_size as usize;
    let _ = (file_protocol.set_position)(file_protocol, 0);
    let r = (file_protocol.read)(file_protocol, &mut read_size, allocated_memory as *mut u8);
    assert_eq!(r, Success, "Failed to read \"{FONT_PATH}\": {r:?}");
    assert_eq!(read_size, file_size as usize);
    cpu::flush_data_cache();
    println!(
        "Loaded Font File(File Size: {:#X}, Location: {:#X})",
        file_size, allocated_memory
    );
    boot_info.font_address = Some((allocated_memory, file_size as usize));

    let _ = (file_protocol.close)(file_protocol);
    let _ = (root_directory.close)(root_directory);
}

fn detect_graphics(boot_service: &EfiBootServices) -> Option<GraphicInfo> {
    let mut graphics_output_protocol: *const EfiGraphicsOutputProtocol = core::ptr::null();

    let r = (boot_service.locate_protocol)(
        &EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID,
        0,
        &mut graphics_output_protocol as *mut _ as usize,
    );
    if r != Success {
        println!("Failed to open EfiGraphicsOutputProtocol: {r:?}");
        return None;
    }

    let graphics_output_protocol = unsafe { &*graphics_output_protocol };
    let mode = unsafe { &*graphics_output_protocol.mode };

    Some(GraphicInfo {
        frame_buffer_base: mode.frame_buffer_base,
        frame_buffer_size: mode.frame_buffer_size,
        info: unsafe { &*mode.info }.clone(),
    })
}

#[panic_handler]
pub fn panic(p: &panic::PanicInfo) -> ! {
    println!("{p}");
    if !unsafe { BOOT_SERVICES.is_null() } {
        (unsafe { &*BOOT_SERVICES }.exit)(
            unsafe { MAIN_HANDLE },
            EfiStatus::Aborted,
            0,
            core::ptr::null(),
        );
    }
    loop {
        unsafe { asm!("wfi") };
    }
}
