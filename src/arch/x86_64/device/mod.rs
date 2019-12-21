/*
デバイスを扱い(ここはサブモジュールを読み込むだけ、共通関数実装)
*/

pub mod local_apic;
pub mod cpu;
pub mod crt;
pub mod io_apic;
pub mod pic;
pub mod serial_port;