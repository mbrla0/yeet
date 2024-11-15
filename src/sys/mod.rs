use crate::{Send, Yield};

#[cfg(target_arch = "x86_64")]
mod x64;
#[cfg(target_arch = "x86_64")]
use x64 as _sys;

#[cfg(target_arch = "aarch64")]
mod arm64;
#[cfg(target_arch = "aarch64")]
use arm64 as _sys;

pub use _sys::Task;

/// Sets up a new task to run the given generator function.
pub fn new_task<T>(func: fn()) -> Task<T> {
	_sys::impl_new_task(func)
}

/// Enters a task with a given payload.
/// 
/// # Panic
/// This function is guaranteed to never panic.
pub unsafe fn enter<T: 'static>(task: *mut Task<T>, data: Send) -> Yield<T> {
	_sys::impl_enter(task, data)
}

/// Exits a task with a given payload.
pub unsafe fn exit<T>(task: *mut Task<T>, data: Yield<T>) -> (*mut Task<T>, Send) {
	_sys::impl_exit(task, data)
}
