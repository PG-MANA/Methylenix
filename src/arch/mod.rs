/*
 * Copyright 2017 PG_MANA
 *
 * This software is Licensed under the Apache License Version 2.0
 * See LICENSE.md
 *
 * Arch 分岐のための何か
 * ここで差異を吸収する
 */

#[cfg(target_arch = "x86_64")]
#[macro_use]
pub mod x86_64;

//use
#[cfg(target_arch = "x86_64")]
pub use arch::x86_64 as target_arch; //これによりarchとは別のmodからはuse arch::target_archで参照できる
