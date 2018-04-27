/*
 * Copyright 2017 PG_MANA
 * 
 * This software is Licensed under the Apache License Version 2.0 
 * See LICENSE.md
 * 
 * structをグローバルでやり取りするためのマネージャ(プロセス通信ができない、割り込み向け)
 */

use core::mem;

