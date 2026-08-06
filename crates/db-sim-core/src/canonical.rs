//! Canonical byte encoding and state hashing.
//!
//! ADR 0001 §5: the state hash must not depend on `JSON.stringify`. JavaScript's number
//! formatting, key escaping, and Unicode handling are engine semantics that Rust will not
//! reproduce by accident, so both implementations encode to an explicit, versioned,
//! length-prefixed byte stream and hash *that*.
//!
//! # The encoding
//!
//! | Kind | Bytes |
//! |---|---|
//! | `u8` / `i8` | 1 byte |
//! | `u32` / `i32` | 4 bytes, little-endian, two's complement |
//! | `u64` / `i64` | 8 bytes, little-endian, two's complement |
//! | `bool` | 1 byte, `0x00` or `0x01` |
//! | string | `u32` byte length, then UTF-8 bytes. No escaping, no terminator |
//! | sequence | `u32` element count, then each element |
//! | raw bytes | `u32` byte length, then the bytes |
//!
//! Every collection is written in a **stated sort order**. Iterating a hash map and
//! hashing the result would make the hash depend on allocator behaviour; the ordering
//! rule is part of the format.
//!
//! The length prefix on every variable-width field is what makes the encoding
//! unambiguous. Without it, `("ab", "c")` and `("a", "bc")` would produce identical bytes
//! and therefore collide — a real vulnerability in a hash used to detect state divergence.

/// Version of the canonical encoding. Any change to the byte layout increments this and
/// forces a `SIMULATION_VERSION` bump, because old hashes become uncomparable.
pub const CANONICAL_ENCODING_VERSION: u32 = 1;

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET_BASIS: u64 = 14_695_981_039_346_656_037;

/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 1_099_511_628_211;

/// Accumulates a canonical byte stream and folds it into an FNV-1a 64-bit hash.
///
/// Hashing is streaming: bytes are folded as they are written rather than buffered, so
/// hashing a large terrain mask does not allocate a copy of it.
#[derive(Debug, Clone)]
pub struct CanonicalHasher {
    state: u64,
}

impl Default for CanonicalHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl CanonicalHasher {
    /// Creates a hasher seeded with the encoding version.
    ///
    /// Mixing the version in at construction means two states that are byte-identical
    /// under different encoding versions still hash differently, so a format change can
    /// never be mistaken for agreement.
    #[must_use]
    pub fn new() -> Self {
        let mut hasher = Self {
            state: FNV_OFFSET_BASIS,
        };
        hasher.write_u32(CANONICAL_ENCODING_VERSION);
        hasher
    }

    /// Folds a single byte into the hash.
    pub const fn write_u8(&mut self, value: u8) {
        self.state ^= value as u64;
        self.state = self.state.wrapping_mul(FNV_PRIME);
    }

    /// Folds raw bytes into the hash without a length prefix.
    ///
    /// Only for callers that have already written an unambiguous length. Prefer
    /// [`Self::write_bytes`].
    pub fn write_raw(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u8(byte);
        }
    }

    /// Writes a length-prefixed byte block.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_length(bytes.len());
        self.write_raw(bytes);
    }

    /// Writes a 32-bit unsigned value, little-endian.
    pub fn write_u32(&mut self, value: u32) {
        self.write_raw(&value.to_le_bytes());
    }

    /// Writes a 32-bit signed value, little-endian two's complement.
    pub fn write_i32(&mut self, value: i32) {
        self.write_raw(&value.to_le_bytes());
    }

    /// Writes a 64-bit unsigned value, little-endian.
    pub fn write_u64(&mut self, value: u64) {
        self.write_raw(&value.to_le_bytes());
    }

    /// Writes a 64-bit signed value, little-endian two's complement.
    pub fn write_i64(&mut self, value: i64) {
        self.write_raw(&value.to_le_bytes());
    }

    /// Writes a boolean as `0x00` or `0x01`.
    pub const fn write_bool(&mut self, value: bool) {
        self.write_u8(if value { 1 } else { 0 });
    }

    /// Writes a length-prefixed UTF-8 string.
    pub fn write_str(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
    }

    /// Writes a sequence element count.
    ///
    /// Call once before writing the elements of any collection. The caller is responsible
    /// for writing elements in the collection's stated sort order.
    pub fn write_length(&mut self, length: usize) {
        // Saturate rather than truncate: a length above u32::MAX cannot occur in a valid
        // match, and saturating keeps the encoding total instead of silently aliasing.
        let truncated = u32::try_from(length).unwrap_or(u32::MAX);
        self.write_u32(truncated);
    }

    /// Writes a domain separator tag.
    ///
    /// Separates structurally different sections that could otherwise produce identical
    /// byte runs — for example, metadata immediately followed by terrain cells.
    pub const fn write_domain_separator(&mut self, tag: u8) {
        self.write_u8(0xff);
        self.write_u8(tag);
    }

    /// Consumes the hasher and returns the 64-bit digest.
    #[must_use]
    pub const fn finish(self) -> u64 {
        self.state
    }

    /// Consumes the hasher and returns the digest as lowercase zero-padded hex.
    ///
    /// This is the wire format for `finalStateHash`.
    #[must_use]
    pub fn finish_hex(self) -> String {
        format!("{:016x}", self.state)
    }
}

/// Domain separator tags. Each structurally distinct section of the state gets one.
pub mod domain {
    /// Match-level metadata: versions, tick, turn, active player, wind.
    pub const METADATA: u8 = 0x01;
    /// The player collection.
    pub const PLAYERS: u8 = 0x02;
    /// Terrain dimensions and cells.
    pub const TERRAIN: u8 = 0x03;
    /// Processed command identifiers.
    pub const COMMANDS: u8 = 0x04;
}

/// A value with a stable canonical byte representation.
pub trait Canonical {
    /// Writes this value's canonical bytes into `hasher`.
    ///
    /// Implementations must write every field that affects gameplay, must write
    /// collections in a stated order, and must **not** write cosmetic identifiers —
    /// appearance never affects the authoritative hash.
    fn write_canonical(&self, hasher: &mut CanonicalHasher);
}

impl Canonical for str {
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_str(self);
    }
}

impl Canonical for i32 {
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_i32(*self);
    }
}

impl Canonical for u32 {
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_u32(*self);
    }
}

impl<T: Canonical> Canonical for [T] {
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_length(self.len());
        for item in self {
            item.write_canonical(hasher);
        }
    }
}

/// Hashes a single [`Canonical`] value, returning hex.
#[must_use]
pub fn hash_canonical<T: Canonical + ?Sized>(value: &T) -> String {
    let mut hasher = CanonicalHasher::new();
    value.write_canonical(&mut hasher);
    hasher.finish_hex()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_prefixing_prevents_concatenation_collisions() {
        // The classic ambiguity: without length prefixes, ("ab","c") and ("a","bc")
        // encode to the same bytes. A colliding state hash would silently mask real
        // divergence between client and server.
        let mut left = CanonicalHasher::new();
        left.write_str("ab");
        left.write_str("c");

        let mut right = CanonicalHasher::new();
        right.write_str("a");
        right.write_str("bc");

        assert_ne!(left.finish(), right.finish());
    }

    #[test]
    fn empty_and_absent_strings_differ() {
        let mut with_empty = CanonicalHasher::new();
        with_empty.write_str("");

        let baseline = CanonicalHasher::new();

        assert_ne!(with_empty.finish(), baseline.finish());
    }

    #[test]
    fn domain_separators_split_adjacent_sections() {
        let mut separated = CanonicalHasher::new();
        separated.write_u8(1);
        separated.write_domain_separator(domain::TERRAIN);
        separated.write_u8(2);

        let mut fused = CanonicalHasher::new();
        fused.write_u8(1);
        fused.write_u8(2);

        assert_ne!(separated.finish(), fused.finish());
    }

    #[test]
    fn version_is_mixed_in_at_construction() {
        let hasher = CanonicalHasher::new();
        assert_ne!(hasher.finish(), FNV_OFFSET_BASIS);
    }

    #[test]
    fn hex_digest_is_zero_padded_to_sixteen() {
        let hasher = CanonicalHasher::new();
        assert_eq!(hasher.finish_hex().len(), 16);
    }

    #[test]
    fn signed_values_use_twos_complement_little_endian() {
        let mut negative = CanonicalHasher::new();
        negative.write_i32(-1);

        let mut all_ones = CanonicalHasher::new();
        all_ones.write_raw(&[0xff, 0xff, 0xff, 0xff]);

        assert_eq!(negative.finish(), all_ones.finish());
    }

    #[test]
    fn sequence_encoding_is_order_sensitive() {
        let ascending: [i32; 3] = [1, 2, 3];
        let descending: [i32; 3] = [3, 2, 1];
        assert_ne!(
            hash_canonical(ascending.as_slice()),
            hash_canonical(descending.as_slice())
        );
    }
}
