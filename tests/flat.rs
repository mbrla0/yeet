//! This module tests generators at a depth of one.

use yeet::Generator;

#[test]
pub fn many_values() {
	fn gen() {
		let mut val = 0u16;
		loop {
			yeet::yeet(val);

			val = match val.checked_add(1) {
				Some(next) => next,
				None => break
			};
		}
	}
	
	let mut gen = Generator::<u16>::from_fn_ptr(gen);
	for i in 0..=u16::MAX {
		assert_eq!(gen.next(), Some(i));
	}
	assert_eq!(gen.next(), None);
}

#[test]
pub fn single_value() {
	fn gen() {
		yeet::yeet(1u8);
	}
	
	let mut gen = Generator::<u8>::from_fn_ptr(gen);

	assert_eq!(gen.next(), Some(1));
	assert_eq!(gen.next(), None);
}

#[test]
#[should_panic]
fn panic_propagation() {
	fn gen() {
		panic!("This should panic!")
	}
	
	let mut gen = Generator::<u8>::from_fn_ptr(gen);
	let _ = gen.next();
}

#[test]
fn move_generator() {
	fn gen() {
		yeet::yeet(1u8);
		yeet::yeet(2u8);
	}

	#[inline(never)]
	fn start() -> (Generator<u8>, *const Generator<u8>) {
		let mut gen = Generator::<u8>::from_fn_ptr(gen);
		assert_eq!(gen.next(), Some(1));
		let ptr = &gen as *const _;
		(gen, ptr)
	}
	let (moved, orig) = start();
	let mut moved = Box::new(moved);
	
	assert_ne!(orig, &*moved as *const _);
	assert_eq!(moved.next(), Some(2));
}