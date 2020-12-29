/*
 * Frame Buffer Manager
 * To write image or text
 * This manager is used to show OS's context,
 * if you want to use graphic normally, make other system like wayland.
 */

use crate::kernel::drivers::multiboot::FrameBufferInfo;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::Address;
use crate::kernel::memory_manager::MemoryPermissionFlags;

pub struct FrameBufferManager {
    frame_buffer_address: usize,
    frame_buffer_width: usize,
    frame_buffer_height: usize,
    frame_buffer_color_depth: u8,
}

impl FrameBufferManager {
    pub const fn new() -> Self {
        Self {
            frame_buffer_address: 0,
            frame_buffer_width: 0,
            frame_buffer_height: 0,
            frame_buffer_color_depth: 0,
        }
    }

    pub fn init_by_multiboot_information(&mut self, frame_buffer_info: &FrameBufferInfo) -> bool {
        if frame_buffer_info.mode != 1 {
            return false;
        }
        self.frame_buffer_address = frame_buffer_info.address as usize;
        self.frame_buffer_width = frame_buffer_info.width as usize;
        self.frame_buffer_height = frame_buffer_info.height as usize;
        self.frame_buffer_color_depth = frame_buffer_info.depth;
        return true;
    }

    pub fn set_frame_buffer_memory_permission(&mut self) -> bool {
        if self.frame_buffer_address == 0 {
            return false;
        }

        match get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .mmap_dev(
                self.frame_buffer_address.into(),
                (self.frame_buffer_width
                    * self.frame_buffer_height
                    * (self.frame_buffer_color_depth >> 3/* /8 */) as usize)
                    .into(),
                MemoryPermissionFlags::data(),
            ) {
            Ok(address) => {
                self.frame_buffer_address = address.to_usize();
                true
            }
            Err(_) => false,
        }
    }

    pub const fn get_frame_buffer_size(&self) -> (usize /*x*/, usize /*y*/) {
        (self.frame_buffer_width, self.frame_buffer_height)
    }

    pub fn clear_screen(&mut self) {
        self.fill(0, 0, self.frame_buffer_width, self.frame_buffer_height, 0);
    }

    pub fn fill(&mut self, start_x: usize, start_y: usize, end_x: usize, end_y: usize, color: u32) {
        assert!(start_x < end_x);
        assert!(start_y < end_y);
        assert!(end_x <= self.frame_buffer_width);
        assert!(end_y <= self.frame_buffer_height);

        if self.frame_buffer_color_depth == 32 {
            for y in start_y..end_y {
                for x in start_x..end_x {
                    unsafe {
                        *((self.frame_buffer_address + (y * self.frame_buffer_width + x) * 4)
                            as *mut u32) = color;
                    }
                }
            }
        } else if self.frame_buffer_color_depth == 24 {
            for y in start_y..end_y {
                for x in start_x..end_x {
                    unsafe {
                        let pixel = (self.frame_buffer_address
                            + (y * self.frame_buffer_width + x) * 3)
                            as *mut u32;
                        *pixel &= 0x000000ff;
                        *pixel |= color;
                    }
                }
            }
        }
    }

    pub fn scroll(
        &mut self,
        from_x: usize,
        from_y: usize,
        to_x: usize,
        to_y: usize,
        size_x: usize,
        size_y: usize,
    ) {
        use core::ptr::copy;
        assert!(from_x + size_x <= self.frame_buffer_width);
        assert!(from_y + size_y <= self.frame_buffer_height);
        assert!(to_x <= from_x);
        assert!(to_y <= from_y);
        if self.frame_buffer_color_depth == 32 {
            for y in 0..size_y {
                unsafe {
                    copy(
                        (self.frame_buffer_address
                            + ((from_y + y) * self.frame_buffer_width + from_x) * 4)
                            as *mut u32,
                        (self.frame_buffer_address
                            + ((to_y + y) * self.frame_buffer_width + to_x) * 4)
                            as *mut u32,
                        size_x,
                    )
                };
            }
        } else if self.frame_buffer_color_depth == 24 {
            for y in 0..size_y {
                unsafe {
                    copy(
                        (self.frame_buffer_address
                            + ((from_y + y) * self.frame_buffer_width + from_x) * 3)
                            as *mut u8,
                        (self.frame_buffer_address
                            + ((to_y + y) * self.frame_buffer_width + to_x) * 3)
                            as *mut u8,
                        size_x * 3,
                    )
                };
            }
        }
    }

    pub fn write_monochrome_bitmap(
        &mut self,
        buffer: usize,
        size_x: usize,
        size_y: usize,
        offset_x: usize,
        offset_y: usize,
        front_color: u32,
        back_color: u32,
        is_not_aligned_data: bool,
    ) {
        assert_ne!(self.frame_buffer_height, 0);
        assert_ne!(self.frame_buffer_width, 0);

        let screen_depth_byte = self.frame_buffer_color_depth as usize >> 3;
        let bitmap_aligned_bitmap_width_pointer = if is_not_aligned_data {
            size_x
        } else {
            (size_x & !3) + 4
        };
        let color = [back_color, front_color];

        let get_bit = |x: usize, y: usize| -> usize {
            let offset = y * bitmap_aligned_bitmap_width_pointer + x;
            unsafe {
                (((*((buffer + offset / 8) as *const u8)) >> (7 - (offset & 0b111))) & 1) as usize
            }
        };

        if self.frame_buffer_color_depth == 32 {
            for height_pointer in (0..size_y).rev() {
                for width_pointer in 0..size_x {
                    unsafe {
                        *((self.frame_buffer_address
                            + ((height_pointer + offset_y) * self.frame_buffer_width
                                + offset_x
                                + width_pointer)
                                * screen_depth_byte) as *mut u32) =
                            color[get_bit(width_pointer, height_pointer)];
                    };
                }
            }
        } else {
            for height_pointer in (0..size_y).rev() {
                for width_pointer in 0..size_x {
                    unsafe {
                        let dot = (self.frame_buffer_address
                            + ((height_pointer + offset_y) * self.frame_buffer_width
                                + offset_x
                                + width_pointer)
                                * screen_depth_byte) as *mut u32;
                        *dot &= 0x000000ff;
                        *dot |= (color[get_bit(width_pointer, height_pointer)]) & 0xffffff;
                    }
                }
            }
        }
    }

    pub fn write_bitmap(
        &mut self,
        buffer: usize,
        depth: u8,
        size_x: usize,
        size_y: usize,
        offset_x: usize,
        offset_y: usize,
        is_not_aligned_data: bool,
    ) -> bool {
        assert_ne!(self.frame_buffer_height, 0);
        assert_ne!(self.frame_buffer_width, 0);

        if depth != 32 && depth != 24 {
            return false;
        }
        let screen_depth_byte = self.frame_buffer_color_depth as usize / 8;
        let bitmap_depth_byte = depth as usize / 8;
        let bitmap_aligned_bitmap_width_pointer = if is_not_aligned_data {
            size_x
        } else {
            ((size_x * bitmap_depth_byte - 1) & !3) + 4
        };

        if self.frame_buffer_color_depth == 32 {
            for height_pointer in (0..size_y).rev() {
                for width_pointer in 0..size_x {
                    unsafe {
                        *((self.frame_buffer_address
                            + ((height_pointer + offset_y) * self.frame_buffer_width
                                + offset_x
                                + width_pointer)
                                * screen_depth_byte) as *mut u32) = *((buffer
                            + (size_y - height_pointer - 1) * bitmap_aligned_bitmap_width_pointer
                            + width_pointer * bitmap_depth_byte)
                            as *const u32);
                    }
                }
            }
        } else {
            for height_pointer in (0..size_y).rev() {
                for width_pointer in 0..size_x {
                    unsafe {
                        let dot = (self.frame_buffer_address
                            + ((height_pointer + offset_y) * self.frame_buffer_width
                                + offset_x
                                + width_pointer)
                                * screen_depth_byte) as *mut u32;
                        *dot &= 0x000000ff;
                        *dot |= *((buffer
                            + (size_y - height_pointer) * bitmap_aligned_bitmap_width_pointer
                            + width_pointer * bitmap_depth_byte)
                            as *const u32)
                            & 0xffffff;
                    }
                }
            }
        }

        return true;
    }
}
