use core::fmt;
use core::ops::Deref;
use core::str::FromStr;

#[cfg(feature = "std")]
use std::time::{SystemTime, UNIX_EPOCH};

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;

use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};

// Base58 alphabet — excludes visually ambiguous characters (0, O, I, l)
const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BASE: u64 = 58;

// ID structure: [8-char timestamp][6-char counter][7-char random] = 21 chars
const COUNTER_CHAR_COUNT: usize = 6;
const RANDOM_CHAR_COUNT: usize = 7;
const ID_LENGTH: usize = 21;

// How many random bytes to fetch per batch. After rejection sampling,
// ~90.6% survive (58/64), yielding ~14848 valid chars (~2121 IDs).
const RANDOM_BATCH_SIZE: usize = 16384;

const FIRST_BYTE: u8 = ALPHABET[0]; // b'1'

// Timestamp encoding: remainder (0-57) -> single byte
const TIMESTAMP_LOOKUP: [u8; 58] = {
    let mut table = [0u8; 58];
    let mut i = 0;
    while i < 58 {
        table[i] = ALPHABET[i];
        i += 1;
    }
    table
};

// Random encoding: for each byte 0-255, mask to 6 bits (0-63).
// Values < 58 map to their alphabet byte; values >= 58 are rejected (0).
// No modulo bias.
const RANDOM_BYTE_LOOKUP: [u8; 256] = {
    let mut table = [0u8; 256];
    let mut byte = 0;
    while byte < 256 {
        let value = byte & 0x3f;
        if value < 58 {
            table[byte] = ALPHABET[value];
        }
        // else stays 0 (rejected)
        byte += 1;
    }
    table
};

// Successor table: byte -> next Base58 byte, or 0 if carry needed.
const SUCCESSOR: [u8; 123] = {
    let mut table = [0u8; 123]; // b'z' + 1
    let mut i = 0;
    while i < 57 {
        table[ALPHABET[i] as usize] = ALPHABET[i + 1];
        i += 1;
    }
    // Last char (b'z') stays 0 (carry)
    table
};

// Validation table: for each byte 0-255, true if it's in the Base58 alphabet.
const IS_BASE58: [bool; 256] = {
    let mut table = [false; 256];
    let mut i = 0;
    while i < 58 {
        table[ALPHABET[i] as usize] = true;
        i += 1;
    }
    table
};

/// The error returned when parsing a string as a [`SparkId`] fails.
///
/// Returned by [`SparkId::from_str`] and [`TryFrom<&str>`] when the input
/// is not a valid 21-char Base58 ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSparkIdError {
    kind: ParseErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParseErrorKind {
    InvalidLength(usize),
    InvalidChar { byte: u8, position: usize },
}

impl fmt::Display for ParseSparkIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ParseErrorKind::InvalidLength(len) => {
                write!(f, "invalid SparkId length: expected 21, got {len}")
            }
            ParseErrorKind::InvalidChar { byte, position } => {
                write!(
                    f,
                    "invalid character '{}' at position {position} in SparkId",
                    *byte as char
                )
            }
        }
    }
}

/// A unique, time-sortable, 21-char Base58 ID.
///
/// `SparkId` is a stack-allocated, `Copy` type that wraps `[u8; 21]`.
/// It dereferences to `&str` for zero-cost string access, and implements
/// `Display` for formatting without heap allocation.
///
/// # Examples
///
/// ```
/// use sparkid::SparkId;
///
/// let id = SparkId::new();
/// assert_eq!(id.len(), 21);
/// println!("{id}");              // Display, no allocation
/// let s: &str = &id;             // Deref to &str, no allocation
/// let owned: String = id.into(); // Into<String> when you need ownership
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SparkId([u8; ID_LENGTH]);

#[cfg(feature = "std")]
use std::cell::RefCell;

#[cfg(feature = "std")]
std::thread_local! {
    static LOCAL_GEN: RefCell<IdGenerator> = RefCell::new(IdGenerator::new());
}

impl SparkId {
    /// Generate a new unique, time-sortable ID.
    ///
    /// Thread-safe via thread-local storage. IDs are strictly monotonically
    /// increasing within each thread; across threads they are unique but unordered.
    ///
    /// # Examples
    ///
    /// ```
    /// let id = sparkid::SparkId::new();
    /// assert_eq!(id.len(), 21);
    /// ```
    #[cfg(feature = "std")]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        LOCAL_GEN.with(|generator| generator.borrow_mut().next_id())
    }

    fn as_str(&self) -> &str {
        // All bytes are ASCII Base58 characters, so this is always valid UTF-8.
        core::str::from_utf8(&self.0).expect("SparkId contains invalid UTF-8")
    }
}

impl Deref for SparkId {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for SparkId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for SparkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for SparkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SparkId({})", self.as_str())
    }
}

impl From<SparkId> for String {
    fn from(id: SparkId) -> String {
        id.as_str().to_owned()
    }
}

impl FromStr for SparkId {
    type Err = ParseSparkIdError;

    /// Parse a string as a `SparkId`.
    ///
    /// Validates that the input is exactly 21 ASCII bytes, all from the
    /// Base58 alphabet (`1-9`, `A-H`, `J-N`, `P-Z`, `a-k`, `m-z`).
    ///
    /// # Examples
    ///
    /// ```
    /// use sparkid::SparkId;
    ///
    /// let id = SparkId::new();
    /// let parsed: SparkId = id.to_string().parse().unwrap();
    /// assert_eq!(id, parsed);
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        if bytes.len() != ID_LENGTH {
            return Err(ParseSparkIdError {
                kind: ParseErrorKind::InvalidLength(bytes.len()),
            });
        }
        for (i, &b) in bytes.iter().enumerate() {
            if !IS_BASE58[b as usize] {
                return Err(ParseSparkIdError {
                    kind: ParseErrorKind::InvalidChar {
                        byte: b,
                        position: i,
                    },
                });
            }
        }
        // Safety: length already validated above, unwrap cannot fail.
        Ok(SparkId(bytes.try_into().unwrap()))
    }
}

impl<'a> TryFrom<&'a str> for SparkId {
    type Error = ParseSparkIdError;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

/// Generates 21-char, Base58, time-sortable, collision-resistant unique IDs.
///
/// Each ID is composed of three parts:
///   - 8-char timestamp prefix  (milliseconds, Base58-encoded, sortable)
///   - 6-char monotonic counter (randomly seeded each millisecond, incremented)
///   - 7-char random tail       (independently random per ID)
///
/// IDs are strictly monotonically increasing within a single generator instance:
/// across milliseconds by the timestamp prefix, and within the same millisecond
/// by incrementing the counter.
///
/// # Examples
///
/// ```
/// let mut gen = sparkid::IdGenerator::new();
/// let id = gen.next_id();
/// assert_eq!(id.len(), 21);
/// assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
/// ```
pub struct IdGenerator {
    timestamp_cache_ms: u64,
    // Full 21-byte ID buffer maintained in place.
    // [0..8]  = timestamp prefix
    // [8..13] = counter head
    // [13]    = counter tail
    // [14..21] = random tail (overwritten every call)
    id_buffer: [u8; ID_LENGTH],
    // Counter tail — kept as separate field for fast successor lookup
    counter_tail: u8,
    // Pre-sampled random bytes (valid Base58 bytes after rejection sampling)
    random_buffer: Box<[u8]>,
    random_count: usize,
    random_position: usize,
    // Raw buffer for random byte generation — avoids allocation on each refill
    raw_buffer: Box<[u8]>,
    // Userspace CSPRNG (ChaCha12, seeded from OS entropy)
    rng: StdRng,
    // Injectable time source for testing
    #[cfg(any(test, feature = "test-internals"))]
    time_function: Option<fn() -> u64>,
}

impl IdGenerator {
    /// Creates a new `IdGenerator` with fresh state.
    pub fn new() -> Self {
        IdGenerator {
            timestamp_cache_ms: 0,
            id_buffer: [FIRST_BYTE; ID_LENGTH],
            counter_tail: FIRST_BYTE,
            random_buffer: vec![0u8; RANDOM_BATCH_SIZE].into_boxed_slice(),
            random_count: 0,
            random_position: 0,
            raw_buffer: vec![0u8; RANDOM_BATCH_SIZE].into_boxed_slice(),
            rng: StdRng::from_os_rng(),
            #[cfg(any(test, feature = "test-internals"))]
            time_function: None,
        }
    }

    /// Generates a unique, time-sortable, 21-char Base58 ID.
    ///
    /// Returns a stack-allocated [`SparkId`] with no heap allocation.
    /// IDs are strictly monotonically increasing within this generator instance.
    ///
    /// Requires the `std` feature (enabled by default) for automatic timestamping.
    /// In `no_std` environments, use [`next_id_at`](Self::next_id_at) instead.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut gen = sparkid::IdGenerator::new();
    /// let id = gen.next_id();
    /// assert_eq!(id.len(), 21);
    /// println!("{id}"); // no allocation
    /// ```
    #[cfg(feature = "std")]
    pub fn next_id(&mut self) -> SparkId {
        self.advance(self.current_time_ms());
        SparkId(self.id_buffer)
    }

    /// Generates a unique, time-sortable, 21-char Base58 ID using the given
    /// timestamp (milliseconds since Unix epoch).
    ///
    /// This is the `no_std`-compatible alternative to [`next_id`](Self::next_id).
    /// The caller is responsible for providing an accurate, monotonically
    /// increasing timestamp. If the provided timestamp is less than the last
    /// seen timestamp, the generator treats it as a clock regression and
    /// increments the counter instead.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut gen = sparkid::IdGenerator::new();
    /// let id = gen.next_id_at(1_700_000_000_000);
    /// assert_eq!(id.len(), 21);
    /// ```
    pub fn next_id_at(&mut self, timestamp_ms: u64) -> SparkId {
        self.advance(timestamp_ms);
        SparkId(self.id_buffer)
    }

    /// Advance internal state and fill id_buffer with the next ID.
    fn advance(&mut self, timestamp: u64) {
        if timestamp > self.timestamp_cache_ms {
            // New millisecond (or first call): encode timestamp, seed counter.
            self.timestamp_cache_ms = timestamp;
            self.encode_timestamp(timestamp);
            self.seed_counter();
        } else {
            // Same millisecond (or clock went backward): increment counter tail.
            let next = SUCCESSOR[self.counter_tail as usize];
            if next != 0 {
                self.counter_tail = next;
            } else {
                self.increment_carry();
            }
        }

        // Ensure random buffer has enough bytes for the tail.
        if self.random_position + RANDOM_CHAR_COUNT > self.random_count {
            self.refill_random();
        }
        let position = self.random_position;
        self.random_position = position + RANDOM_CHAR_COUNT;

        // Hot path: only write counter tail (1 byte) + random (7 bytes).
        // The prefix (8) and counter head (5) are already up to date in id_buffer.
        self.id_buffer[13] = self.counter_tail;
        self.id_buffer[14..21].copy_from_slice(&self.random_buffer[position..position + RANDOM_CHAR_COUNT]);
    }

    #[cfg(feature = "std")]
    fn current_time_ms(&self) -> u64 {
        #[cfg(any(test, feature = "test-internals"))]
        if let Some(f) = self.time_function {
            return f();
        }
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_millis() as u64
    }

    fn encode_timestamp(&mut self, mut timestamp: u64) {
        let mut remainder: u64;

        remainder = timestamp % BASE; timestamp /= BASE;
        let c7 = TIMESTAMP_LOOKUP[remainder as usize];
        remainder = timestamp % BASE; timestamp /= BASE;
        let c6 = TIMESTAMP_LOOKUP[remainder as usize];
        remainder = timestamp % BASE; timestamp /= BASE;
        let c5 = TIMESTAMP_LOOKUP[remainder as usize];
        remainder = timestamp % BASE; timestamp /= BASE;
        let c4 = TIMESTAMP_LOOKUP[remainder as usize];
        remainder = timestamp % BASE; timestamp /= BASE;
        let c3 = TIMESTAMP_LOOKUP[remainder as usize];
        remainder = timestamp % BASE; timestamp /= BASE;
        let c2 = TIMESTAMP_LOOKUP[remainder as usize];
        remainder = timestamp % BASE; timestamp /= BASE;
        let c1 = TIMESTAMP_LOOKUP[remainder as usize];
        let c0 = TIMESTAMP_LOOKUP[timestamp as usize];

        self.id_buffer[0] = c0;
        self.id_buffer[1] = c1;
        self.id_buffer[2] = c2;
        self.id_buffer[3] = c3;
        self.id_buffer[4] = c4;
        self.id_buffer[5] = c5;
        self.id_buffer[6] = c6;
        self.id_buffer[7] = c7;
    }

    fn refill_random(&mut self) {
        self.rng.fill_bytes(&mut self.raw_buffer);
        let mut count = 0;
        for &byte in &*self.raw_buffer {
            let mapped = RANDOM_BYTE_LOOKUP[byte as usize];
            if mapped != 0 {
                self.random_buffer[count] = mapped;
                count += 1;
            }
        }
        self.random_count = count;
        self.random_position = 0;
    }

    fn seed_counter(&mut self) {
        if self.random_position + COUNTER_CHAR_COUNT > self.random_count {
            self.refill_random();
        }
        let position = self.random_position;
        self.random_position = position + COUNTER_CHAR_COUNT;

        self.id_buffer[8..13]
            .copy_from_slice(&self.random_buffer[position..position + 5]);
        self.counter_tail = self.random_buffer[position + 5];
    }

    /// Handle carry propagation through the counter head bytes.
    ///
    /// Called when the counter tail overflows. Walks backward through
    /// the counter head (positions 12 down to 8). On full overflow
    /// (all 6 counter chars maxed), bumps the timestamp forward by 1ms
    /// and reseeds. Because the counter is randomly seeded each ms,
    /// overflow probability is n / 58^6 for n IDs generated in that ms.
    fn increment_carry(&mut self) {
        for i in (8..=12).rev() {
            let next = SUCCESSOR[self.id_buffer[i] as usize];
            if next != 0 {
                self.id_buffer[i] = next;
                for j in (i + 1)..=12 {
                    self.id_buffer[j] = FIRST_BYTE;
                }
                self.counter_tail = FIRST_BYTE;
                return;
            }
        }
        // Full overflow: bump timestamp, reseed.
        self.timestamp_cache_ms += 1;
        self.encode_timestamp(self.timestamp_cache_ms);
        self.seed_counter();
    }
}

// Test-only methods — exposed behind `test-internals` feature for integration tests.
#[cfg(any(test, feature = "test-internals"))]
impl IdGenerator {
    /// Set an injectable time source (returns milliseconds since epoch).
    pub fn set_time_function(&mut self, f: fn() -> u64) {
        self.time_function = Some(f);
    }

    /// Clear the injectable time source, reverting to `SystemTime`.
    pub fn clear_time_function(&mut self) {
        self.time_function = None;
    }

    /// Read the prefix+counter_head buffer (first 13 bytes of id_buffer).
    pub fn prefix_plus_counter_head(&self) -> &[u8; 13] {
        self.id_buffer[..13].try_into().unwrap()
    }

    /// Read the counter tail byte.
    pub fn counter_tail(&self) -> u8 {
        self.counter_tail
    }

    /// Encode a timestamp into the prefix (test access).
    pub fn encode_timestamp_test(&mut self, timestamp: u64) {
        self.encode_timestamp(timestamp);
    }

    /// Seed the counter from the random buffer (test access).
    pub fn seed_counter_test(&mut self) {
        self.seed_counter();
    }

    /// Set the counter head bytes directly.
    pub fn set_counter_head(&mut self, bytes: &[u8; 5]) {
        self.id_buffer[8..13].copy_from_slice(bytes);
    }

    /// Set the counter tail byte directly.
    pub fn set_counter_tail(&mut self, byte: u8) {
        self.counter_tail = byte;
    }

    /// Set the cached timestamp.
    pub fn set_timestamp_cache_ms(&mut self, timestamp: u64) {
        self.timestamp_cache_ms = timestamp;
    }

    /// Read the cached timestamp.
    pub fn timestamp_cache_ms(&self) -> u64 {
        self.timestamp_cache_ms
    }

    /// Trigger carry propagation (test access).
    pub fn increment_carry_test(&mut self) {
        self.increment_carry();
    }

    /// Refill the random buffer (test access).
    pub fn refill_random_test(&mut self) {
        self.refill_random();
    }

    /// Read the valid portion of the random buffer.
    pub fn random_buffer_valid(&self) -> &[u8] {
        &self.random_buffer[..self.random_count]
    }

    /// Read the count of valid random bytes.
    pub fn random_count(&self) -> usize {
        self.random_count
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "std")]
impl Iterator for IdGenerator {
    type Item = SparkId;

    fn next(&mut self) -> Option<SparkId> {
        Some(self.next_id())
    }
}
