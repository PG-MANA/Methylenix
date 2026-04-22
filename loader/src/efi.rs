//!
//! UEFI helper functions
//!

pub mod memory;

use crate::FONT_PATH;

use crate::arch::target_arch::context::memory_layout::physical_address_to_direct_map_loader;
use crate::arch::target_arch::paging::{PAGE_MASK, PAGE_SIZE_USIZE};

use crate::kernel::drivers::boot_information::{FontInfo, GraphicInfo};
use crate::kernel::drivers::efi::{
    EfiBootServices, EfiHandle, EfiStatus,
    EfiStatus::Success,
    protocol::{file_protocol::*, graphics_output_protocol::*, loaded_image_protocol::*},
};
use crate::kernel::memory_manager::data_type::PAddress;

use core::num::NonZeroUsize;

pub static mut BOOT_SERVICES: *const EfiBootServices = core::ptr::null();
pub static mut MAIN_HANDLE: EfiHandle = 0;

pub struct FileHandler {
    root_file_protocol: *const EfiFileProtocol,
    file_protocol: *const EfiFileProtocol,
}

pub fn open_file(
    main_handle: EfiHandle,
    boot_services: &EfiBootServices,
    file_name: &str,
) -> Result<FileHandler, EfiStatus> {
    let mut loaded_image_protocol: *const EfiLoadedImageProtocol = core::ptr::null();
    let mut simple_file_protocol: *const EfiSimpleFileProtocol = core::ptr::null();
    let mut root_file_protocol: *const EfiFileProtocol = core::ptr::null();
    let mut file_protocol: *const EfiFileProtocol = core::ptr::null();
    let mut utf16_file_name: [u16; 128] = [0; 128];

    /* Open loaded_image_protocol */
    let r = (boot_services.open_protocol)(
        main_handle,
        &EFI_LOADED_IMAGE_PROTOCOL_GUID,
        &mut loaded_image_protocol as *mut _ as usize,
        main_handle,
        0,
        EFI_OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
    );
    if r != Success {
        pr_err!("Failed to open LOADED_IMAGE_PROTOCOL: {r:?}");
        return Err(r);
    }

    /* Open simple_file_system_protocol */
    let r = (boot_services.open_protocol)(
        unsafe { (*loaded_image_protocol).device_handle },
        &EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID,
        &mut simple_file_protocol as *mut _ as usize,
        main_handle,
        0,
        EFI_OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
    );
    if r != Success {
        pr_err!("Failed to open SIMPLE_FILE_SYSTEM_PROTOCOL: {r:?}");
        return Err(r);
    }
    let simple_file_protocol = unsafe { &*simple_file_protocol };

    /* Open root directory */
    let r = (simple_file_protocol.open_volume)(simple_file_protocol, &mut root_file_protocol);
    if r != Success {
        pr_err!("Failed to open the root directory: {r:?}");
        return Err(r);
    }
    let root_file_protocol = unsafe { &*root_file_protocol };

    /* Open the file */
    for (i, e) in utf16_file_name.iter_mut().zip(file_name.encode_utf16()) {
        *i = e;
    }
    let r = (root_file_protocol.open)(
        root_file_protocol,
        &mut file_protocol,
        utf16_file_name.as_ptr(),
        EFI_FILE_MODE_READ,
        0,
    );
    if r != Success {
        pr_err!("Failed to open \"{file_name}\": {r:?}");
        let _ = (root_file_protocol.close)(root_file_protocol);
        return Err(r);
    }
    let file_protocol = unsafe { &*file_protocol };

    Ok(FileHandler {
        root_file_protocol,
        file_protocol,
    })
}

pub fn get_file_size(file_handler: &FileHandler) -> u64 {
    let file_protocol = unsafe { &*file_handler.file_protocol };
    let r = (file_protocol.set_position)(file_protocol, u64::MAX);
    if r != Success {
        pr_err!("Failed to ge the file size: {r:?}");
        return 0;
    }
    let mut file_size: u64 = 0;
    let r = (file_protocol.get_position)(file_protocol, &mut file_size);
    if r != Success {
        pr_err!("Failed to ge the file size: {r:?}");
        return 0;
    }
    file_size
}

pub fn read_file(file_handler: &FileHandler, offset: usize, size: usize, buffer: *mut u8) {
    let file_protocol = unsafe { &*file_handler.file_protocol };
    let r = (file_protocol.set_position)(file_protocol, offset as u64);
    let mut read_size = size;

    assert_eq!(r, Success, "Failed to seek: {r:?}");
    let r = (file_protocol.read)(file_protocol, &mut read_size, buffer);
    assert_eq!(r, Success, "Failed to read: {r:?}");
    assert_eq!(size, read_size);
}

pub fn close_file(file_handler: FileHandler) {
    let root_file_protocol = unsafe { &*file_handler.root_file_protocol };
    let file_protocol = unsafe { &*file_handler.file_protocol };

    let _ = (file_protocol.close)(file_protocol);
    let _ = (root_file_protocol.close)(root_file_protocol);
}

pub fn detect_graphics(boot_services: &EfiBootServices) -> Option<GraphicInfo> {
    let mut graphics_output_protocol: *const EfiGraphicsOutputProtocol = core::ptr::null();

    let r = (boot_services.locate_protocol)(
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

    if mode.frame_buffer_base == 0
        || mode.frame_buffer_size == 0
        || mode.size_of_info < size_of_val(&mode.info)
    {
        pr_warn!("Invalid frame buffer information");
        return None;
    }
    Some(GraphicInfo {
        frame_buffer_address: PAddress::new(mode.frame_buffer_base),
        frame_buffer_size: NonZeroUsize::new(mode.frame_buffer_size)?,
        info: unsafe { &*mode.info }.clone(),
    })
}

pub fn load_font_file(main_handle: EfiHandle, boot_services: &EfiBootServices) -> Option<FontInfo> {
    let font_handler = open_file(main_handle, boot_services, FONT_PATH)
        .map_err(|e| pr_err!("Failed to load the font: {e:?}"))
        .ok()?;

    let file_size = get_file_size(&font_handler);
    if file_size == 0 {
        println!("Invalid font file size");
        close_file(font_handler);
        return None;
    }

    /* Load the font file */
    let Some(allocated_memory) =
        memory::allocate_pages((((file_size as usize - 1) & PAGE_MASK) / PAGE_SIZE_USIZE) + 1)
    else {
        pr_warn!("Failed to allocate memory");
        close_file(font_handler);
        return None;
    };
    read_file(
        &font_handler,
        0,
        file_size as usize,
        allocated_memory as *mut u8,
    );
    println!("Loaded th font (Address: {allocated_memory:#X}, Size: {file_size:#X})");

    close_file(font_handler);
    Some(FontInfo {
        font_address: physical_address_to_direct_map_loader(PAddress::new(allocated_memory)),
        font_size: NonZeroUsize::new(file_size as usize)?,
    })
}
