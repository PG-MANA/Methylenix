//!
//! Print to UEFI Simple Text Output Protocol
//!

use crate::EfiStatus::Success;
use crate::kernel::drivers::efi::protocol::simple_text_output_protocol::EfiSimpleTextOutputProtocol;

use core::fmt::{Error, Write};

pub(super) struct Printer<'a> {
    p: &'a EfiSimpleTextOutputProtocol,
}

pub(super) static mut WRITER: Option<Printer> = None;

impl Write for Printer<'_> {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        let mut buf = [0; 256];
        let mut pointer = 0;

        for x in s.encode_utf16() {
            if pointer >= buf.len() - 1 {
                let status = (self.p.output_string)(self.p, buf.as_ptr());
                if status != Success {
                    return Err(Error {});
                }
                pointer = 0;
            }
            if x == b'\n' as u16 {
                buf[pointer] = b'\r' as u16;
                buf[pointer + 1] = x;
                let status = (self.p.output_string)(self.p, buf.as_ptr());
                if status != Success {
                    return Err(Error {});
                }
                pointer = 0;
                continue;
            }
            buf[pointer] = x;
            pointer += 1;
        }
        buf[pointer] = 0;
        if (self.p.output_string)(self.p, buf.as_ptr()) == Success {
            Ok(())
        } else {
            Err(Error {})
        }
    }
}

pub fn init(output_service: &'static EfiSimpleTextOutputProtocol) {
    unsafe { *(&raw mut WRITER).as_mut().unwrap() = Some(Printer { p: output_service }) };
}
