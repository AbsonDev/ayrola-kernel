#![allow(dead_code, clippy::missing_const_for_fn)]
use asupersync::Time;
fn foo(now: Time, past: Time) {
    let _ms: u64 = now.duration_since(past);
}
fn main() {}
