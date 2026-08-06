//! Error types for the simulation core.
//!
//! The core never panics on untrusted input (`SECURITY_BASELINE.md` §6). Every fallible
//! operation returns a `Result` carrying one of these variants, and the room process
//! turns a rejection into a categorized security or gameplay event rather than a crash.

use core::fmt;

/// A failure inside the authoritative simulation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimError {
    /// A definition, map, or state field fell outside its permitted range.
    OutOfRange {
        /// Which field.
        field: &'static str,
    },
    /// Fixed-point arithmetic would have overflowed.
    ///
    /// Surfaced rather than wrapped: a wrapped coordinate is a teleporting player.
    Overflow {
        /// Which computation.
        context: &'static str,
    },
    /// A referenced character, ability, map, or behavior identifier does not exist.
    UnknownDefinition,
    /// A terrain buffer's length disagrees with its declared dimensions.
    TerrainShapeMismatch,
    /// A character definition failed validation.
    InvalidCharacter {
        /// Why the character was rejected.
        reason: CharacterRejection,
    },
    /// The content roster failed its self-consistency check at load time.
    InvalidRoster,
}

/// Why a character definition was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterRejection {
    /// An ability was declared for a slot it does not belong to.
    SlotMismatch,
    /// The character does not offer exactly `PASSIVES_PER_CHARACTER` passives.
    WrongPassiveCount,
    /// Two abilities or passives share an identifier.
    DuplicateId,
    /// A referenced character identifier is not in the roster.
    UnknownCharacter,
    /// A stat fell outside its permitted range.
    StatOutOfRange,
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { field } => write!(f, "value out of range: {field}"),
            Self::Overflow { context } => write!(f, "fixed-point overflow in {context}"),
            Self::UnknownDefinition => f.write_str("unknown definition identifier"),
            Self::TerrainShapeMismatch => {
                f.write_str("terrain buffer length does not match its dimensions")
            }
            Self::InvalidCharacter { reason } => write!(f, "invalid character: {reason:?}"),
            Self::InvalidRoster => f.write_str("character roster failed validation"),
        }
    }
}

impl core::error::Error for SimError {}

/// Result alias for simulation operations.
pub type SimResult<T> = Result<T, SimError>;
