//!
//! TTY Manager
//!

use crate::arch::target_arch::device::cpu::is_interrupt_enabled;

use crate::kernel::fifo::FIFO;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::sync::spin_lock::SpinLockFlag;
use crate::kernel::task_manager::wait_queue::WaitQueue;

use core::fmt;
use core::mem::MaybeUninit;

pub struct TtyManager {
    input_lock: SpinLockFlag,
    output_lock: SpinLockFlag,
    input_queue: FIFO<u8, { Self::DEFAULT_INPUT_BUFFER_SIZE }>,
    output_queue: FIFO<u8, { Self::DEFAULT_OUTPUT_BUFFER_SIZE }>,
    output_driver: Option<&'static (dyn Writer)>,
    input_wait_queue: WaitQueue,
}

pub trait Writer {
    fn write(&self, buf: &[u8], size_to_write: usize) -> fmt::Result;
}

impl TtyManager {
    const DEFAULT_INPUT_BUFFER_SIZE: usize = 512;
    const DEFAULT_OUTPUT_BUFFER_SIZE: usize = 512;

    pub const fn new() -> Self {
        Self {
            input_lock: SpinLockFlag::new(),
            output_lock: SpinLockFlag::new(),
            input_queue: FIFO::new(0),
            output_queue: FIFO::new(0),
            output_driver: None,
            input_wait_queue: WaitQueue::new(),
        }
    }

    pub fn input(&mut self, data: u8) -> fmt::Result {
        let _lock = if let Ok(l) = self.input_lock.try_lock() {
            l
        } else if is_interrupt_enabled() {
            self.input_lock.lock()
        } else {
            return Err(fmt::Error {});
        };
        if self.input_queue.enqueue(data) {
            #[allow(unused_must_use)]
            if let Err(e) = self.input_wait_queue.wakeup() {
                use core::fmt::Write;
                write!(self, "Cannot wakeup sleeping threads. Error: {:?}\n", e);
            }
            Ok(())
        } else {
            Err(fmt::Error {})
        }
    }

    pub fn getc(&mut self, allow_sleep: bool) -> Option<u8> {
        let _lock = self.input_lock.lock();
        if let Some(c) = self.input_queue.dequeue() {
            return Some(c);
        }
        if !allow_sleep {
            return None;
        }
        drop(_lock);
        #[allow(unused_must_use)]
        if let Err(e) = self.input_wait_queue.add_current_thread() {
            use core::fmt::Write;
            write!(self, "Cannot wakeup sleeping threads. Error: {:?}\n", e);
        }
        self.getc(false)
    }

    pub fn open(&mut self, driver: &'static dyn Writer) -> bool {
        let _lock = self.output_lock.lock();
        if self.output_driver.is_some() {
            unimplemented!();
        } else {
            self.output_driver = Some(driver);
            return true;
        }
    }

    pub fn puts(&mut self, s: &str) -> fmt::Result {
        let _lock = if let Ok(l) = self.output_lock.try_lock() {
            l
        } else if is_interrupt_enabled() {
            self.output_lock.lock()
        } else {
            return Err(fmt::Error {});
        };

        if self.output_driver.is_none() {
            return Err(fmt::Error {});
        }

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

    pub fn flush(&mut self) -> fmt::Result {
        let _lock = if let Ok(l) = self.output_lock.try_lock() {
            l
        } else if is_interrupt_enabled() {
            self.output_lock.lock()
        } else {
            return Err(fmt::Error {});
        };

        if self.output_driver.is_none() {
            return Err(fmt::Error {});
        }
        self._flush()
    }

    fn _flush(&mut self) -> fmt::Result {
        /* output_driver must be some and locked */
        let mut buffer: [u8; Self::DEFAULT_OUTPUT_BUFFER_SIZE] =
            [unsafe { MaybeUninit::uninit().assume_init() }; Self::DEFAULT_OUTPUT_BUFFER_SIZE];
        let mut pointer = 0usize;
        while let Some(e) = self.output_queue.dequeue() {
            buffer[pointer] = e;
            pointer += 1;
            if pointer == Self::DEFAULT_OUTPUT_BUFFER_SIZE {
                self.output_driver.unwrap().write(&buffer, pointer)?;
                pointer = 0;
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
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"),$($arg)*));
}

#[macro_export]
macro_rules! kprintln {
    ($fmt:expr) => (print!(concat!($fmt,"\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"),$($arg)*));
}

#[macro_export]
macro_rules! pr_info {
    ($fmt:expr) => ($crate::kernel::tty::print_debug_message(6, format_args!(concat!($fmt,"\n"))));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::tty::print_debug_message(6, format_args!(concat!($fmt, "\n"),$($arg)*)));
}

#[macro_export]
macro_rules! pr_warn {
    ($fmt:expr) => ($crate::kernel::tty::print_debug_message(4, format_args!(concat!($fmt,"\n"))));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::tty::print_debug_message(4, format_args!(concat!($fmt, "\n"),$($arg)*)));
}

#[macro_export]
macro_rules! pr_err {
    ($fmt:expr) => ($crate::kernel::tty::print_debug_message(3, format_args!(concat!($fmt,"\n"))));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::tty::print_debug_message(3, format_args!(concat!($fmt, "\n"),$($arg)*)));
}
