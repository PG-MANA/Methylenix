/*
 * Copyright 2017 PG_MANA
 *
 * This software is Licensed under the Apache License Version 2.0
 * See LICENSE.md
 *
 * Rust入門コード
 * C言語との連携はないと考えている...考えているつもり
 */

#![no_std]
#![feature(asm)]
#![feature(lang_items)]
#![feature(const_fn)]
#![feature(const_size_of)]
#![feature(naked_functions)]
#![feature(use_extern_macros)]
//#![no_main]

//crate
//pub extern crate rlibc;

//Arch
#[macro_use]
pub mod arch;

//usr
pub mod usr;

//そう...何もない!!
