/*
 * Multiboot Information Frame Buffer
 */

#[derive(Clone)]
pub struct FrameBufferInfo {
    pub address: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u8,
    pub mode: u8, /* 0 is palette color, 2 is direct RGB, 3 is text mode*/
}

#[repr(C)]
#[allow(dead_code)]
pub struct MultibootTagFrameBuffer {
    s_type: u32,
    size: u32,
    framebuffer_addr: u64,
    framebuffer_pitch: u32,
    framebuffer_width: u32,
    framebuffer_height: u32,
    framebuffer_bpp: u8,
    framebuffer_type: u8,
    /* https://www.gnu.org/software/grub/manual/multiboot2/multiboot.html 3.6.12 Framebuffer info */
    reserved: u8,
    /* color_info is ignored */
}

impl FrameBufferInfo {
    pub fn new(info: &MultibootTagFrameBuffer) -> FrameBufferInfo {
        FrameBufferInfo {
            address: info.framebuffer_addr,
            pitch: info.framebuffer_pitch,
            width: info.framebuffer_width,
            height: info.framebuffer_height,
            depth: info.framebuffer_bpp,
            mode: info.framebuffer_type,
        }
    }
}
