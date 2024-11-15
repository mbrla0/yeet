//! This module tests recursive generators.

use yeet::Generator;

#[test]
fn simple() {
	fn simple_1() {
		yeet::yeet(1u32);
	}

	fn simple_0() {
		let inner = Generator::<u32>::from_fn_ptr(simple_1);
		yeet::yeet(0u32);
		yeet::yeet_all(inner);
	}
	
	let tree = Generator::<u32>::from_fn_ptr(simple_0);
	let vals = tree.collect::<Vec<_>>();

	assert_eq!(&vals, &[0, 1])
}

#[test]
#[should_panic]
fn panic_propagation() {
	fn simple_1() {
		panic!("This should panic!");
	}

	fn simple_0() {
		let inner = Generator::<u32>::from_fn_ptr(simple_1);
		yeet::yeet(0u32);
		yeet::yeet_all(inner);
	}

	let tree = Generator::<u32>::from_fn_ptr(simple_0);
	let _ = tree.collect::<Vec<_>>();
}

#[test]
fn dfs() {
	fn dfs_3() {
		yeet::yeet(3u32);
	}

	fn dfs_2() {
		let l = Generator::<u32>::from_fn_ptr(dfs_3);
		let r = Generator::<u32>::from_fn_ptr(dfs_3);

		yeet::yeet(2u32);

		yeet::yeet_all(l);
		yeet::yeet_all(r);
	}

	fn dfs_1() {
		let l = Generator::<u32>::from_fn_ptr(dfs_2);
		let r = Generator::<u32>::from_fn_ptr(dfs_2);

		yeet::yeet(1u32);

		yeet::yeet_all(l);
		yeet::yeet_all(r);
	}

	fn dfs_0() {
		let l = Generator::<u32>::from_fn_ptr(dfs_1);
		let r = Generator::<u32>::from_fn_ptr(dfs_1);

		yeet::yeet(0u32);

		yeet::yeet_all(l);
		yeet::yeet_all(r);
	}
	
	let tree = Generator::<u32>::from_fn_ptr(dfs_0);
	let vals = tree.collect::<Vec<_>>();

	assert_eq!(&vals, &[0, 1, 2, 3, 3, 2, 3, 3, 1, 2, 3, 3, 2, 3, 3])
}
