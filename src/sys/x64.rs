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
	regs: [u64; 16],
	pc: u64,
}

/// Adds an alignment requirement to [`SnapshotUnaligned`] that allows instances
/// of the snapshot structure to be created and managed within Rust, but still
/// be used correctly from inside our assembly code.
#[repr(C, align(8))]
pub struct Snapshot(SnapshotUnaligned);

/// Known-ABI wrapping for [`super::generator_start`].
unsafe extern "sysv64" fn abi_wrap_generator_start<T: 'static>(task: *mut Task<T>) -> ! {
	super::generator_start(task)
}

/// See [`super::start`].
pub unsafe fn impl_start<T: 'static>(task: *mut Task<T>) {
	let tx_snap = (*task).tx_snap.as_mut_ptr();

	/* Set RSP and RBP to the top of the stack region in the task. */
	let stack = ((*task).stack.as_ptr() as usize + (*task).stack.len() * size_of::<PageAlign>()) as u64;
	(&raw mut (*tx_snap).0.regs[6]).write_unaligned(stack);
	(&raw mut (*tx_snap).0.regs[7]).write_unaligned(stack);

	/* Set the PC to the proper specialization of `_generator_start`. */
	(&raw mut (*tx_snap).0.pc)
		.write_unaligned(abi_wrap_generator_start::<T> as usize as u64);

	/* Set the first argument of `generator_start` to this generator instance. */
	(&raw mut (*tx_snap).0.regs[4])
		.write_unaligned(task as usize as u64);
}

global_asm!(r#"
	.global x64_do_switch_ctx
x64_do_switch_ctx:
	/* Save the return address as the PC of the `from` snapshot. */
	POP QWORD PTR [RCX + 128]
	
	/* Load the context of the `to` snapshot. */
	MOV RBX, QWORD PTR [RDX + 8]
	MOV RDI, QWORD PTR [RDX + 32]
	MOV RSI, QWORD PTR [RDX + 40]
	MOV RSP, QWORD PTR [RDX + 48]
	MOV RBP, QWORD PTR [RDX + 56]
	MOV R8,  QWORD PTR [RDX + 64]
	MOV R9,  QWORD PTR [RDX + 72]
	MOV R10, QWORD PTR [RDX + 80]
	MOV R11, QWORD PTR [RDX + 88]
	MOV R12, QWORD PTR [RDX + 96]
	MOV R13, QWORD PTR [RDX + 104]
	MOV R14, QWORD PTR [RDX + 112]
	MOV R15, QWORD PTR [RDX + 120]

	/* Call to resume execution. We don't ever expect the function to return,
	 * but we do this to align the stack properly for Rust. */
	CALL QWORD PTR [RDX + 128]
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
			MOV QWORD PTR [RCX + 8],   RBX
			MOV QWORD PTR [RCX + 32],  RDI
			MOV QWORD PTR [RCX + 40],  RSI
			MOV QWORD PTR [RCX + 48],  RSP
			MOV QWORD PTR [RCX + 56],  RBP
			MOV QWORD PTR [RCX + 64],  R8
			MOV QWORD PTR [RCX + 72],  R9
			MOV QWORD PTR [RCX + 80],  R10
			MOV QWORD PTR [RCX + 88],  R11
			MOV QWORD PTR [RCX + 96],  R12
			MOV QWORD PTR [RCX + 104], R13
			MOV QWORD PTR [RCX + 112], R14
			MOV QWORD PTR [RCX + 120], R15

			/* Call the second half of the context switch function, which both
			 * restores most of the context of the `to` function and makes
			 * preparations a resume to return after the CALL. */
			CALL x64_do_switch_ctx
			
			/* x64_do_switch_ctx CALLs this location. Get rid of the extra
			 * value on the stack. */
			ADD RSP, 8
		"#,
		inout("rax") task,
		inout("rcx") from,
		inout("rdx") to,
	);

	let _ = from;
	let _ = to;

	/* Return the new pointer to be used for the task if this was a yield. */
	task
}

