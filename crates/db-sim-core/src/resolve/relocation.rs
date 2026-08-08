//! Resolvers for Teleport, Relocate, and Obscure.
//!
//! Placeholder. Owned by the relocation task; see `docs/MODULE_OWNERSHIP.md`.

use crate::error::SimResult;
use crate::types::SpecialEffect;

use super::ResolveContext;

/// Resolves a relocation-family effect.
///
/// # Errors
///
/// Returns an error when the effect's parameters are out of range or a fixed-point
/// computation would overflow.
pub fn resolve(_ctx: &mut ResolveContext<'_>, _effect: &SpecialEffect) -> SimResult<()> {
    Ok(())
}
