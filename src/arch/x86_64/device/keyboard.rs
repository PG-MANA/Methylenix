/*
 * Copyright 2017 PG_MANA
 *
 * This software is Licensed under the Apache License Version 2.0
 * See LICENSE.md
 *
 * キーボード制御
 */

use arch::x86_64::device::{cpu, pic};
use arch::x86_64::interrupt::{idt, IDTMan};
use usr::fifo::FIFO;

pub struct Keyboard {
    fifo: FIFO<u8>,
}

pub static mut default_keyboard: Keyboard = Keyboard {
    fifo: FIFO::const_new(128, &0u8),
};

impl Keyboard {
    const PORT_KEYSTA: u16 = 0x0064;
    const KEYSTA_SEND_NOTREADY: u8 = 0x02;
    const KEYCMD_WRITE_MODE: u8 = 0x60;
    const KBC_MODE: u8 = 0x47;
    const PORT_KEYDAT: u16 = 0x0060;
    const PORT_KEYCMD: u16 = 0x0064;

    pub unsafe fn init(idt_manager: &IDTMan, selector: u64) {
        make_interrupt_hundler!(inthandler21, Keyboard::inthandler21_main);
        idt_manager.set_gatedec(
            0x21,
            idt::GateDescriptor::new(
                inthandler21, /*上のマクロで指定した名前*/
                selector as u16,
                0,
                idt::GateDescriptor::AR_INTGATE32,
            ),
        );
        Keyboard::wait_kbc_sendready();
        cpu::out_byte(Keyboard::PORT_KEYCMD, Keyboard::KEYCMD_WRITE_MODE);
        Keyboard::wait_kbc_sendready();
        cpu::out_byte(Keyboard::PORT_KEYDAT, Keyboard::KBC_MODE);
        pic::pic0_accept(0x02); //1はタイマー(1 <<1 = 0x02)
    }

    /*pub fn new() -> Keyboard {
        Keyboard {
            fifo : FIFO::new(128),
        }
    }*/

    pub fn dequeue_key() -> Option<u8> {
        unsafe { default_keyboard.fifo.dequeue() }
    }

    unsafe fn read_keycode() -> u8 {
        cpu::in_byte(Keyboard::PORT_KEYDAT)
    }

    unsafe fn wait_kbc_sendready() {
        /* キーボードコントローラがデータ送信可能になるのを待つ */
        loop {
            if (cpu::in_byte(Keyboard::PORT_KEYSTA) & Keyboard::KEYSTA_SEND_NOTREADY) == 0 {
                break;
            }
        }
    }

    pub fn inthandler21_main() {
        unsafe {
            default_keyboard.fifo.queue(Keyboard::read_keycode());
            pic::pic0_eoi(0x01); //IRQ-01
        }
    }
}
