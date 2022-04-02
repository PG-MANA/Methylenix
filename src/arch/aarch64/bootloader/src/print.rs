//!
//! println for EfiOutputService
//!

use crate::efi::EFI_SUCCESS;
use crate::SYSTEM_TABLE;

use core::fmt;
use core::fmt::Write;

struct Print {}

static mut EFI_OUTPUT_SERVICE_PRINT: Print = Print {};

impl fmt::Write for Print {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if unsafe { SYSTEM_TABLE.is_null() } {
            return Err(fmt::Error {});
        }
        let output_service = unsafe { &*(*SYSTEM_TABLE).get_console_output_protocol() };
        let mut buf = [0; 256];
        let mut pointer = 0;

        for x in s.encode_utf16() {
            if pointer >= buf.len() - 1 {
                let status = (output_service.output_string)(output_service, buf.as_ptr());
                if status != EFI_SUCCESS {
                    return Err(fmt::Error {});
                }
                pointer = 0;
            }
            if x == b'\n' as u16 {
                buf[pointer] = b'\r' as u16;
                buf[pointer + 1] = x;
                let status = (output_service.output_string)(output_service, buf.as_ptr());
                if status != EFI_SUCCESS {
                    return Err(fmt::Error {});
                }
                pointer = 0;
                continue;
            }
            buf[pointer] = x;
            pointer += 1;
        }
        buf[pointer] = 0;
        if (output_service.output_string)(output_service, buf.as_ptr()) == EFI_SUCCESS {
            Ok(())
        } else {
            Err(fmt::Error {})
        }
    }
}

pub fn print(args: fmt::Arguments) {
    let _ = unsafe { EFI_OUTPUT_SERVICE_PRINT.write_fmt(args) };
}

#[macro_export]
macro_rules! println {
    () => ($crate::print::print(format_args_nl!("")));
    ($fmt:expr) => ($crate::print::print(format_args_nl!($fmt)));
    ($fmt:expr, $($arg:tt)*) => ($crate::print::print(format_args_nl!($fmt, $($arg)*)));
}
