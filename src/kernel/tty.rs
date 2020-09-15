/*
 * TTY Manager
 */

use crate::kernel::fifo::FIFO;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::sync::spin_lock::SpinLockFlag;

use core::fmt;
use core::mem::MaybeUninit;

pub struct TtyManager {
    lock: SpinLockFlag,
    input_queue: FIFO<u8, 512usize /*Self::DEFAULT_INPUT_BUFFER_SIZE*/>,
    output_queue: FIFO<u8, 512usize /*Self::DEFAULT_OUTPUT_BUFFER_SIZE*/>,
    output_driver: Option<&'static (dyn Writer)>,
}

pub trait Writer {
    fn write(&self, buf: &[u8], size_to_write: usize) -> fmt::Result;
}

impl TtyManager {
    const DEFAULT_INPUT_BUFFER_SIZE: usize = 512;
    const DEFAULT_OUTPUT_BUFFER_SIZE: usize = 512;

    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            input_queue: FIFO::new(0),
            output_queue: FIFO::new(0),
            output_driver: None,
        }
    }

    pub fn open(&mut self, driver: &'static dyn Writer) -> bool {
        let _lock = self.lock.lock();
        if self.output_driver.is_some() {
            unimplemented!();
        } else {
            self.output_driver = Some(driver);
            return true;
        }
    }

    pub fn puts(&mut self, s: &str) -> fmt::Result {
        if self.output_driver.is_none() {
            return Err(fmt::Error {});
        }
        let _lock = if let Ok(l) = self.lock.try_lock() {
            l
        } else {
            //return Err(fmt::Error {});
            return Ok(());
        };
        for c in s.bytes().into_iter() {
            if !self.output_queue.enqueue(c) {
                self._flush()?;
                if !self.output_queue.enqueue(c) {
                    return Err(fmt::Error {});
                }
            }
            if c == '\n' as u8 {
                self._flush()?;
            }
        }
        Ok(())
    }

    fn _flush(&mut self) -> fmt::Result {
        /* assume output_driver is some and locked */
        let mut buffer: [u8; Self::DEFAULT_OUTPUT_BUFFER_SIZE] =
            [unsafe { MaybeUninit::uninit().assume_init() }; Self::DEFAULT_OUTPUT_BUFFER_SIZE];
        let mut pointer = 0usize;
        while let Some(e) = self.output_queue.dequeue() {
            buffer[pointer] = e;
            pointer += 1;
            if pointer == Self::DEFAULT_OUTPUT_BUFFER_SIZE {
                break;
            }
        }
        self.output_driver.unwrap().write(&buffer, pointer)
    }
}

impl fmt::Write for TtyManager {
    fn write_str(&mut self, string: &str) -> fmt::Result {
        self.puts(string)
    }
}

pub fn kernel_print(args: fmt::Arguments) {
    use core::fmt::Write;
    let r = get_kernel_manager_cluster()
        .kernel_tty_manager
        .write_fmt(args);
    if r.is_err() {
        //panic!("Cannot write a string");
    }
}

#[track_caller]
pub fn print_debug_message(level: usize, args: fmt::Arguments) {
    use core::panic::Location;
    let level_str = match level {
        3 => "[ERROR]",
        4 => "[WARN]",
        5 => "[NOTICE]",
        6 => "[INFO]",
        _ => "[???]",
    };
    let file = Location::caller().file(); //THINKING: filename only
    let line = Location::caller().line();
    kernel_print(format_args!("{} {}:{} | {}", level_str, file, line, args));
}

// macros
#[macro_export]
macro_rules! puts {
    ($fmt:expr) => {
        print!($fmt);
    };
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::kernel::tty::kernel_print(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! println {
    ($fmt:expr) => (print!(concat!($fmt,"\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"),$($arg)*)); //\nをつける
}

#[macro_export]
macro_rules! kprintln {
    ($fmt:expr) => (print!(concat!($fmt,"\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"),$($arg)*)); //\nをつける
}

#[macro_export]
macro_rules! pr_info {
    ($fmt:expr) => ($crate::kernel::tty::print_debug_message(6, format_args!(concat!($fmt,"\n"))));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::tty::print_debug_message(6, format_args!(concat!($fmt, "\n"),$($arg)*))); //\nをつける
}

#[macro_export]
macro_rules! pr_warn {
    ($fmt:expr) => ($crate::kernel::tty::print_debug_message(4, format_args!(concat!($fmt,"\n"))));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::tty::print_debug_message(4, format_args!(concat!($fmt, "\n"),$($arg)*))); //\nをつける
}

#[macro_export]
macro_rules! pr_err {
    ($fmt:expr) => ($crate::kernel::tty::print_debug_message(3, format_args!(concat!($fmt,"\n"))));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::tty::print_debug_message(3, format_args!(concat!($fmt, "\n"),$($arg)*))); //\nをつける
}
