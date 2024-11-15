use std::arch::{asm, global_asm};
use std::mem::MaybeUninit;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use crate::{Yield, Send, yield_internal};

/// See [`super::new_task`].
pub fn impl_new_task<T>(func: fn()) -> Task<T> {

	Task {
		rx_snap: MaybeUninit::uninit(),
		tx_snap: MaybeUninit::zeroed(),
		data_out: MaybeUninit::uninit(),
		data_in: MaybeUninit::uninit(),
		func: Some(func),
		stack: Box::into_pin(vec![0; 2048 * 1024].into_boxed_slice()),
		started: false,
	}
}

/// See [`super::enter`].
pub unsafe fn impl_enter<T: 'static>(task: *mut Task<T>, send: Send) -> Yield<T> {
	/* Set up the initial thread state of the task. */
	if !(*task).started {
		let tx_snap = (*task).tx_snap.as_mut_ptr();

		/* Set SP to the top of the stack region in the task. */
		(&raw mut (*tx_snap).0.sp)
			.write_unaligned(((*task).stack.as_ptr() as usize + (*task).stack.len()) as u64);

		/* Set the PC to the proper specialization of `_generator_start`. */
		(&raw mut (*tx_snap).0.pc)
			.write_unaligned(_generator_start::<T> as usize as u64);

		/* Set the first argument of `_generator_start` to this generator instance. */
		(&raw mut (*tx_snap).0.regs[0])
			.write_unaligned(task as usize as u64);

		(*task).started = true;
	}
	
	/* Send in the resume data expected by the producer. */
	(*task).data_in.write(send);
	
	/* Enter the task, and wait for it to yield data. We don't use the pointer,
	 * but we expect it to stay the same, as the task is not allowed to move
	 * its own context pointer. */
	switch_ctx(task, false);

	/* Pull out the data we expect the producer to output. */
	(*task).data_out.assume_init_read()
}

/// See [`super::exit`].
pub unsafe fn impl_exit<T>(task: *mut Task<T>, data: Yield<T>) -> (*mut Task<T>, Send) {
	/* Send in the data for the consumer. */
	(*task).data_out.write(data);

	/* Exit the task and return control to the consumer, and wait for it to
	 * enter the task again. We return both the resume data that the consumer
	 * sent in and the context pointer, as the context structure might've been
	 * moved around by the consumer. */
	let new_task = switch_ctx(task, true);

	(new_task, (*new_task).data_in.assume_init_read())
}

/// Every generator comprises a consumer task and a producer task, with a
/// channel for sending data from one to the other. This structure provides the
/// storage for that data.
#[repr(C)]
pub struct Task<T> {
	/// Storage for the context snapshot of the consumer task.
	rx_snap: MaybeUninit<Snapshot>,
	/// Storage for the context snapshot of the producer task.
	tx_snap: MaybeUninit<Snapshot>,
	/// Storage for the data being sent from producer to consumer.
	data_out: MaybeUninit<Yield<T>>,
	/// Storage for the data being sent from consumer to producer.
	data_in: MaybeUninit<Send>,
	/// Storage for the generator function that we want to execute.
	func: Option<fn()>,
	/// Stack region that belongs to the generator.
	stack: Pin<Box<[u8]>>,
	/// Whether this task has already been started.
	started: bool,
}

/// Executes the generator.
/// 
/// This function is the function at the root of the call stack of all generator
/// tasks. It is responsible for wrapping the safe generator function that was
/// given to us by the user, running it, and yielding the values we expect in
/// the consumer side of the runtime.
unsafe extern "C" fn _generator_start<T: 'static>(task: *mut Task<T>) -> ! {
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

/// Contains the register state of a given coroutine at the time of a context
/// switch.
/// 
/// # On `repr(packed)`
/// Generally, packed structures are very troublesome in Rust, but the contents
/// of this structure are not meant to be inspected from inside Rust at all, and
/// are only supposed to be used from inside assembly code.
#[repr(C, packed)]
struct SnapshotUnaligned {
	regs: [u64; 32],
	pc: u64,
	sp: u64,
	pstate: u64,
}

/// Adds an alignment requirement to [`SnapshotUnaligned`] that allows instances
/// of the snapshot structure to be created and managed within Rust, but still
/// be used correctly from inside our assembly code.
#[repr(C, align(8))]
struct Snapshot(SnapshotUnaligned);

global_asm!(r#"
	.global arm64_do_switch_ctx
arm64_do_switch_ctx:
	/* Save X30 as the program counter in the `from` snapshot. */
	STR X30, [X1, #256]
	
	/* Load the context of the `to` snapshot. */
	LDR      X3,  [X2, #24]
	LDP X4,  X5,  [X2, #32]
	LDP X6,  X7,  [X2, #48]
	LDP X8,  X9,  [X2, #64]
	LDP X10, X11, [X2, #80]
	LDP X12, X13, [X2, #96]
	LDP X14, X15, [X2, #112]
	LDP X16, X17, [X2, #128]
	LDP X18, X19, [X2, #144]
	LDP X20, X21, [X2, #160]
	LDP X22, X23, [X2, #176]
	LDP X24, X25, [X2, #192]
	LDP X26, X27, [X2, #208]
	LDP X28, X29, [X2, #224]
	LDP X30, X31, [X2, #240]

	/* Load the return address into IP0. */
	LDR X16, [X2, #256]

	/* Load the stack pointer. */
	LDR X1, [X2, #264]
	MOV SP, X1

	/* Jump to resume execution. */
	BR X16
"#);

/// Switches the context of the current thread.
/// 
/// Enters the context given by the `to` parameter, and makes preparations for
/// the context given by the `from` parameter to be correctly returnable.
unsafe fn switch_ctx<T>(mut task: *mut Task<T>, yi: bool) -> *mut Task<T> {
	let (mut to, mut from) = if !yi {
		(
			(*task).tx_snap.as_mut_ptr(),
			(*task).rx_snap.as_mut_ptr(),
		)
	} else {
		(
			(*task).rx_snap.as_mut_ptr(),
			(*task).tx_snap.as_mut_ptr(),
		)
	};

	asm!(
		r#"
			/* Populate the origin snapshot structure. */
			STR      X3,  [X1, #24]
			STP X4,  X5,  [X1, #32]
			STP X6,  X7,  [X1, #48]
			STP X8,  X9,  [X1, #64]
			STP X10, X11, [X1, #80]
			STP X12, X13, [X1, #96]
			STP X14, X15, [X1, #112]
			STP X16, X17, [X1, #128]
			STP X18, X19, [X1, #144]
			STP X20, X21, [X1, #160]
			STP X22, X23, [X1, #176]
			STP X24, X25, [X1, #192]
			STP X26, X27, [X1, #208]
			STP X28, X29, [X1, #224]
			STP X30, X31, [X1, #240]
			STR XZR, [X1, #272]

			/* Store the stack pointer. */
			MOV X3, SP
			STR X3, [X1, #264]

			/* Call the second half of the context switch function, which both
			 * restores most of the context of the `to` function and makes
			 * preparations a resume to return after the BL. */
			BL arm64_do_switch_ctx

			/* X16 is still clobbered at this point. Restore it. */
			LDR X16, [X2, #128]
		"#,
		inout("x0") task,
		inout("x1") from,
		inout("x2") to,
	);

	let _ = from;
	let _ = to;

	/* Return the new pointer to be used for the task if this was a yield. */
	task
}

