#![no_std]
#![no_main]
#![feature(asm_sym)]
#![feature(abi_efiapi)]
#![feature(format_args_nl)]
#![feature(naked_functions)]
#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_uninit_array)]
#![feature(panic_info_message)]

#[macro_use]
mod print;
mod cpu;
mod efi;
mod elf;
mod guid;
mod paging;

use self::cpu::get_current_el;
use self::efi::memory_map::{EfiAllocateType, EfiMemoryType};
use self::efi::protocol::file_protocol::{
    EfiFileProtocol, EfiSimpleFileProtocol, EFI_FILE_MODE_READ,
    EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID,
};
use self::efi::protocol::graphics_output_protocol::{
    EfiGraphicsOutputModeInformation, EfiGraphicsOutputProtocol, EfiGraphicsOutputProtocolMode,
    EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID,
};
use self::efi::protocol::loaded_image_protocol::{
    EfiLoadedImageProtocol, EFI_LOADED_IMAGE_PROTOCOL_GUID, EFI_OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
};
use self::efi::{EfiBootServices, EfiHandle, EfiSystemTable, EFI_SUCCESS};
use self::efi::{EFI_PAGE_MASK, EFI_PAGE_SIZE};
use self::elf::{Elf64Header, ELF_MACHINE_AA64, ELF_PROGRAM_HEADER_SEGMENT_LOAD};
use self::paging::{
    associate_address, associate_direct_map_address, estimate_num_of_pages_to_direct_map,
    init_ttbr1, MemoryPermissionFlags, NUM_OF_ENTRIES_IN_PAGE_TABLE, PAGE_MASK, PAGE_SIZE,
    TTBR1_EL1_START_ADDRESS,
};

use core::arch::asm;
use core::mem::MaybeUninit;
use core::panic;

static mut SYSTEM_TABLE: *const EfiSystemTable = core::ptr::null();
static mut PAGE_TABLES_BASE_ADDRESS: usize = 0;
static mut NUM_OF_PAGE_TABLES: usize = 0;
static mut BOOT_INFO: MaybeUninit<BootInformation> = MaybeUninit::uninit();

const KERNEL_PATH: &str = "\\EFI\\BOOT\\kernel.elf";

const DIRECT_MAP_START_ADDRESS: usize = 0xffff_0000_0000_0000;
const DIRECT_MAP_END_ADDRESS: usize = 0xffff_7fff_ffff_ffff;
const DIRECT_MAP_BASE_ADDRESS: usize = 0;

const ELF_64_HEADER_SIZE: usize = core::mem::size_of::<Elf64Header>();

struct BootInformation {
    elf_header_buffer: [u8; ELF_64_HEADER_SIZE],
    elf_program_header_address: usize,
    efi_system_table: EfiSystemTable,
    graphic_info: Option<GraphicInfo>,
    memory_info: MemoryInfo,
}

struct MemoryInfo {
    #[allow(dead_code)]
    efi_descriptor_version: u32,
    #[allow(dead_code)]
    efi_descriptor_size: usize,
    #[allow(dead_code)]
    efi_memory_map_size: usize,
    #[allow(dead_code)]
    efi_memory_map_address: usize,
}

struct GraphicInfo {
    #[allow(dead_code)]
    pub frame_buffer_base: usize,
    #[allow(dead_code)]
    pub frame_buffer_size: usize,
    #[allow(dead_code)]
    pub info: EfiGraphicsOutputModeInformation,
}

#[no_mangle]
extern "efiapi" fn efi_main(main_handle: EfiHandle, system_table: *const EfiSystemTable) {
    assert!(!system_table.is_null());
    let system_table = unsafe { &*system_table };
    unsafe { SYSTEM_TABLE = system_table };
    if !system_table.verify() {
        panic!("Failed to verify the EFI System Table.");
    }

    println!("Boot loader for AArch64");

    let mut boot_info: BootInformation = unsafe { MaybeUninit::zeroed().assume_init() };
    unsafe {
        core::mem::forget(core::mem::replace(
            &mut boot_info.efi_system_table,
            core::mem::transmute_copy(system_table),
        ))
    };
    let mut num_of_needed_page_tables = 1
        + 3
        + estimate_num_of_pages_to_direct_map(
            DIRECT_MAP_END_ADDRESS - DIRECT_MAP_START_ADDRESS + 1,
        );
    load_elf_binary(
        main_handle,
        unsafe { &*system_table.get_boot_services() },
        &mut boot_info,
        &mut num_of_needed_page_tables,
    );
    boot_info.graphic_info = detect_graphics(unsafe { &*system_table.get_boot_services() });

    println!(
        "Number of estimated pages for the page tables: {:#X}",
        num_of_needed_page_tables
    );

    let mut page_table_address = 0;
    let r = (unsafe { &*system_table.get_boot_services() }.allocate_pages)(
        EfiAllocateType::AllocateAnyPages,
        EfiMemoryType::EfiLoaderData,
        num_of_needed_page_tables,
        &mut page_table_address,
    );
    if r != EFI_SUCCESS {
        panic!("Failed to allocate memory for page tables");
    }

    unsafe {
        NUM_OF_PAGE_TABLES = num_of_needed_page_tables;
        PAGE_TABLES_BASE_ADDRESS = page_table_address;
    }

    let mut memory_map_address = 0;
    let r = (unsafe { &*system_table.get_boot_services() }.allocate_pages)(
        EfiAllocateType::AllocateAnyPages,
        EfiMemoryType::EfiLoaderData,
        1,
        &mut memory_map_address,
    );
    if r != EFI_SUCCESS {
        panic!("Failed to allocate memory for memory maps");
    }
    let mut memory_map_key = 0;
    let mut memory_map_size = EFI_PAGE_SIZE;
    let mut descriptor_size = 0;
    let mut descriptor_version = 0;
    let r = (unsafe { &*system_table.get_boot_services() }.get_memory_map)(
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

    unsafe { BOOT_INFO.write(boot_info) };

    /* Exit Boot Service and map kernel */
    let r = (unsafe { &*system_table.get_boot_services() }.exit_boot_services)(
        main_handle,
        memory_map_key,
    );
    if r != EFI_SUCCESS {
        panic!("Failed to exit boot service");
    }
    unsafe { SYSTEM_TABLE = core::ptr::null() };
    unsafe { cpu::cli() };

    /* Down to EL1 if currentEL is EL2 */
    if unsafe { get_current_el() >> 2 } == 2 {
        down_to_el1();
        /* Never comes here */
    }
    boot_latter_half();
}

fn boot_latter_half() -> ! {
    let boot_info = unsafe { BOOT_INFO.assume_init_ref() };
    init_ttbr1(alloc_table().unwrap());

    let elf_header = unsafe { Elf64Header::from_ptr(&boot_info.elf_header_buffer) }.unwrap();
    if !elf_header.is_executable_file() {
        panic!("Non executable file!");
    }
    for entry in elf_header.get_program_header_iter(boot_info.elf_program_header_address) {
        if entry.get_segment_type() == ELF_PROGRAM_HEADER_SEGMENT_LOAD {
            let alignment = entry.get_align().max(1);
            let num_of_pages = ((entry.get_memory_size() as usize
                + (entry.get_virtual_address() & (alignment - 1)) as usize
                - 1)
                & PAGE_MASK)
                / PAGE_SIZE
                + 1;
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
                alloc_table,
            )
            .expect("Failed to map kernel");
        }
    }

    associate_direct_map_address(
        DIRECT_MAP_BASE_ADDRESS,
        DIRECT_MAP_START_ADDRESS,
        MemoryPermissionFlags::data(),
        DIRECT_MAP_END_ADDRESS - DIRECT_MAP_START_ADDRESS + 1,
        alloc_table,
    )
    .expect("Failed to setup direct map");
    /* TODO: pass remained pages to kernel */
    unsafe {
        (core::mem::transmute::<u64, extern "C" fn(*const BootInformation)>(
            elf_header.get_entry_point(),
        ))(BOOT_INFO.as_ptr())
    };

    unreachable!()
}

fn alloc_table() -> Option<usize> {
    if unsafe { NUM_OF_PAGE_TABLES } == 0 {
        None
    } else {
        unsafe { NUM_OF_PAGE_TABLES -= 1 };
        let address = unsafe { PAGE_TABLES_BASE_ADDRESS };
        unsafe { PAGE_TABLES_BASE_ADDRESS += PAGE_SIZE };
        Some(address)
    }
}

fn load_elf_binary(
    main_handle: EfiHandle,
    boot_service: &EfiBootServices,
    boot_info: &mut BootInformation,
    estimated_needed_page_table: &mut usize,
) {
    /* Open root directory */
    let mut root_directory: *const EfiFileProtocol = core::ptr::null();
    let mut loaded_image_protocol: *const EfiLoadedImageProtocol = core::ptr::null();
    let mut simple_file_protocol: *const EfiSimpleFileProtocol = core::ptr::null();

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
    let r = (simple_file_protocol.open_volume)(simple_file_protocol, &mut root_directory);
    if r != EFI_SUCCESS {
        panic!("Failed to open the volume: {:#X}", r);
    };
    let root_directory = unsafe { &*root_directory };
    let mut file_protocol: *const EfiFileProtocol = core::ptr::null();
    let mut kernel_path: [MaybeUninit<u16>; KERNEL_PATH.len() + 1] = MaybeUninit::uninit_array();

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
    let elf_header =
        unsafe { Elf64Header::from_ptr(&boot_info.elf_header_buffer) }.expect("Invalid ELF file");
    if elf_header.get_machine_type() != ELF_MACHINE_AA64 {
        panic!("ELF file is not for this computer.");
    }

    let mut elf_program_headers_size = elf_header.get_program_header_array_size() as usize;
    if elf_program_headers_size == 0 {
        panic!("Invalid ELF file");
    }
    println!("Entry Point: {:#X}", elf_header.get_entry_point());
    let r = (boot_service.allocate_pages)(
        EfiAllocateType::AllocateAnyPages,
        EfiMemoryType::EfiLoaderData,
        ((elf_program_headers_size - 1) & EFI_PAGE_MASK) / EFI_PAGE_SIZE + 1,
        &mut boot_info.elf_program_header_address,
    );
    if r != EFI_SUCCESS {
        panic!("Failed to allocate memory for ELF Program Header");
    }
    let r = (file_protocol.set_position)(file_protocol, elf_header.get_program_header_offset());
    if r != EFI_SUCCESS {
        panic!("Failed to seek for Program Header");
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

    *estimated_needed_page_table += 5; /* TODO: better estimation */
    for entry in elf_header.get_program_header_iter_mut(unsafe {
        core::slice::from_raw_parts_mut(
            boot_info.elf_program_header_address as *mut u8,
            elf_program_headers_size,
        )
    }) {
        if entry.get_segment_type() == ELF_PROGRAM_HEADER_SEGMENT_LOAD {
            println!(
                "PA: {:#X}, VA: {:#X}, MS: {:#X}, FS: {:#X}, FO: {:#X}, AL: {}, R:{}, W: {}, E:{}",
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
            if (entry.get_virtual_address() as usize) < TTBR1_EL1_START_ADDRESS {
                println!(
                    "VirtualAddress({:#X}) is not on High Memory, this may cause the boot error.",
                    entry.get_virtual_address()
                );
            }
            let alignment = entry.get_align().max(1);
            let align_offset = (entry.get_virtual_address() & (alignment - 1)) as usize;
            if ((entry.get_virtual_address() as usize) & !EFI_PAGE_MASK) != 0 {
                panic!("Invalid Alignment: {:#X}", alignment);
            } else if entry.get_memory_size() == 0 {
                continue;
            }

            let aligned_memory_size = ((entry.get_memory_size() as usize + align_offset - 1)
                & EFI_PAGE_MASK)
                + EFI_PAGE_SIZE;
            let num_of_pages = aligned_memory_size / EFI_PAGE_SIZE;
            let mut allocated_memory = 0;
            let r = (boot_service.allocate_pages)(
                EfiAllocateType::AllocateAnyPages,
                EfiMemoryType::EfiLoaderData,
                num_of_pages,
                &mut allocated_memory,
            );
            if r != EFI_SUCCESS {
                panic!("Failed to allocate memory for Kernel");
            }
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
                if entry.get_memory_size() > entry.get_file_size() {
                    unsafe {
                        core::ptr::write_bytes(
                            (allocated_memory + align_offset + read_size) as *mut u8,
                            0,
                            (entry.get_memory_size() - entry.get_file_size()) as usize,
                        )
                    }
                }
            }
            entry.set_physical_address((allocated_memory + align_offset) as u64);
            *estimated_needed_page_table += (num_of_pages / NUM_OF_ENTRIES_IN_PAGE_TABLE).max(1);
        }
    }
    let _ = (file_protocol.close)(file_protocol);
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
    let supported_size = core::mem::size_of::<EfiGraphicsOutputProtocolMode>();
    if (mode.size_of_info as usize) != supported_size {
        println!("Unsupported EfiGraphicsOutputModeInformation(Expected {:#X} bytes, but found {:#X} bytes",supported_size,mode.size_of_info );
        return None;
    }
    Some(GraphicInfo {
        frame_buffer_base: mode.frame_buffer_base,
        frame_buffer_size: mode.frame_buffer_size,
        info: unsafe { &*mode.info }.clone(),
    })
}

#[naked]
extern "C" fn down_to_el1() {
    unsafe {
        asm!(
            "
            mov x0, (1 << 11) | (1 << 10) | (1 << 9) | (1 << 8) | (1 << 1) | (1 << 0) 
            msr cnthctl_el2, x0
            mov x0, sp
            msr sp_el1, x0
            adr x0, {}
            msr elr_el2, x0
            mrs x0, tcr_el2
            msr tcr_el1, x0
            mrs x0, ttbr0_el2
            msr ttbr0_el1, x0
            mrs x0, sctlr_el2
            msr sctlr_el1, x0
            mrs x0, mair_el2
            msr mair_el1, x0
            mov x0, 0xC5
            msr spsr_el2, x0
            mov x0, (1 << 47) | (1 << 41) | (1 << 40)
            orr x0, x0, (1 << 31)
            orr x0, x0, (1 << 19)
            msr hcr_el2, x0
            eret
        ",
            sym boot_latter_half,
            options(noreturn)
        )
    }
}

#[panic_handler]
#[no_mangle]
pub fn panic(p: &panic::PanicInfo) -> ! {
    if let Some(location) = p.location() {
        if unsafe { !SYSTEM_TABLE.is_null() } {
            println!(
                "Panic: Line {} in {}: {}",
                location.line(),
                location.file(),
                p.message().unwrap()
            )
        }
    }
    loop {
        unsafe { asm!("wfi") };
    }
}
