#[cfg(target_arch = "x86_64")]
pub mod x86_64;

//use
#[cfg(target_arch = "x86_64")]
pub use arch::x86_64 as target_arch; //これによりarchとは別のmodからはuse arch::target_archで参照できる
