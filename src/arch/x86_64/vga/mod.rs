/*
 * Copyright 2017 PG_MANA
 *
 * This software is Licensed under the Apache License Version 2.0
 * See LICENSE.md
 *
 * VGA Textモードでprintするもの
 * 張り切ってコードをかいたもののコンパイラに怒られ続け生き残ったコードはこれだけ...
 */

//use
use core::fmt;
use usr::spin_lock::Mutex;

pub struct VgaText {
    count: u32,
    buf: u32,
    width: u32,
}

pub static mut VGA_TEXT: Mutex<VgaText> = Mutex::new(VgaText {
    count: 0,
    buf: 0xb8000,
    width: 80,
});

impl VgaText {
    //メソッド構文
    //https://rust-lang-ja.github.io/the-rust-programming-language-ja/1.6/book/method-syntax.html
    pub fn write(&mut self, string: &str) -> bool {
        for code in string.bytes() {
            //code:一文字
            match code {//if多用するより良さそう
                b'\n'/*('\n' as u8)*/ => self.count += self.width - self.count % self.width,
                b'\r' => self.count -= self.count % self.width,
                code => {
                    let t: u16 = 0x0b00 | (code as u16);
                    unsafe {
                        *((self.buf + self.count *2) as *mut u16) = t;
                    }
                    self.count += 1;//++使いたい
                }
            }
        }
        return true;
    }
}

impl fmt::Write for VgaText {
    // VgaText : public fmt::Write みたいな感じ?
    fn write_str(&mut self, string: &str) -> fmt::Result {
        if self.write(string) {
            return Ok(());
        } else {
            return Err(fmt::Error {});
        }
    }
}

#[allow(unused_must_use)]
pub fn print_vga(args: fmt::Arguments) {
    unsafe {
        use core::fmt::Write;
        let vga = VGA_TEXT.try_lock();
        if vga.is_ok() {
            vga.unwrap().write_fmt(args);
        }
    }
}

pub fn print_str_vga(string: &str) {
    unsafe {
        let vga = VGA_TEXT.try_lock();
        if vga.is_ok() {
            vga.unwrap().write(string);
        }
    }
}

macro_rules! puts {
    ($fmt:expr) => {
        $crate::arch::x86_64::vga::print_str_vga($fmt)
    };
}

macro_rules! print {
    ($($arg:tt)*) => {
        $crate::arch::x86_64::vga::print_vga(format_args!($($arg)*));
    };
}

macro_rules! println {
    ($fmt:expr) => (print!(concat!($fmt,"\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"),$($arg)*)); //\nをつける
}
