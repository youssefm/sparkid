//! Compile-time verification that sparkid works under no_std + alloc.
//!
//! This crate does not run any tests — it only needs to compile.
//! If someone accidentally adds an ungated `std::` import to sparkid,
//! `cargo build -p no_std_check` will fail.

#![no_std]

extern crate alloc;

use alloc::string::String;
use sparkid::{IdGenerator, SparkId};

/// Exercises the full no_std public API surface at compile time.
/// Never called — just needs to type-check.
#[allow(dead_code)]
fn verify_no_std_api() {
    // IdGenerator is constructible
    let mut gen = IdGenerator::new();

    // next_id_at is available (the no_std entry point)
    let id: SparkId = gen.next_id_at(1_700_000_000_000);

    // SparkId core traits work without std
    let _: &str = &id;           // Deref<Target = str>
    let _: &str = id.as_ref();   // AsRef<str>
    let _: String = id.into();   // Into<String> (alloc)

    // Display, Debug, Clone, Copy, Eq, Ord, Hash — all core traits
    let _ = core::format_args!("{id}");
    let _ = core::format_args!("{id:?}");
    let copy = id;
    let _ = id == copy;
    let _ = id < copy;
}
