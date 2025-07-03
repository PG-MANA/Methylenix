//!
//! TTY Manager
//!

use crate::kernel::{
    collections::fifo::Fifo,
    file_manager::{
        File, FileDescriptor, FileDescriptorData, FileError, FileOperationDriver, FileSeekOrigin,
    },
    manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster},
    memory_manager::data_type::{Address, MOffset, MSize, VAddress},
    sync::spin_lock::{IrqSaveSpinLockFlag, SpinLockFlag},
    task_manager::{wait_queue::WaitQueue, work_queue::WorkList},
};

use core::fmt;
use core::fmt::Write;
use core::mem::MaybeUninit;
use core::ptr::NonNull;

pub struct TtyManager {
    input_lock: SpinLockFlag,
    output_lock: IrqSaveSpinLockFlag,
    input_queue: Fifo<u8, { Self::DEFAULT_INPUT_BUFFER_SIZE }>,
    output_queue: Fifo<u8, { Self::DEFAULT_OUTPUT_BUFFER_SIZE }>,
    output_driver: Option<&'static (dyn Writer)>,
    text_color: (u32, u32),
    input_wait_queue: WaitQueue,
}

pub trait Writer {
    fn write(
        &self,
        buf: &[u8],
        size_to_write: usize,
        foreground_color: u32,
        background_color: u32,
    ) -> fmt::Result;
}

macro_rules! kprint {
    () => ($crate::kernel::tty::kernel_print(format_args!("")));
    ($fmt:expr) => ($crate::kernel::tty::kernel_print(format_args!($fmt)));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::tty::kernel_print(format_args!($fmt, $($arg)*)));
}

macro_rules! kprintln {
    () => ($crate::kernel::tty::kernel_print(format_args!("\n")));
    ($fmt:expr) => ($crate::kernel::tty::kernel_print(format_args!("{}\n", format_args!($fmt))));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::tty::kernel_print(format_args!("{}\n", format_args!($fmt, $($arg)*))));
}

macro_rules! pr_debug {
    ($fmt:expr) => ($crate::kernel::tty::print_debug_message(7, format_args!($fmt)));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::tty::print_debug_message(7, format_args!($fmt, $($arg)*)));
}

macro_rules! pr_info {
    ($fmt:expr) => ($crate::kernel::tty::print_debug_message(6, format_args!($fmt)));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::tty::print_debug_message(6, format_args!($fmt, $($arg)*)));
}

macro_rules! pr_warn {
    ($fmt:expr) => ($crate::kernel::tty::print_debug_message(4, format_args!($fmt)));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::tty::print_debug_message(4, format_args!($fmt, $($arg)*)));
}

macro_rules! pr_err {
    ($fmt:expr) => ($crate::kernel::tty::print_debug_message(3, format_args!($fmt)));
    ($fmt:expr, $($arg:tt)*) => ($crate::kernel::tty::print_debug_message(3, format_args!($fmt, $($arg)*)));
}

macro_rules! bug_on_err {
    ($e:expr) => {
        if let Err(err) = $e {
            pr_warn!("{:?}", err);
        }
    };
}

impl TtyManager {
    const DEFAULT_INPUT_BUFFER_SIZE: usize = 512;
    const DEFAULT_OUTPUT_BUFFER_SIZE: usize = 512;
    pub const NUMBER_OF_KERNEL_TTY: usize = 2;
    pub const DEFAULT_KERNEL_TTY: usize = 1;

    pub const fn new() -> Self {
        Self {
            input_lock: SpinLockFlag::new(),
            output_lock: IrqSaveSpinLockFlag::new(),
            input_queue: Fifo::new(0),
            output_queue: Fifo::new(0),
            output_driver: None,
            text_color: (0x55FFFF, 0x000000),
            input_wait_queue: WaitQueue::new(),
        }
    }

    fn input_into_fifo(data: usize) {
        /* Temporary Implementation */
        for tty in &mut get_kernel_manager_cluster().kernel_tty_manager {
            let _lock = tty.input_lock.lock();
            if tty.input_queue.enqueue(data as u8).is_ok()
                && let Err(err) = tty.input_wait_queue.wakeup_all()
            {
                drop(_lock);
                pr_err!("Failed to wakeup sleeping threads: {:?}", err);
            }
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
        bug_on_err!(self.input_wait_queue.add_current_thread());
        self.getc(false)
    }

    pub fn open(&mut self, driver: &'static dyn Writer) -> bool {
        let _lock = self.output_lock.lock();
        if self.output_driver.is_some() {
            unimplemented!();
        } else {
            self.output_driver = Some(driver);
            true
        }
    }

    pub fn input_from_interrupt_handler(c: u8) {
        let work = WorkList::new(Self::input_into_fifo, c as usize);
        bug_on_err!(get_cpu_manager_cluster().work_queue.add_work(work));
    }

    pub fn puts(&mut self, s: &str) -> fmt::Result {
        let _lock = self.output_lock.lock();

        if self.output_driver.is_none() {
            return Ok(());
            //return Err(fmt::Error {});
        }

        for c in s.bytes() {
            if self.output_queue.enqueue(c).is_err() {
                self._flush()?;
                if self.output_queue.enqueue(c).is_err() {
                    return Err(fmt::Error {});
                }
            }
            if c == b'\n' {
                self._flush()?;
            }
        }
        Ok(())
    }

    pub fn flush(&mut self) -> fmt::Result {
        let _lock = self.output_lock.lock();
        self._flush()
    }

    fn _flush(&mut self) -> fmt::Result {
        if self.output_driver.is_none() {
            return Ok(());
        }
        /* output_driver must be some and locked */
        let mut buffer: [u8; Self::DEFAULT_OUTPUT_BUFFER_SIZE] =
            [unsafe { MaybeUninit::zeroed().assume_init() }; Self::DEFAULT_OUTPUT_BUFFER_SIZE];
        let mut pointer = 0usize;
        while let Some(e) = self.output_queue.dequeue() {
            buffer[pointer] = e;
            pointer += 1;
            if pointer == Self::DEFAULT_OUTPUT_BUFFER_SIZE {
                self.output_driver
                    .unwrap()
                    .write(&buffer, pointer, self.text_color.0, self.text_color.1)
                    .or_else(|err| {
                        self.output_driver = None;
                        Err(err)
                    })?;
                pointer = 0;
            }
        }
        self.output_driver
            .unwrap()
            .write(&buffer, pointer, self.text_color.0, self.text_color.1)
            .or_else(|err| {
                self.output_driver = None;
                Err(err)
            })
    }

    fn change_font_color(
        &mut self,
        foreground_color: u32,
        background_color: u32,
    ) -> Option<(u32, u32)> {
        let _lock = self.output_lock.lock();
        let _ = self._flush();
        let old = self.text_color;
        self.text_color = (foreground_color, background_color);
        drop(_lock);
        Some(old)
    }

    pub fn open_tty_as_file(&'static mut self, permission: u8) -> Result<File, ()> {
        Ok(File::new(
            FileDescriptor::new(FileDescriptorData::Address(0), 0, permission),
            NonNull::new(self).unwrap(),
        ))
    }
}

impl Write for TtyManager {
    fn write_str(&mut self, string: &str) -> fmt::Result {
        self.puts(string)
    }
}

impl FileOperationDriver for TtyManager {
    fn read(
        &mut self,
        _descriptor: &mut FileDescriptor,
        buffer: VAddress,
        length: MSize,
    ) -> Result<MSize, FileError> {
        for read_size in 0..length.to_usize() {
            if let Some(c) = self.getc(true) {
                unsafe { *((buffer.to_usize() + read_size) as *mut u8) = c };
                if c == b'\n' {
                    return Ok(MSize::new(read_size + 1));
                }
            } else {
                return Ok(MSize::new(read_size));
            }
        }
        Ok(length)
    }

    fn write(
        &mut self,
        _descriptor: &mut FileDescriptor,
        buffer: VAddress,
        length: MSize,
    ) -> Result<MSize, FileError> {
        if let Ok(s) = core::str::from_utf8(unsafe {
            core::slice::from_raw_parts(buffer.to_usize() as *const u8, length.to_usize())
        }) {
            self.puts(s).or(Err(FileError::DeviceError))?;
            self.flush().or(Err(FileError::DeviceError))?;
            Ok(length)
        } else {
            Err(FileError::OperationNotSupported)
        }
    }

    fn seek(
        &mut self,
        _descriptor: &mut FileDescriptor,
        _offset: MOffset,
        _origin: FileSeekOrigin,
    ) -> Result<MOffset, FileError> {
        Ok(MOffset::new(0))
    }

    fn close(&mut self, _descriptor: &mut FileDescriptor) {}
}

pub fn kernel_print(args: fmt::Arguments) {
    for tty in &mut get_kernel_manager_cluster().kernel_tty_manager {
        if tty.output_driver.is_none() {
            continue;
        }
        let _ = tty.write_fmt(args);
    }
}

#[track_caller]
pub fn print_debug_message(level: usize, args: fmt::Arguments) {
    use core::panic::Location;
    let level = match level {
        3 => ("[ERROR]", (0xFF0000, 0x000000)),
        4 => ("[WARN]", (0xFF7F27, 0x000000)),
        5 => ("[NOTICE]", (0xFFFF00, 0x000000)),
        6 => ("[INFO]", (0x55FFFF, 0x000000)),
        7 => ("[DEBUG]", (0x55FFFF, 0x000000)),
        _ => ("[???]", (0x55FFFF, 0x000000)),
    };
    let file = Location::caller().file(); //THINKING: filename only
    let line = Location::caller().line();
    for tty in &mut get_kernel_manager_cluster().kernel_tty_manager {
        if tty.output_driver.is_none() {
            continue;
        }
        let original_color = tty.change_font_color(level.1.0, level.1.1);
        let _ = tty.write_fmt(format_args!("{} {}:{} | {}\n", level.0, file, line, args));
        if let Some(c) = original_color {
            tty.change_font_color(c.0, c.1);
        }
    }
}
