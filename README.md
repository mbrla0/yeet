# yeet
Python-like generators and `yield` operations, but in Rust!

This crate implements cooperative user-mode tasks, and generators on top of them,
allowing for generator code that looks just like Python! Really, it's as simple as
calling `yeet::yeet()` where in Python you'd put a `yield` expression.

```rust
fn generator() {
    yeet::yeet(1u8);
    yeet::yeet(2u8);
    yeet::yeet(3u8);
}

fn main() {
    let mut gen = yeet::Generator::<u8>::from_fn_ptr(generator);
    assert_eq!(gen.next(), Some(1));
    assert_eq!(gen.next(), Some(2));
    assert_eq!(gen.next(), Some(3));
}
```

## Disclaimer
This is a pet project, that I'm doing for fun, so don't take it too seriously.
I've taken a few steps to try and make sure it's not too horrible when it comes
to memory safety, but I haven't spent a lot of time on that. Don't blame me if
you end up shooting yourself in the foot by using this.

Don't take this to mean I'm not open to PRs or issues being opened pointing out
mistakes that I've made, but do take this as me saying I don't want to be blamed
if someone tries to push this into production and breaks a few things.

You've been warned.

## Implementation Status
As one might expect, implementing this takes quite a bit of architecture-specific
muscle, so support for different architectures has to be added in manually.
Currently, this crate supports the following architectures:
- [X] AArch64
- [ ] x86_64

Support for architectures that are listed but not marked are in the roadmap, but I
haven't gotten to them yet.