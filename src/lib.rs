use std::any::Any;
use std::cell::RefCell;
use crate::sys::Task;

mod sys;

/// A generator task.
/// 
/// This struct wraps around a generator function, and allows a given user to
/// request values from it, for as long as desired.
/// 
/// # Execution Model
/// Generators are functions that execute as part of generator tasks, and 
/// generator tasks are cooperative user-mode threads that can be suspended and
/// resumed at yield points. Tasks are divided into producers and consumers,
/// with consumer tasks being the ones responsible for holding the [`Generator`]
/// object, and producer tasks being responsible for producing values.
/// 
/// When a consumer task wishes for that a new value be generated, it calls the
/// [`Generator::next`] function. Its executing gets temporarily suspended, and
/// the producer task is resumed. Producer tasks may at any moment choose to
/// call [`yeet`] or [`yeet_all`], at which point their execution is suspended.
/// Execution is then transferred back to the generator, which now has been
/// handed the just-yielded value.
/// 
/// Code running inside a generator task is allowed to create [`Generator`]
/// objects of its own. In this case, the tasks spawned will be producers for
/// the current task, which is their consumer task. In effect, this means that
/// one is allowed to recursively spawn generator tasks. This allows one to
/// build with generator tasks a tree topology that is much like a function call
/// tree, and, like in a function call tree, only one node gets is running at
/// any given time.
/// 
/// Different tasks never cross native thread boundaries.
/// 
/// # From a Generator Function
/// Instances of this struct may be created using the [`Generator::from_fn_ptr`]
/// function, which will run the given function as a generator task. It is
/// expected that all the values yielded by the function are of type `T`.
/// 
pub struct Generator<T: 'static> {
	task: Task<T>,
	first: bool,
}
impl<T: 'static> Generator<T> {
	/// Creates a new instance of this structure from a raw function pointer.
	pub fn from_fn_ptr(func: fn()) -> Self {
		Self {
			task: sys::new_task(func),
			first: true,
		}
	}
	
	/// Enters the task sending the given resume value.
	fn enter_with(&mut self, val: Send) -> Yield<T> {
		let this = &mut self.task as *mut _;
		TASK_STACK.with_borrow_mut(|stack| {
			stack.push(this as *mut dyn Any);
		});
		
		/* This cannot panic. */
		let result = unsafe {
			sys::enter(this, val)
		};

		/* We want to stop any possible unwinds here, because if we're running
		 * inside a task, the start function might want to call `yield_internal`
		 * to report the panic to the parent task.
		 * 
		 * Right at this moment, though, `yield_internal` will consider this the
		 * parent task. Which is a problem, because if we panic before the
		 * context switch, the yield address for this task might be complete
		 * nonsense, and if we panic after the context switch, we will end up
		 * re-running destructors. Both of these are horrible outcomes. */  
		let try_pop = std::panic::catch_unwind(|| {
			TASK_STACK.with_borrow_mut(|stack| {
				stack.pop();
			})
		});
		if let Err(_) = try_pop {
			/* If we fail to pop the stack, we're done for. Stop here. */
			std::process::abort()
		}
		
		result
	}
}
impl<T: 'static> Iterator for Generator<T> {
	type Item = T;

	fn next(&mut self) -> Option<Self::Item> {
		self.first = false;
		match self.enter_with(Send::Continue) {
			Yield::StopIteration => None,
			Yield::Panic(what) => std::panic::resume_unwind(what),
			Yield::Value(value) => Some(value)
		}
	}
}
impl<T: 'static> Drop for Generator<T> {
	fn drop(&mut self) {
		if self.first {
			/* Tasks that haven't been started don't need cleanup. */
			return
		}
		
		loop {
			match self.enter_with(Send::Cancel) {
				Yield::StopIteration => 
					/* The task had already ended before we cancelled it */
					break,
				Yield::Panic(what) => {
					if what.is::<CancelTask>() {
						/* This is confirmation that the task was cancelled. */
						break
					} else {
						/* Something else happened that we weren't expecting.
						 * Propagate the exception up. */
						std::panic::resume_unwind(what)
					}
				}
				Yield::Value(_) => {
					/* This may happen if there's a yield in destructor code. 
					 * Just drop whatever value we receive. */
				}
			}
		}
	}
}

thread_local! {
	/// The current stack of executing tasks.
	/// 
	/// Every time a task is entered the pointer to its context structure gets
	/// pushed to this stack, and every time a task returns the pointer to its
	/// context structure gets popped off the stack.
	///
	/// In effect, this stack always points to the context structure for the
	/// currently running task.
	static TASK_STACK: RefCell<Vec<*mut dyn Any>> = Default::default()
}

/// Yields the given packet of data, and returns the data sent by the consumer.
fn yield_internal<T: 'static>(val: Yield<T>) -> Send {
	let task = TASK_STACK.with_borrow_mut(|stack| {
		let top = match stack.last() {
			Some(top) => *top,
			None => panic!("Tried to yield from outside a generator!")
		};

		let task = unsafe { &mut *top };
		match task.downcast_mut::<Task<T>>() {
			Some(task) => task as *mut _,
			None => panic!("Tried to yield a value of the wrong type!")
		}
	});
	
	let (_, value) = unsafe { sys::exit(task, val) };
	
	value
}

/// Yield the given value.
///
/// This function will suspend the currently running function and return control
/// to the consumer, along with the value being yielded. 
/// 
/// # Requirements
/// This function must be called from inside a generator. Meaning that code
/// which calls into this function must have been reached through the [`Generator`]
/// type, by using [`Generator::from_fn_ptr`].
///
/// The type `T` must also match the type used in the specialization of the
/// [`Generator`] structure that is driving the current generator.
///
/// # Panic
/// This function will panic if it is either not being called from inside a
/// generator, of if `T` is mismatched with the type expected by the consumer.  
pub fn yeet<T: 'static>(val: T) {
	match yield_internal(Yield::Value(val)) {
		Send::Continue => {
			/* We've been requested to continue, so do nothing and let the
			 * current task yield another value or enter the stop loop. */ 
		}
		Send::Cancel => {
			/* We've been requested to stop. Start unwinding the stack on this
			 * task, so that we can properly clean it up, and let the task start
			 * function for the current system propagate the cancellation up to
			 * the parent task. */
			std::panic::panic_any(CancelTask)
		}
	}
}

/// Yield all the values in the given iterator.
pub fn yeet_all<T: 'static, I: Iterator<Item = T>>(iter: I) {
	for i in iter {
		yeet(i)
	}
}

/// Internal signal associated with task cancellation.
///
/// # Task Cancellation
/// Cancelling generator tasks is not a trivial endeavor, seeing as we need to
/// run the destructors for all the functions in the call stacks of the tree of
/// tasks that any given generator may have spawned.
///
/// To do this, we use the regular panic mechanism provided to us by Rust. This
/// is generally fine as our code should always be at the base of all generator
/// task call stacks. But it is a problem if users call [`std::panic::catch_unwind`].
struct CancelTask;

/// Possible signals that may be sent to a producer.
enum Send {
	/// Continue until the next yield point.
	Continue,
	/// Cancel the task and free up all the resources associated with it.
	Cancel
}

/// Possible ways data may come out of a producer.
/// 
/// When yielding a value, there are extra conditions that we want to communicate
/// from the producer to the consumer, but we only want to keep them inside the
/// crate, as an implementation detail.
enum Yield<T> {
	/// The generator is done yielding data.
	///
	/// Any subsequent request will yield the same value.
	StopIteration,
	/// The generator has panicked with the given payload.
	/// 
	/// We should propagate this panic forward, and we must ensure that any
	/// subsequent request will yield a [`StopIteration`]. 
	Panic(Box<dyn Any + std::marker::Send + 'static>),
	/// The generator has yielded another piece of data.
	Value(T)
}