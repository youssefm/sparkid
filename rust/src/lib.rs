//! # sparkid
//!
//! Fast, time-sortable, 21-char Base58 unique ID generator.
//!
//! Each ID is composed of three parts:
//!   - 8-char timestamp prefix  (milliseconds since epoch, Base58-encoded)
//!   - 6-char monotonic counter (randomly seeded each millisecond, incremented)
//!   - 7-char random tail       (independently random per ID)
//!
//! IDs are strictly monotonically increasing within a single generator instance
//! (or thread, when using [`SparkId::new`]).
//!
//! # Examples
//!
//! ```
//! use sparkid::SparkId;
//!
//! // Simple usage — zero-allocation, thread-safe
//! let id = SparkId::new();
//! let s = id.as_str();
//! assert_eq!(s.len(), 21);
//! println!("{id}");                 // Display, no heap allocation
//! let owned: String = id.into();   // Into<String> when needed
//!
//! // Advanced usage — own generator instance
//! let mut gen = sparkid::IdGenerator::new();
//! let id = gen.next_id();
//! let s = id.as_str();
//! assert_eq!(s.len(), 21);
//! ```

#![no_std]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod generator;

pub use generator::{IdGenerator, ParseSparkIdError, SparkId, SparkIdStr};


