use core::fmt;
use core::ops::Deref;
use core::str::FromStr;

#[cfg(feature = "std")]
use std::time::{SystemTime, UNIX_EPOCH};

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;

use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};

// Base58 alphabet — excludes visually ambiguous characters (0, O, I, l)
const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BASE: u64 = ALPHABET.len() as u64;
const BASE_USIZE: usize = ALPHABET.len();

// ID structure: [8-char timestamp][6-char counter][7-char random] = 21 chars
const TIMESTAMP_CHAR_COUNT: usize = 8;
const COUNTER_CHAR_COUNT: usize = 6;
const RANDOM_CHAR_COUNT: usize = 7;
const ID_LENGTH: usize = TIMESTAMP_CHAR_COUNT + COUNTER_CHAR_COUNT + RANDOM_CHAR_COUNT;

// How many random bytes to fetch per batch. After rejection sampling,
// ~90.6% survive (58/64), yielding ~14848 valid chars (~2121 IDs).
const RANDOM_BATCH_SIZE: usize = 16384;

// Index-space constants (indices 0-57 into the Base58 alphabet)
const FIRST_INDEX: u8 = 0;
const MAX_INDEX: u8 = 57;
const INVALID_INDEX: u8 = 0xFF;

// Number of 6-bit fields packed into the u128 binary representation
const PACKED_BYTE_COUNT: usize = 16;

// Derived layout constants
const COUNTER_HEAD_CHAR_COUNT: usize = COUNTER_CHAR_COUNT - 1;
const COUNTER_TAIL_OFFSET: usize = TIMESTAMP_CHAR_COUNT + COUNTER_HEAD_CHAR_COUNT;
const PREFIX_INDEX_COUNT: usize = COUNTER_TAIL_OFFSET; // timestamp (8) + counter head (5)

// Reverse-lookup table: ASCII byte -> Base58 index (0-57), or 0xFF for invalid.
const DECODE: [u8; 256] = {
    let mut table = [INVALID_INDEX; 256];
    let mut i: usize = 0;
    while i < BASE_USIZE {
        table[ALPHABET[i] as usize] = i as u8;
        i += 1;
    }
    table
};

/// Pack the first 13 indices (timestamp + counter head) into the upper
/// portion of a `u128`, occupying bits 127..50 with bits 49..0 zeroed.
///
/// Used to cache the slow-changing prefix so the hot path only packs the
/// 8 fast-changing suffix indices (counter tail + random) using `u64`.
fn pack_prefix(indices: &[u8; PREFIX_INDEX_COUNT]) -> u128 {
    let mut value: u128 = 0;
    for &index in indices {
        value = (value << 6) | index as u128;
    }
    // Shift to position: 13 indices × 6 bits = 78 bits of data.
    // The lowest index (index 12) needs to land at bits 55..50,
    // so shift left by 50.
    value << 50
}

/// Pack the suffix (counter tail + 7 random indices) into the lower 50 bits
/// of a `u128`. Uses `u64` arithmetic since the result fits in 50 bits,
/// avoiding expensive u128 shift/OR chains on the hot path.
#[inline(always)]
fn pack_suffix(counter_tail: u8, random: &[u8]) -> u128 {
    let mut value: u64 = counter_tail as u64;
    value = (value << 6) | random[0] as u64;
    value = (value << 6) | random[1] as u64;
    value = (value << 6) | random[2] as u64;
    value = (value << 6) | random[3] as u64;
    value = (value << 6) | random[4] as u64;
    value = (value << 6) | random[5] as u64;
    // Last index + 2 padding bits
    value = (value << 8) | (random[6] as u64) << 2;
    value as u128
}

/// The error returned when parsing a string or binary representation
/// as a [`SparkId`] fails.
///
/// Returned by [`SparkId::from_str`], [`TryFrom<&str>`], and
/// [`SparkId::from_bytes`] when the input is not a valid SparkId.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSparkIdError {
    kind: ParseErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
enum ParseErrorKind {
    InvalidLength(usize),
    InvalidChar { byte: u8, position: usize },
    InvalidBinaryIndex { value: u8, byte_position: usize },
    InvalidPadding,
}

impl fmt::Display for ParseSparkIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ParseErrorKind::InvalidLength(length) => {
                write!(f, "invalid SparkId length: expected 21, got {length}")
            }
            ParseErrorKind::InvalidChar { byte, position } => {
                write!(
                    f,
                    "invalid character '{}' at position {position} in SparkId",
                    *byte as char
                )
            }
            ParseErrorKind::InvalidBinaryIndex {
                value,
                byte_position,
            } => {
                write!(
                    f,
                    "invalid 6-bit index {value} at byte {byte_position} in binary SparkId"
                )
            }
            ParseErrorKind::InvalidPadding => {
                write!(f, "non-zero padding bits in binary SparkId")
            }
        }
    }
}

/// A unique, time-sortable, Base58-encoded ID stored as a `u128`.
///
/// `SparkId` is a stack-allocated, `Copy` type that wraps a `u128`.
/// The 21 Base58 characters (each a 6-bit index, 0-57) are bit-packed
/// into the 128-bit value, preserving lexicographic sort order.
///
/// Use [`as_str`](SparkId::as_str) to obtain a stack-allocated
/// [`SparkIdStr`] for zero-allocation string access.
///
/// # Examples
///
/// ```
/// use sparkid::SparkId;
///
/// let id = SparkId::new();
/// let s = id.as_str();             // Stack-allocated string, no heap
/// println!("{id}");                 // Display, no allocation
/// let owned: String = id.into();   // Into<String> when you need ownership
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SparkId(u128);

/// A stack-allocated, 21-byte ASCII representation of a [`SparkId`].
///
/// Obtained via [`SparkId::as_str`]. Dereferences to `&str` for
/// zero-cost string access.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SparkIdStr([u8; ID_LENGTH]);

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
    /// let s = id.as_str();
    /// assert_eq!(s.len(), 21);
    /// ```
    #[cfg(feature = "std")]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        LOCAL_GEN.with(|generator| generator.borrow_mut().next_id())
    }

    /// Returns the inner `u128` binary representation.
    pub fn as_u128(&self) -> u128 {
        self.0
    }

    /// Construct a `SparkId` from its `u128` binary representation.
    ///
    /// Validates that every 6-bit field is in the range `[0, 57]` and that
    /// the 2 padding bits are zero. This is the inverse of [`as_u128`](Self::as_u128).
    pub fn from_u128(value: u128) -> Result<Self, ParseSparkIdError> {
        Self::from_bytes(value.to_be_bytes())
    }

    /// Returns the 16-byte big-endian binary representation.
    pub fn to_bytes(&self) -> [u8; PACKED_BYTE_COUNT] {
        self.0.to_be_bytes()
    }

    /// Construct a `SparkId` from its 16-byte big-endian binary representation.
    ///
    /// Validates that every 6-bit field is in the range `[0, 57]` and that
    /// the 2 padding bits in the last byte are zero.
    pub fn from_bytes(bytes: [u8; PACKED_BYTE_COUNT]) -> Result<Self, ParseSparkIdError> {
        let value = u128::from_be_bytes(bytes);

        // Validate padding: the bottom 2 bits must be zero.
        if value & 0x03 != 0 {
            return Err(ParseSparkIdError {
                kind: ParseErrorKind::InvalidPadding,
            });
        }

        // Validate all 21 six-bit fields are in [0, 57].
        let mut shift = 122i32;
        let mut char_index = 0usize;
        while char_index < ID_LENGTH {
            let index = ((value >> shift as u32) & 0x3F) as u8;
            if index > MAX_INDEX {
                return Err(ParseSparkIdError {
                    kind: ParseErrorKind::InvalidBinaryIndex {
                        value: index,
                        byte_position: (char_index * 6) / 8,
                    },
                });
            }
            shift -= 6;
            char_index += 1;
        }

        Ok(SparkId(value))
    }

    /// Returns a stack-allocated [`SparkIdStr`] containing the 21-char
    /// Base58 string representation.
    pub fn as_str(&self) -> SparkIdStr {
        SparkIdStr::from_packed(self.0)
    }

    /// Returns the embedded timestamp as milliseconds since the Unix epoch.
    ///
    /// Unpacks the first 8 Base58 indices from the binary representation
    /// and reconstructs the original timestamp via Horner's method.
    pub fn timestamp_ms(&self) -> u64 {
        // Extract the first 8 six-bit indices directly from the u128.
        // Index 0 is at bits 127..122, index 1 at 121..116, etc.
        let v = self.0;
        let index_0 = ((v >> 122) & 0x3F) as u64;
        let index_1 = ((v >> 116) & 0x3F) as u64;
        let index_2 = ((v >> 110) & 0x3F) as u64;
        let index_3 = ((v >> 104) & 0x3F) as u64;
        let index_4 = ((v >> 98) & 0x3F) as u64;
        let index_5 = ((v >> 92) & 0x3F) as u64;
        let index_6 = ((v >> 86) & 0x3F) as u64;
        let index_7 = ((v >> 80) & 0x3F) as u64;

        // Horner's method: val = i0*58^7 + i1*58^6 + ... + i7*58^0
        let mut value: u64 = index_0;
        value = value * BASE + index_1;
        value = value * BASE + index_2;
        value = value * BASE + index_3;
        value = value * BASE + index_4;
        value = value * BASE + index_5;
        value = value * BASE + index_6;
        value = value * BASE + index_7;
        value
    }

    /// Returns the embedded timestamp as a [`SystemTime`].
    ///
    /// # Examples
    ///
    /// ```
    /// let id = sparkid::SparkId::new();
    /// let ts = id.timestamp();
    /// ```
    #[cfg(feature = "std")]
    pub fn timestamp(&self) -> SystemTime {
        UNIX_EPOCH + std::time::Duration::from_millis(self.timestamp_ms())
    }
}

// ---------------------------------------------------------------------------
// SparkIdStr impls
// ---------------------------------------------------------------------------

impl SparkIdStr {
    pub(crate) fn from_packed(value: u128) -> Self {
        let mut out = [0u8; ID_LENGTH];
        let mut shift = 122i32; // first index at bits 127..122
        let mut i = 0;
        while i < 21 {
            out[i] = ALPHABET[((value >> shift as u32) & 0x3F) as usize];
            shift -= 6;
            i += 1;
        }
        Self(out)
    }

    fn as_str_inner(&self) -> &str {
        // SAFETY: from_packed only produces bytes from ALPHABET, which are ASCII
        // and therefore always valid UTF-8. Skips the redundant validation scan.
        unsafe { core::str::from_utf8_unchecked(&self.0) }
    }
}

impl Deref for SparkIdStr {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str_inner()
    }
}

impl AsRef<str> for SparkIdStr {
    fn as_ref(&self) -> &str {
        self.as_str_inner()
    }
}

impl fmt::Display for SparkIdStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str_inner())
    }
}

impl fmt::Debug for SparkIdStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SparkIdStr({})", self.as_str_inner())
    }
}

// ---------------------------------------------------------------------------
// SparkId trait impls
// ---------------------------------------------------------------------------

impl fmt::Display for SparkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_str())
    }
}

impl fmt::Debug for SparkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SparkId({})", &self.as_str())
    }
}

impl From<SparkId> for String {
    fn from(id: SparkId) -> String {
        let s = id.as_str();
        String::from(&*s)
    }
}

impl FromStr for SparkId {
    type Err = ParseSparkIdError;

    /// Parse a string as a `SparkId`.
    ///
    /// Validates that the input is exactly 21 ASCII bytes, all from the
    /// Base58 alphabet (`1-9`, `A-H`, `J-N`, `P-Z`, `a-k`, `m-z`),
    /// then packs the indices into a `u128`.
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
        let mut value: u128 = 0;
        let mut position = 0;
        while position < 20 {
            let index = DECODE[bytes[position] as usize];
            if index == INVALID_INDEX {
                return Err(ParseSparkIdError {
                    kind: ParseErrorKind::InvalidChar {
                        byte: bytes[position],
                        position,
                    },
                });
            }
            value = (value << 6) | index as u128;
            position += 1;
        }
        let index = DECODE[bytes[20] as usize];
        if index == INVALID_INDEX {
            return Err(ParseSparkIdError {
                kind: ParseErrorKind::InvalidChar {
                    byte: bytes[20],
                    position: 20,
                },
            });
        }
        value = (value << 8) | (index as u128) << 2;
        Ok(SparkId(value))
    }
}

impl<'a> TryFrom<&'a str> for SparkId {
    type Error = ParseSparkIdError;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

// ---------------------------------------------------------------------------
// serde impls
// ---------------------------------------------------------------------------

#[cfg(feature = "serde")]
mod serde_support {
    use super::{SparkId, SparkIdStr, ID_LENGTH, PACKED_BYTE_COUNT};
    use core::fmt;
    use core::str::FromStr;
    use serde::de::{self, Visitor};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    // -- SparkId --------------------------------------------------------------

    impl Serialize for SparkId {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            if serializer.is_human_readable() {
                serializer.serialize_str(self.as_str().as_str_inner())
            } else {
                let bytes = self.0.to_be_bytes();
                serializer.serialize_bytes(&bytes)
            }
        }
    }

    impl<'de> Deserialize<'de> for SparkId {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct SparkIdVisitor;

            impl<'de> Visitor<'de> for SparkIdVisitor {
                type Value = SparkId;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    write!(
                        formatter,
                        "a {ID_LENGTH}-char Base58 string or {PACKED_BYTE_COUNT}-byte binary SparkId"
                    )
                }

                fn visit_str<E: de::Error>(self, value: &str) -> Result<SparkId, E> {
                    SparkId::from_str(value).map_err(de::Error::custom)
                }

                fn visit_bytes<E: de::Error>(self, value: &[u8]) -> Result<SparkId, E> {
                    let bytes: [u8; PACKED_BYTE_COUNT] =
                        value.try_into().map_err(|_| {
                            de::Error::invalid_length(
                                value.len(),
                                &"16 bytes for binary SparkId",
                            )
                        })?;
                    SparkId::from_bytes(bytes).map_err(de::Error::custom)
                }
            }

            if deserializer.is_human_readable() {
                deserializer.deserialize_str(SparkIdVisitor)
            } else {
                deserializer.deserialize_bytes(SparkIdVisitor)
            }
        }
    }

    // -- SparkIdStr -----------------------------------------------------------

    impl Serialize for SparkIdStr {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_str(self.as_str_inner())
        }
    }

    impl<'de> Deserialize<'de> for SparkIdStr {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct SparkIdStrVisitor;

            impl<'de> Visitor<'de> for SparkIdStrVisitor {
                type Value = SparkIdStr;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    write!(formatter, "a {ID_LENGTH}-char Base58 SparkId string")
                }

                fn visit_str<E: de::Error>(self, value: &str) -> Result<SparkIdStr, E> {
                    let id = SparkId::from_str(value).map_err(de::Error::custom)?;
                    Ok(id.as_str())
                }
            }

            deserializer.deserialize_str(SparkIdStrVisitor)
        }
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
/// Internally, the generator maintains an index buffer where each element is a
/// Base58 index (0-57) rather than an ASCII byte. The final `SparkId` is
/// produced by packing these indices into a `u128`.
///
/// # Examples
///
/// ```
/// let mut gen = sparkid::IdGenerator::new();
/// let id = gen.next_id();
/// let s = id.as_str();
/// assert_eq!(s.len(), 21);
/// assert!(s.chars().all(|c| c.is_ascii_alphanumeric()));
/// ```
pub struct IdGenerator {
    timestamp_cache_ms: u64,
    // Indices for timestamp (0..8) and counter head (8..13), used for carry
    // propagation. The counter tail and random tail are stored separately.
    index_buffer: [u8; PREFIX_INDEX_COUNT],
    // Counter tail — kept as separate field for fast increment
    counter_tail: u8,
    // Cached packed u128 of indices 0-12 (bits 127..50). Recomputed only when
    // timestamp or counter head changes (~once per ms), avoiding 13 u128
    // shift+OR operations on the hot path.
    cached_prefix: u128,
    // Pre-sampled random indices (valid Base58 indices after rejection sampling)
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
            index_buffer: [FIRST_INDEX; PREFIX_INDEX_COUNT],
            counter_tail: FIRST_INDEX,
            cached_prefix: 0,
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
    /// let s = id.as_str();
    /// assert_eq!(s.len(), 21);
    /// println!("{id}"); // no allocation
    /// ```
    #[cfg(feature = "std")]
    pub fn next_id(&mut self) -> SparkId {
        let random_position = self.prepare_next(self.current_time_ms());
        // SAFETY: advance() guarantees random_position + RANDOM_CHAR_COUNT <= random_count,
        // and random_count <= random_buffer.len() (RANDOM_BATCH_SIZE).
        let random = unsafe {
            self.random_buffer
                .get_unchecked(random_position..random_position + RANDOM_CHAR_COUNT)
        };
        SparkId(self.cached_prefix | pack_suffix(self.counter_tail, random))
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
    /// let s = id.as_str();
    /// assert_eq!(s.len(), 21);
    /// ```
    pub fn next_id_at(&mut self, timestamp_ms: u64) -> SparkId {
        let random_position = self.prepare_next(timestamp_ms);
        // SAFETY: advance() guarantees random_position + RANDOM_CHAR_COUNT <= random_count,
        // and random_count <= random_buffer.len() (RANDOM_BATCH_SIZE).
        let random = unsafe {
            self.random_buffer
                .get_unchecked(random_position..random_position + RANDOM_CHAR_COUNT)
        };
        SparkId(self.cached_prefix | pack_suffix(self.counter_tail, random))
    }

    /// Advance internal state for the next ID. Returns the random buffer
    /// position for the 7 random tail indices.
    #[inline(always)]
    fn prepare_next(&mut self, timestamp: u64) -> usize {
        if timestamp > self.timestamp_cache_ms {
            // New millisecond (or first call): encode timestamp, seed counter.
            self.timestamp_cache_ms = timestamp;
            self.encode_timestamp(timestamp);
            self.seed_counter();
        } else {
            // Same millisecond (or clock went backward): increment counter tail.
            if self.counter_tail < MAX_INDEX {
                self.counter_tail += 1;
            } else {
                self.increment_carry();
            }
        }

        // Ensure random buffer has enough indices for the tail.
        if self.random_position + RANDOM_CHAR_COUNT > self.random_count {
            self.refill_random();
        }
        let position = self.random_position;
        self.random_position = position + RANDOM_CHAR_COUNT;
        position
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

    #[cold]
    fn encode_timestamp(&mut self, mut timestamp: u64) {
        let mut remainder: u64;

        remainder = timestamp % BASE; timestamp /= BASE;
        let c7 = remainder as u8;
        remainder = timestamp % BASE; timestamp /= BASE;
        let c6 = remainder as u8;
        remainder = timestamp % BASE; timestamp /= BASE;
        let c5 = remainder as u8;
        remainder = timestamp % BASE; timestamp /= BASE;
        let c4 = remainder as u8;
        remainder = timestamp % BASE; timestamp /= BASE;
        let c3 = remainder as u8;
        remainder = timestamp % BASE; timestamp /= BASE;
        let c2 = remainder as u8;
        remainder = timestamp % BASE; timestamp /= BASE;
        let c1 = remainder as u8;
        let c0 = timestamp as u8;

        self.index_buffer[0] = c0;
        self.index_buffer[1] = c1;
        self.index_buffer[2] = c2;
        self.index_buffer[3] = c3;
        self.index_buffer[4] = c4;
        self.index_buffer[5] = c5;
        self.index_buffer[6] = c6;
        self.index_buffer[7] = c7;
    }

    #[cold]
    fn refill_random(&mut self) {
        self.rng.fill_bytes(&mut self.raw_buffer);
        let mut count = 0;
        for &byte in &*self.raw_buffer {
            let value = byte & 0x3F;
            // Branchless: always write, only advance count when valid.
            // Avoids branch mispredictions (~9.4% reject rate).
            // Safe because count <= loop iteration < RANDOM_BATCH_SIZE.
            self.random_buffer[count] = value;
            count += (value < BASE_USIZE as u8) as usize;
        }
        self.random_count = count;
        self.random_position = 0;
    }

    #[cold]
    fn seed_counter(&mut self) {
        if self.random_position + COUNTER_CHAR_COUNT > self.random_count {
            self.refill_random();
        }
        let position = self.random_position;
        self.random_position = position + COUNTER_CHAR_COUNT;

        self.index_buffer[TIMESTAMP_CHAR_COUNT..COUNTER_TAIL_OFFSET]
            .copy_from_slice(&self.random_buffer[position..position + COUNTER_HEAD_CHAR_COUNT]);
        self.counter_tail = self.random_buffer[position + COUNTER_HEAD_CHAR_COUNT];
        self.cached_prefix = pack_prefix(&self.index_buffer);
    }

    /// Handle carry propagation through the counter head indices.
    ///
    /// Called when the counter tail overflows. Walks backward through
    /// the counter head (positions 12 down to 8). On full overflow
    /// (all 6 counter indices maxed), bumps the timestamp forward by 1ms
    /// and reseeds. Because the counter is randomly seeded each ms,
    /// overflow probability is n / 58^6 for n IDs generated in that ms.
    #[cold]
    fn increment_carry(&mut self) {
        for i in (TIMESTAMP_CHAR_COUNT..COUNTER_TAIL_OFFSET).rev() {
            if self.index_buffer[i] < MAX_INDEX {
                self.index_buffer[i] += 1;
                self.counter_tail = FIRST_INDEX;
                self.cached_prefix = pack_prefix(&self.index_buffer);
                return;
            }
            self.index_buffer[i] = FIRST_INDEX;
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

    /// Read the prefix buffer (timestamp + counter head indices).
    ///
    /// Note: these are Base58 index values (0-57), not ASCII bytes.
    pub fn timestamp_and_counter_head(&self) -> &[u8; PREFIX_INDEX_COUNT] {
        &self.index_buffer
    }

    /// Read the counter tail index value.
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

    /// Set the counter head indices directly (values 0-57).
    pub fn set_counter_head(&mut self, indices: &[u8; 5]) {
        self.index_buffer[TIMESTAMP_CHAR_COUNT..COUNTER_TAIL_OFFSET].copy_from_slice(indices);
    }

    /// Set the counter tail index directly (value 0-57).
    pub fn set_counter_tail(&mut self, index: u8) {
        self.counter_tail = index;
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

    /// Read the count of valid random indices.
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
