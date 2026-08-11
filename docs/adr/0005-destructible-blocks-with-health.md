# ADR 0005: Destructible blocks with hit points

**Status:** Accepted (2026-08-07)
**Amends:** `PRODUCT_SPEC.md` §4 (terrain), `adr/0002-character-kits.md` (ability terrain profiles)
**Decided by:** Product owner directive — "everything on the screen except the landscape background has hit points"

## Context

`TerrainMask` stores one `Material` byte per cell. A cell is solid or empty, full stop. There
is no notion of a damaged-but-standing block, and no way for two weapons to affect the same
terrain differently beyond crater radius.

The product requirement is that **destructible blocks have hit points, and remaining health
maps proportionally to remaining usable surface area** — a block at 40% health offers 40% of
the standing room it started with. Ammunition types affect terrain differently. Portals,
drones, and similar objects follow later as separate entities that reuse the same health
concept.

## Decision

### 1. A block is an entity; the cell mask stays the collision truth

```rust
pub struct TerrainBlock {
    pub id: u32,                 // stable, assigned at map load
    pub origin_cell_x: i32,      // inclusive
    pub origin_cell_y: i32,      // inclusive
    pub width_cells: u16,
    pub height_cells: u16,
    pub material: Material,
    pub health: u16,
    pub max_health: u16,
}
```

Blocks live in `SimulationState.blocks`, sorted by `id`.

**The cell mask remains the single source of truth for collision and rendering.** Block damage
recomputes which cells are solid; nothing that reads terrain changes. This is the property
that makes the whole change affordable: swept displacement, ballistic collision,
`is_solid_at`, crater operations, and the terrain section of the state hash all keep working
untouched.

### 2. Health maps to surface area by column erosion

The requirement is literal — *percent of health equals percent of usable surface*. Column
erosion satisfies it exactly:

```
solid_columns = ceil(width_cells * health / max_health)
```

`ceil`, so a block with any health above zero keeps at least one column. At zero health every
column is cleared and the block is gone.

**Which columns survive is decided by distance from the impact**, nearest eroded first, ties
broken by ascending `x`. A grenade landing on the right end eats the right end, which is what
a player expects to see. With no impact position (scripted or environmental damage), erosion
runs edge-inward from both sides so the block shrinks toward its centre.

Column erosion rather than perimeter erosion because the surface players care about is the
**walkable top edge**. Eroding a perimeter ring would delete the standing surface first and
leave an unreachable core — the opposite of "surface area you can still use."

**Erosion is a pure function of `(block, health, impact_x)`.** It never reads iteration order,
never uses the RNG, and is recomputed from health rather than accumulated — so replaying the
same damage sequence always yields the same mask, and a block's appearance can never drift
from its health.

### 3. Abilities carry a terrain damage profile

`TerrainProfile` currently describes *geometry* (crater, dig). Block damage is a separate
axis, so it is a separate field rather than a new variant:

```rust
pub struct BlockDamage {
    /// Damage dealt to each block within the terrain radius.
    pub amount: u16,
    /// Falloff from the impact point, as for character splash.
    pub falloff: bool,
    /// Materials this ammunition can damage at all.
    pub material_mask: MaterialMask,
}
```

This gives per-ammunition terrain behaviour directly: a drill does high single-block damage,
a cluster spreads low damage across several, and a weapon with `amount = u16::MAX` levels
everything — which is the launch default the owner asked for while the system is proven.

`MaterialMask` is reused rather than reinvented; reinforced stone already resists everything
but the Breach Pick, and that rule now governs block damage too.

## Consequences

**The state hash changes.** Blocks are gameplay state and must be encoded. `SIMULATION_VERSION`
increments. No completed matches exist, so nothing is invalidated.

**Two representations of solidity must not drift.** Cells are derived from block health, so
there is exactly one authority (health) and one derivation (erosion). Any code path that
writes cells inside a block's span *without* going through block damage reintroduces the drift
this design exists to prevent. Existing crater operations still write cells directly — that is
accepted for now, and §"Open" records it.

**Blocks are addressable.** Damage attribution, the result panel, and future portals and drones
all get a target with an id rather than an anonymous cell.

**Memory cost is negligible** — a handful of blocks per map versus half a million cells.

## Rejected alternative: per-cell hit points

Widening `cells` to `(Material, u8)` is conceptually simpler: a cell dies at zero, and block
health is the surviving fraction. Rejected because it doubles mask memory, changes the terrain
hash encoding for every cell, provides no addressable entity for the UI or damage attribution,
and — decisively — makes "40% health equals 40% surface area" an emergent accident of where
damage happened to land rather than a guaranteed property.

## Open

- **Crater operations still bypass block health.** An explosion currently clears cells directly.
  Until craters are routed through block damage, a block can be holed without losing health.
  Tracked as `todolist.md` P3.
- **Erosion direction is a default, not a law.** Edge-inward and impact-directed are both
  implemented; whether some ammunition should erode top-down (thinning a platform rather than
  shortening it) is a tuning question left open.
- **Partial blocks and blast occlusion interact** (`todolist.md` P7). If occlusion is ever
  implemented, an eroded block is partially transparent and needs its own rule.
