#![no_std]
#![no_main]
#![feature(const_maybe_uninit_uninit_array)]
#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_uninit_array)]

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
    protocol::{file_protocol::*, graphics_output_protocol::*, loaded_image_protocol::*},
    EfiBootServices, EfiHandle, EfiSystemTable, EFI_PAGE_MASK, EFI_PAGE_SIZE, EFI_SUCCESS,
};
use self::elf::{Elf64Header, ELF_MACHINE_AA64, ELF_PROGRAM_HEADER_SEGMENT_LOAD};
use self::paging::*;

use core::arch::asm;
use core::mem::MaybeUninit;
use core::panic;

static mut BOOT_SERVICES: *const EfiBootServices = core::ptr::null();

const KERNEL_PATH: &str = "\\EFI\\BOOT\\kernel.elf";
const FONT_PATH: &str = "\\EFI\\BOOT\\font";

const KERNEL_STACK_PAGES: usize = 64;

static mut DIRECT_MAP_START_ADDRESS: usize = 0;
const DIRECT_MAP_END_ADDRESS: usize = 0xffff_ff1f_ffff_ffff;

#[no_mangle]
extern "efiapi" fn efi_main(main_handle: EfiHandle, system_table: *const EfiSystemTable) {
    assert!(!system_table.is_null());
    let system_table = unsafe { &*system_table };
    if !system_table.verify() {
        panic!("Failed to verify the EFI System Table.");
    }
    unsafe { BOOT_SERVICES = system_table.get_boot_services() };

    /* Set up println */
    print::init(system_table.get_console_output_protocol());
    println!("AArch64 Boot Loader version {}", env!("CARGO_PKG_VERSION"));
    dump_system();

    /* Setup BootInformation */
    const { assert!(core::mem::size_of::<BootInformation>() <= EFI_PAGE_SIZE) };
    let boot_info = alloc_pages(1).expect("Failed to allocate a page for BootInformation");
    unsafe {
        core::ptr::write_bytes(
            boot_info as *mut u8,
            0,
            core::mem::size_of::<BootInformation>(),
        )
    };
    let boot_info = unsafe { &mut *(boot_info as *mut BootInformation) };
    boot_info.efi_system_table = unsafe { core::mem::transmute_copy(system_table) };

    /* Set up page table */
    assert_eq!(EFI_PAGE_SIZE, PAGE_SIZE);
    let top_level_page_table = alloc_pages(1).expect("Failed to allocate a page for page tables");
    init_paging(top_level_page_table);

    let entry_point = load_kernel(main_handle, unsafe { &*BOOT_SERVICES }, boot_info);

    /* Set up the direct mapping */
    unsafe { DIRECT_MAP_START_ADDRESS = get_direct_map_start_address() };
    associate_direct_map_address(
        0,
        unsafe { DIRECT_MAP_START_ADDRESS },
        MemoryPermissionFlags::data(),
        DIRECT_MAP_END_ADDRESS - unsafe { DIRECT_MAP_START_ADDRESS } + 1,
        alloc_pages,
    )
    .expect("Failed to setup direct map");
    println!(
        "DIRECT_MAP_START_ADDRESS: {:#X} ~ {:#X}(Physical Address: {:#X} ~ {:#X})",
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
    if r != EFI_SUCCESS {
        panic!("Failed to get memory map: {:#X}", r);
    }

    boot_info.memory_info = MemoryInfo {
        efi_descriptor_version: descriptor_version,
        efi_descriptor_size: descriptor_size,
        efi_memory_map_size: memory_map_size,
        efi_memory_map_address: memory_map_address,
    };

    adjust_boot_info(boot_info);

    if unsafe { cpu::get_current_el() >> 2 } == 2 {
        if (unsafe { cpu::get_id_aa64mmfr1_el1() } & (0b1111 << 8)) != (1 << 8) {
            unsafe { cpu::jump_to_el1() };
        } else {
            /* Enable E2H*/
            println!("TODO: Enable E2H");
            /* Enabling E2H hangs up... */
            /*unsafe {
                cpu::set_hcr_el2(
                    cpu::get_hcr_el2()
                        | (1 << 34)
                        | (1 << 31)
                        | (1 << 27)
                        | (1 << 5)
                        | (1 << 4)
                        | (1 << 3),
                )
            };*/

            unsafe { cpu::jump_to_el1() };
        }
    }

    println!("Exit boot services");

    /* Exit Boot Service and map kernel */
    let r = (unsafe { &*system_table.get_boot_services() }.exit_boot_services)(
        main_handle,
        memory_map_key,
    );
    if r != EFI_SUCCESS {
        panic!("Failed to exit boot service");
    }

    /* Jump to the kernel */
    cpu::flush_data_cache();
    apply_paging_settings();
    cpu::flush_instruction_cache();
    unsafe { cpu::cli() };

    unsafe {
        asm!("
        mov x0, {arg}
        mov sp, {stack}
        br {entry}",
        arg = in(reg) boot_info as *mut _ as usize + DIRECT_MAP_START_ADDRESS,
        stack = in(reg) kernel_stack + DIRECT_MAP_START_ADDRESS,
        entry = in(reg) entry_point,
        options(noreturn))
    };
}

fn dump_system() {
    let el = unsafe { cpu::get_current_el() >> 2 };
    println!("CurrentEL: {el}");
    println!("SCTLR_EL1: {:#X}", unsafe { cpu::get_sctlr_el1() });
    println!("ID_AA64MMFR0_EL1: {:#X}", unsafe {
        cpu::get_id_aa64mmfr0_el1()
    });
    println!("ID_AA64MMFR1_EL1: {:#X}", unsafe {
        cpu::get_id_aa64mmfr1_el1()
    });
    if el == 2 {
        println!("TCR_EL2: {:#X}", unsafe { cpu::get_tcr_el2() });
    } else {
        println!("TCR_EL1: {:#X}", unsafe { cpu::get_tcr_el1() });
    }
}

fn adjust_boot_info(boot_info: &mut BootInformation) {
    /* Convert physical address to direct mapped address */
    fn to_direct_mapped_address(address: &mut usize) {
        let virtual_address = *address + unsafe { DIRECT_MAP_START_ADDRESS };
        assert!(virtual_address <= DIRECT_MAP_END_ADDRESS);
        *address = virtual_address;
    }

    to_direct_mapped_address(&mut boot_info.elf_program_header_address);
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
    if result != EFI_SUCCESS {
        println!("Failed to allocate memory: {:#X}", result);
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
    const ELF_64_HEADER_SIZE: usize = core::mem::size_of::<Elf64Header>();
    let mut root_directory: *const EfiFileProtocol = core::ptr::null();
    let mut loaded_image_protocol: *const EfiLoadedImageProtocol = core::ptr::null();
    let mut simple_file_protocol: *const EfiSimpleFileProtocol = core::ptr::null();
    let mut file_protocol: *const EfiFileProtocol = core::ptr::null();
    let mut kernel_path: [MaybeUninit<u16>; KERNEL_PATH.len() + 1] = MaybeUninit::uninit_array();

    /* Open loaded_image_protocol */
    let r = (boot_service.open_protocol)(
        main_handle,
        &EFI_LOADED_IMAGE_PROTOCOL_GUID,
        &mut loaded_image_protocol as *mut _ as usize,
        main_handle,
        0,
        EFI_OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
    );
    if r != EFI_SUCCESS {
        panic!("Failed to open LOADED_IMAGE_PROTOCOL: {:#X}", r);
    }

    /* Open simple_file_system_protocol */
    let r = (boot_service.open_protocol)(
        unsafe { (*loaded_image_protocol).device_handle },
        &EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID,
        &mut simple_file_protocol as *mut _ as usize,
        main_handle,
        0,
        EFI_OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
    );
    if r != EFI_SUCCESS {
        panic!(
            "Failed to open EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID: {:#X}",
            r
        );
    }
    let simple_file_protocol = unsafe { &*simple_file_protocol };

    /* Open root directory */
    let r = (simple_file_protocol.open_volume)(simple_file_protocol, &mut root_directory);
    if r != EFI_SUCCESS {
        panic!("Failed to open the volume: {:#X}", r);
    };
    let root_directory = unsafe { &*root_directory };

    /* Open the kernel file */
    for (i, e) in KERNEL_PATH.encode_utf16().enumerate() {
        kernel_path[i].write(e);
    }
    kernel_path[KERNEL_PATH.len()].write(0);
    let r = (root_directory.open)(
        root_directory,
        &mut file_protocol,
        unsafe { MaybeUninit::array_assume_init(kernel_path) }.as_ptr(),
        EFI_FILE_MODE_READ,
        0,
    );
    if r != EFI_SUCCESS {
        panic!("Failed to open \"{}\": {:#X}", KERNEL_PATH, r);
    };
    let file_protocol = unsafe { &*file_protocol };

    /* Read ELF Header */
    let mut read_size = ELF_64_HEADER_SIZE;
    let r = (file_protocol.read)(
        file_protocol,
        &mut read_size,
        boot_info.elf_header_buffer.as_mut_ptr(),
    );
    if r != EFI_SUCCESS || read_size != ELF_64_HEADER_SIZE {
        panic!(
            "Failed to read ELF Header (Read Size: {:#X}, expected: {:#X}, EfiStatus: {:#X})",
            read_size, ELF_64_HEADER_SIZE, r
        );
    }
    cpu::flush_data_cache();
    let elf_header =
        unsafe { Elf64Header::from_ptr(&boot_info.elf_header_buffer) }.expect("Invalid ELF file");
    if !elf_header.is_executable_file() || elf_header.get_machine_type() != ELF_MACHINE_AA64 {
        panic!("ELF file is not for this computer.");
    }

    /* Read ELF Program Header */
    let mut elf_program_headers_size = elf_header.get_program_header_array_size() as usize;
    if elf_program_headers_size == 0 {
        panic!("Invalid ELF file");
    }
    boot_info.elf_program_header_address =
        alloc_pages(((elf_program_headers_size - 1) & EFI_PAGE_MASK) / EFI_PAGE_SIZE + 1)
            .expect("Failed to allocate memory for ELF Program Header");
    let r = (file_protocol.set_position)(file_protocol, elf_header.get_program_header_offset());
    if r != EFI_SUCCESS {
        panic!("Failed to seek: {:#X}", r);
    }
    let r = (file_protocol.read)(
        file_protocol,
        &mut elf_program_headers_size,
        boot_info.elf_program_header_address as *mut u8,
    );
    if r != EFI_SUCCESS
        || elf_program_headers_size != elf_header.get_program_header_array_size() as usize
    {
        panic!("Failed to read Program Header");
    }
    cpu::flush_data_cache();

    /* Load and map segments */
    for entry in elf_header.get_program_header_iter_mut(unsafe {
        core::slice::from_raw_parts_mut(
            boot_info.elf_program_header_address as *mut u8,
            elf_program_headers_size,
        )
    }) {
        if entry.get_segment_type() != ELF_PROGRAM_HEADER_SEGMENT_LOAD {
            println!(
                "Segment({{ PA: {:#X}, VA: {:#X}}}) will not be loaded.",
                entry.get_physical_address(),
                entry.get_virtual_address()
            );
            continue;
        }
        assert!(entry.get_virtual_address() as usize >= TTBR1_EL1_START_ADDRESS);

        let alignment = entry.get_align().max(1);
        let align_offset = (entry.get_virtual_address() & (alignment - 1)) as usize;
        if ((entry.get_virtual_address() as usize) & !EFI_PAGE_MASK) != 0 {
            panic!("Invalid Alignment: {:#X}", alignment);
        } else if entry.get_memory_size() == 0 {
            continue;
        }
        let aligned_memory_size =
            ((entry.get_memory_size() as usize + align_offset - 1) & EFI_PAGE_MASK) + EFI_PAGE_SIZE;
        let num_of_pages = aligned_memory_size / EFI_PAGE_SIZE;
        let allocated_memory =
            alloc_pages(num_of_pages).expect("Failed to allocate memory for the kernel");
        if entry.get_file_size() > 0 {
            let r = (file_protocol.set_position)(file_protocol, entry.get_file_offset());
            if r != EFI_SUCCESS {
                panic!("Failed to seek for Kernel");
            }
            let mut read_size = entry.get_file_size() as usize;
            let r = (file_protocol.read)(
                file_protocol,
                &mut read_size,
                (allocated_memory + align_offset) as *mut u8,
            );
            if r != EFI_SUCCESS || read_size != entry.get_file_size() as usize {
                panic!("Failed to read Kernel");
            }
        }
        if entry.get_memory_size() > entry.get_file_size() {
            unsafe {
                core::ptr::write_bytes(
                    (allocated_memory + align_offset + read_size) as *mut u8,
                    0,
                    (entry.get_memory_size() - entry.get_file_size()) as usize,
                )
            };
        }
        entry.set_physical_address((allocated_memory + align_offset) as u64);
        cpu::flush_data_cache();

        assert_eq!(EFI_PAGE_SIZE, PAGE_SIZE);
        associate_address(
            entry.get_physical_address() as usize,
            entry.get_virtual_address() as usize,
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
            "PA: {:#X}, VA: {:#X}, MS: {:#X}, FS: {:#X}, FO: {:#X}, AL: {:#X}, R:{}, W: {}, E:{}",
            entry.get_physical_address(),
            entry.get_virtual_address(),
            entry.get_memory_size(),
            entry.get_file_size(),
            entry.get_file_offset(),
            entry.get_align(),
            entry.is_segment_readable(),
            entry.is_segment_writable(),
            entry.is_segment_executable()
        );
    }

    println!("Entry Point: {:#X}", elf_header.get_entry_point());

    /* Close handlers */
    let _ = (file_protocol.close)(file_protocol);
    let _ = (root_directory.close)(root_directory);

    elf_header.get_entry_point() as usize
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
    let mut font_path: [MaybeUninit<u16>; FONT_PATH.len() + 1] = MaybeUninit::uninit_array();

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
    if r != EFI_SUCCESS {
        println!("Failed to open LOADED_IMAGE_PROTOCOL: {:#X}", r);
        return;
    }

    /* Open simple_file_system_protocol */
    let r = (boot_service.open_protocol)(
        unsafe { (*loaded_image_protocol).device_handle },
        &EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID,
        &mut simple_file_protocol as *mut _ as usize,
        main_handle,
        0,
        EFI_OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
    );
    if r != EFI_SUCCESS {
        println!(
            "Failed to open EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID: {:#X}",
            r
        );
        return;
    }
    let simple_file_protocol = unsafe { &*simple_file_protocol };

    /* Open root directory */
    let r = (simple_file_protocol.open_volume)(simple_file_protocol, &mut root_directory);
    if r != EFI_SUCCESS {
        panic!("Failed to open the volume: {:#X}", r);
    };
    let root_directory = unsafe { &*root_directory };

    /* Open the font file */
    for (i, e) in FONT_PATH.encode_utf16().enumerate() {
        font_path[i].write(e);
    }
    font_path[FONT_PATH.len()].write(0);

    let r = (root_directory.open)(
        root_directory,
        &mut file_protocol,
        unsafe { MaybeUninit::array_assume_init(font_path) }.as_ptr(),
        EFI_FILE_MODE_READ,
        0,
    );
    if r != EFI_SUCCESS {
        println!("Failed to open \"{}\": {:#X}", FONT_PATH, r);
        (root_directory.close)(root_directory);
        return;
    };
    let file_protocol = unsafe { &*file_protocol };

    /* Get the file size */
    let r = (file_protocol.set_position)(file_protocol, u64::MAX);
    if r != EFI_SUCCESS {
        panic!("Failed to seek \"{}\": {:#X}", FONT_PATH, r);
    };
    let mut file_size: u64 = 0;
    let r = (file_protocol.get_position)(file_protocol, &mut file_size);
    if r != EFI_SUCCESS {
        panic!("Failed to seek \"{}\": {:#X}", FONT_PATH, r);
    };
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
    if r != EFI_SUCCESS || read_size != file_size as usize {
        println!(
            "Failed to read Font size(Read Size: {:#X}, expected: {:#X}, EfiStatus: {:#X})",
            read_size, file_size, r
        );
    } else {
        cpu::flush_data_cache();
        println!(
            "Loaded Font File(File Size: {:#X}, Location: {:#X})",
            file_size, allocated_memory
        );
        boot_info.font_address = Some((allocated_memory, file_size as usize));
    }
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
    if r != EFI_SUCCESS {
        println!("Failed to open EfiGraphicsOutputProtocol: {:#X}", r);
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
#[no_mangle]
pub fn panic(p: &panic::PanicInfo) -> ! {
    println!("{p}");
    loop {
        unsafe { asm!("wfi") };
    }
}
