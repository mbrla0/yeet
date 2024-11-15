use std::mem::MaybeUninit;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use crate::{Send, Yield, yield_internal};

#[cfg(target_arch = "x86_64")]
mod x64;
#[cfg(target_arch = "x86_64")]
use x64 as _sys;

#[cfg(target_arch = "aarch64")]
mod arm64;
#[cfg(target_arch = "aarch64")]
use arm64 as _sys;

/// Every generator comprises a consumer task and a producer task, with a
/// channel for sending data from one to the other. This structure provides the
/// storage for that data.
#[repr(C)]
pub struct Task<T> {
	/// Storage for the context snapshot of the consumer task.
	rx_snap: MaybeUninit<_sys::Snapshot>,
	/// Storage for the context snapshot of the producer task.
	tx_snap: MaybeUninit<_sys::Snapshot>,
	/// Storage for the data being sent from producer to consumer.
	data_out: MaybeUninit<Yield<T>>,
	/// Storage for the data being sent from consumer to producer.
	data_in: MaybeUninit<Send>,
	/// Storage for the generator function that we want to execute.
	func: Option<fn()>,
	/// Stack region that belongs to the generator.
	stack: Pin<Box<[PageAlign]>>,
	/// Whether this task has already been started.
	started: bool,
}

/// Executes the generator.
///
/// This function is the function at the root of the call stack of all generator
/// tasks. It is responsible for wrapping the safe generator function that was
/// given to us by the user, running it, and yielding the values we expect in
/// the consumer side of the runtime.
unsafe fn generator_start<T: 'static>(task: *mut Task<T>) -> ! {
	/* We can assert unwind safety here as we'll just abort the process if we
	 * catch a panic. No data should be accessed at all. */
	let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
		/* Let the generator function run. */
		if let Some(func) = (&mut *task).func.take() {
			/* It is _absolutely_ not safe to let the unwind continue beyond this
			 * point. There's nothing above this function in the call stack. */
			if let Err(what) = std::panic::catch_unwind(func) {
				/* Let the runtime on the consumer side propagate the panic. */
				let _ = yield_internal::<T>(Yield::Panic(what));
			}
		}

		/* We're done with the generator. Ask the consumer to stop requesting more
	 	 * data, and keep asking, for as long as we need. */
		loop {
			let _ = yield_internal::<T>(Yield::StopIteration);
		}
	}));

	/* There's no way we can recover from this. =( */
	std::process::abort()
}

/// Used to align our stack.
#[repr(align(0x10000))]
#[derive(Copy, Clone)]
struct PageAlign(#[allow(dead_code)] u8);

/// Sets up a new task to run the given generator function.
pub fn new_task<T>(func: fn()) -> Task<T> {
	Task {
		rx_snap: MaybeUninit::uninit(),
		tx_snap: MaybeUninit::zeroed(),
		data_out: MaybeUninit::uninit(),
		data_in: MaybeUninit::uninit(),
		func: Some(func),
		stack: Box::into_pin(vec![PageAlign(0); 2048 * 1024 / size_of::<PageAlign>()].into_boxed_slice()),
		started: false,
	}
}

/// Enters a task with a given payload.
/// 
/// # Panic
/// This function is guaranteed to never panic.
pub unsafe fn enter<T: 'static>(task: *mut Task<T>, data: Send) -> Yield<T> {
	/* Set up the initial thread state of the task. */
	if !(*task).started {
		start(task);
		(*task).started = true;
	}

	/* Send in the resume data expected by the producer. */
	(*task).data_in.write(data);

	/* Enter the task, and wait for it to yield data. We don't use the pointer,
	 * but we expect it to stay the same, as the task is not allowed to move
	 * its own context pointer. */
	switch_ctx(task, false);

	/* Pull out the data we expect the producer to output. */
	(*task).data_out.assume_init_read()
}

/// Exits a task with a given payload.
pub unsafe fn exit<T>(task: *mut Task<T>, data: Yield<T>) -> (*mut Task<T>, Send) {
	/* Send in the data for the consumer. */
	(*task).data_out.write(data);

	/* Exit the task and return control to the consumer, and wait for it to
	 * enter the task again. We return both the resume data that the consumer
	 * sent in and the context pointer, as the context structure might've been
	 * moved around by the consumer. */
	let new_task = switch_ctx(task, true);

	(new_task, (*new_task).data_in.assume_init_read())
}

/// Sets a task up for execution with [`switch_ctx`].
unsafe fn start<T: 'static>(task: *mut Task<T>) {
	_sys::impl_start(task)
}

/// Switches the context of the current thread.
///
/// If `yielding` is true, switches to the consumer task from the producer task,
/// and if `yielding` is false, switches to the producer task from the consumer
/// task.
unsafe fn switch_ctx<T>(task: *mut Task<T>, yielding: bool) -> *mut Task<T> {
	_sys::impl_switch_ctx(task, yielding)
}
