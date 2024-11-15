use std::arch::{asm, global_asm};
use crate::sys::{PageAlign, Task};

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
}

/// Adds an alignment requirement to [`SnapshotUnaligned`] that allows instances
/// of the snapshot structure to be created and managed within Rust, but still
/// be used correctly from inside our assembly code.
#[repr(C, align(8))]
pub struct Snapshot(SnapshotUnaligned);

/// Known-ABI wrapping for [`super::generator_start`].
unsafe extern "C" fn abi_wrap_generator_start<T: 'static>(task: *mut Task<T>) -> ! {
	super::generator_start(task)
} 

/// See [`super::start`].
pub unsafe fn impl_start<T: 'static>(task: *mut Task<T>) {
	let tx_snap = (*task).tx_snap.as_mut_ptr();

	/* Set SP to the top of the stack region in the task. */
	(&raw mut (*tx_snap).0.sp)
		.write_unaligned(((*task).stack.as_ptr() as usize + (*task).stack.len() * size_of::<PageAlign>()) as u64);

	/* Set the PC to the proper specialization of `_generator_start`. */
	(&raw mut (*tx_snap).0.pc)
		.write_unaligned(abi_wrap_generator_start::<T> as usize as u64);

	/* Set the first argument of `_generator_start` to this generator instance. */
	(&raw mut (*tx_snap).0.regs[0])
		.write_unaligned(task as usize as u64);
}

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

/// See [`super::switch_ctx`].
pub unsafe fn impl_switch_ctx<T>(mut task: *mut Task<T>, yi: bool) -> *mut Task<T> {
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

