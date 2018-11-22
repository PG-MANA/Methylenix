#[derive(Clone)]
pub struct FrameBufferInfo {
    pub address: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u8,
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
    reserved: u8,
    /*color_infoは無視してる*/
}

impl FrameBufferInfo {
    pub fn new(info: &MultibootTagFrameBuffer) -> FrameBufferInfo {
        let mut fb_info = FrameBufferInfo {
            address: info.framebuffer_addr,
            pitch: info.framebuffer_pitch,
            width: info.framebuffer_width,
            height: info.framebuffer_height,
            depth: info.framebuffer_bpp,
        };
        if info.framebuffer_type != 1 {
            //Direct RGBではない
            fb_info.depth = 0;
        }
        fb_info
    }
}
