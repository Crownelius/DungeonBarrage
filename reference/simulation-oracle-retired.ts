/**
 * Pure, deterministic gameplay primitives for Dungeon Barrage.
 *
 * This module deliberately has no DOM, renderer, timer, or Node-only dependencies.
 * Coordinates are integer fixed-point values; one terrain cell is POSITION_SCALE
 * simulation units. Cosmetic identifiers are retained in state for presentation but
 * are intentionally omitted from gameplay hashes.
 */

export const SIMULATION_VERSION = 1;
export const CONTENT_VERSION = 1;
export const FIXED_TICK_RATE = 60;
export const POSITION_SCALE = 1_024;
/** One character body width (BW) equals four terrain cells. */
export const BODY_WIDTH = 4 * POSITION_SCALE;
/** Canonical 1.25 BW launch-roster melee reach. */
export const BASE_MELEE_RANGE = 5 * POSITION_SCALE;

export const WEAPON_SLOTS = ["main", "offHand", "melee"] as const;

export type WeaponSlot = (typeof WEAPON_SLOTS)[number];
export type AmmoPolicy =
  | Readonly<{ kind: "finite"; capacity: number }>
  | Readonly<{ kind: "infinite" }>;

export interface SpecialEffectDefinition {
  readonly trigger: "onFire" | "onFlight" | "onImpact" | "onTurnEnd";
  readonly kind:
    | "knockback"
    | "chill"
    | "cluster"
    | "embers"
    | "tunnel"
    | "return"
    | "recoil"
    | "selfDamage";
  readonly magnitude: number;
  readonly durationTurns?: number;
}

export type ProjectileTerrainProfile =
  | Readonly<{ kind: "none" }>
  | Readonly<{ kind: "crater"; radiusCells: number }>
  | Readonly<{ kind: "dig"; radiusCells: number; lengthCells: number }>;

export interface ProjectileDefinition {
  readonly kind: "projectile";
  /** Fixed-point distance added each tick at full power. */
  readonly speedPerTick: number;
  /** Fixed-point downward acceleration added each tick. */
  readonly gravityPerTick: number;
  readonly windScaleBasisPoints: number;
  readonly maxTicks: number;
  readonly terrain: ProjectileTerrainProfile;
}

export interface StrikeDefinition {
  readonly kind: "strike";
  readonly range: number;
  readonly terrain?: Readonly<{ kind: "dig"; radiusCells: number }>;
  readonly selfDamage?: number;
}

export interface WeaponDefinition {
  readonly id: string;
  readonly displayName: string;
  readonly slot: WeaponSlot;
  readonly ammo: AmmoPolicy;
  readonly actionPointCost: number;
  readonly baseDamage: number;
  readonly attack: ProjectileDefinition | StrikeDefinition;
  readonly specialEffects: readonly SpecialEffectDefinition[];
}

const launchWeapons = [
  {
    id: "ramshot-cannon",
    displayName: "Ramshot Cannon",
    slot: "main",
    ammo: { kind: "finite", capacity: 3 },
    actionPointCost: 3,
    baseDamage: 62,
    attack: {
      kind: "projectile",
      speedPerTick: 420,
      gravityPerTick: 18,
      windScaleBasisPoints: 7_500,
      maxTicks: 240,
      terrain: { kind: "crater", radiusCells: 6 },
    },
    specialEffects: [{ trigger: "onImpact", kind: "knockback", magnitude: 1_800 }],
  },
  {
    id: "frostfall-mortar",
    displayName: "Frostfall Mortar",
    slot: "main",
    ammo: { kind: "finite", capacity: 3 },
    actionPointCost: 3,
    baseDamage: 48,
    attack: {
      kind: "projectile",
      speedPerTick: 350,
      gravityPerTick: 22,
      windScaleBasisPoints: 9_000,
      maxTicks: 260,
      terrain: { kind: "crater", radiusCells: 8 },
    },
    specialEffects: [
      { trigger: "onImpact", kind: "chill", magnitude: 2 * BODY_WIDTH, durationTurns: 1 },
    ],
  },
  {
    id: "mole-drill",
    displayName: "Mole Drill",
    slot: "main",
    ammo: { kind: "finite", capacity: 2 },
    actionPointCost: 4,
    baseDamage: 58,
    attack: {
      kind: "projectile",
      speedPerTick: 440,
      gravityPerTick: 13,
      windScaleBasisPoints: 3_500,
      maxTicks: 220,
      terrain: { kind: "dig", radiusCells: 1, lengthCells: 24 },
    },
    specialEffects: [{ trigger: "onImpact", kind: "tunnel", magnitude: 6 * BODY_WIDTH }],
  },
  {
    id: "cinder-cluster",
    displayName: "Cinder Cluster",
    slot: "main",
    ammo: { kind: "finite", capacity: 2 },
    actionPointCost: 4,
    baseDamage: 56,
    attack: {
      kind: "projectile",
      speedPerTick: 390,
      gravityPerTick: 19,
      windScaleBasisPoints: 10_000,
      maxTicks: 240,
      terrain: { kind: "crater", radiusCells: 3 },
    },
    specialEffects: [
      { trigger: "onFlight", kind: "cluster", magnitude: 5 },
      { trigger: "onImpact", kind: "embers", magnitude: 5, durationTurns: 2 },
    ],
  },
  {
    id: "recurve-bow",
    displayName: "Recurve Bow",
    slot: "offHand",
    ammo: { kind: "finite", capacity: 5 },
    actionPointCost: 2,
    baseDamage: 32,
    attack: {
      kind: "projectile",
      speedPerTick: 500,
      gravityPerTick: 16,
      windScaleBasisPoints: 12_000,
      maxTicks: 210,
      terrain: { kind: "none" },
    },
    specialEffects: [],
  },
  {
    id: "longsword",
    displayName: "Longsword",
    slot: "offHand",
    ammo: { kind: "infinite" },
    actionPointCost: 2,
    baseDamage: 24,
    attack: { kind: "strike", range: BASE_MELEE_RANGE * 2 },
    specialEffects: [],
  },
  {
    id: "returning-boomerang",
    displayName: "Returning Boomerang",
    slot: "offHand",
    ammo: { kind: "finite", capacity: 3 },
    actionPointCost: 2,
    baseDamage: 32,
    attack: {
      kind: "projectile",
      speedPerTick: 430,
      gravityPerTick: 9,
      windScaleBasisPoints: 8_000,
      maxTicks: 180,
      terrain: { kind: "none" },
    },
    specialEffects: [{ trigger: "onFlight", kind: "return", magnitude: 90 }],
  },
  {
    id: "needle-7",
    displayName: "Needle-7 Service Pistol",
    slot: "offHand",
    ammo: { kind: "finite", capacity: 6 },
    actionPointCost: 2,
    baseDamage: 26,
    attack: {
      kind: "projectile",
      speedPerTick: 760,
      gravityPerTick: 4,
      windScaleBasisPoints: 0,
      maxTicks: 120,
      terrain: { kind: "none" },
    },
    specialEffects: [],
  },
  {
    id: "heavy-revolver",
    displayName: "Heavy Revolver",
    slot: "offHand",
    ammo: { kind: "finite", capacity: 3 },
    actionPointCost: 3,
    baseDamage: 42,
    attack: {
      kind: "projectile",
      speedPerTick: 700,
      gravityPerTick: 5,
      windScaleBasisPoints: 0,
      maxTicks: 120,
      terrain: { kind: "none" },
    },
    specialEffects: [
      { trigger: "onImpact", kind: "knockback", magnitude: 900 },
      { trigger: "onFire", kind: "recoil", magnitude: BODY_WIDTH / 2 },
    ],
  },
  {
    id: "trench-spade",
    displayName: "Trench Spade",
    slot: "melee",
    ammo: { kind: "finite", capacity: 4 },
    actionPointCost: 1,
    baseDamage: 22,
    attack: {
      kind: "strike",
      range: BASE_MELEE_RANGE,
      terrain: { kind: "dig", radiusCells: 3 },
    },
    specialEffects: [],
  },
  {
    id: "blood-maul",
    displayName: "Blood Maul",
    slot: "melee",
    ammo: { kind: "finite", capacity: 2 },
    actionPointCost: 2,
    baseDamage: 52,
    attack: { kind: "strike", range: BASE_MELEE_RANGE, selfDamage: 14 },
    specialEffects: [{ trigger: "onImpact", kind: "selfDamage", magnitude: 14 }],
  },
  {
    id: "breach-pick",
    displayName: "Breach Pick",
    slot: "melee",
    ammo: { kind: "finite", capacity: 3 },
    actionPointCost: 2,
    baseDamage: 30,
    attack: {
      kind: "strike",
      range: BASE_MELEE_RANGE,
      terrain: { kind: "dig", radiusCells: 2 },
    },
    specialEffects: [],
  },
] as const satisfies readonly WeaponDefinition[];

export type WeaponId = (typeof launchWeapons)[number]["id"];

export const LAUNCH_WEAPONS: readonly WeaponDefinition[] = Object.freeze(
  launchWeapons.map((weapon) => Object.freeze(weapon)),
);

export const WEAPON_DEFINITIONS: Readonly<Record<WeaponId, WeaponDefinition>> =
  Object.freeze(
    Object.fromEntries(LAUNCH_WEAPONS.map((weapon) => [weapon.id, weapon])) as Record<
      WeaponId,
      WeaponDefinition
    >,
  );

export interface EquippedWeapon {
  readonly weaponId: string;
  /** Presentation-only. Ownership is checked outside the deterministic match core. */
  readonly skinId?: string;
}

export interface Loadout {
  readonly main: EquippedWeapon;
  readonly offHand: EquippedWeapon;
  readonly melee: EquippedWeapon;
}

export interface LoadoutValidationResult {
  readonly valid: boolean;
  readonly errors: readonly string[];
}

export function getWeaponDefinition(id: string): WeaponDefinition | undefined {
  return WEAPON_DEFINITIONS[id as WeaponId];
}

export function validateLaunchRoster(): readonly string[] {
  const errors: string[] = [];
  const ids = new Set<string>();

  if (LAUNCH_WEAPONS.length !== 12) {
    errors.push(`Launch roster must contain 12 weapons; found ${LAUNCH_WEAPONS.length}.`);
  }

  for (const weapon of LAUNCH_WEAPONS) {
    if (ids.has(weapon.id)) errors.push(`Duplicate weapon id: ${weapon.id}.`);
    ids.add(weapon.id);
    if (weapon.ammo.kind === "finite" && weapon.ammo.capacity <= 0) {
      errors.push(`${weapon.id} must have positive finite ammo.`);
    }
    if (weapon.slot === "main" && weapon.specialEffects.length === 0) {
      errors.push(`${weapon.id} is a main weapon without a special effect.`);
    }
  }

  const counts = Object.fromEntries(WEAPON_SLOTS.map((slot) => [slot, 0])) as Record<
    WeaponSlot,
    number
  >;
  for (const weapon of LAUNCH_WEAPONS) counts[weapon.slot] += 1;
  if (counts.main !== 4 || counts.offHand !== 5 || counts.melee !== 3) {
    errors.push(
      `Launch roster slots must be 4 main / 5 off-hand / 3 melee; found ${counts.main}/${counts.offHand}/${counts.melee}.`,
    );
  }

  const infiniteWeapons = LAUNCH_WEAPONS.filter((weapon) => weapon.ammo.kind === "infinite");
  if (infiniteWeapons.length !== 1 || infiniteWeapons[0]?.id !== "longsword") {
    errors.push("Longsword must be the only infinite-ammo weapon.");
  }

  const longsword = WEAPON_DEFINITIONS.longsword;
  if (longsword.attack.kind !== "strike" || longsword.attack.range !== BASE_MELEE_RANGE * 2) {
    errors.push("Longsword range must be exactly twice the baseline melee range.");
  }

  return errors;
}

export function validateLoadout(value: unknown): LoadoutValidationResult {
  const errors: string[] = [];
  if (!isRecord(value)) {
    return { valid: false, errors: ["Loadout must be an object."] };
  }

  const actualKeys = Object.keys(value).sort();
  const expectedKeys = [...WEAPON_SLOTS].sort();
  for (const slot of WEAPON_SLOTS) {
    if (!Object.hasOwn(value, slot)) errors.push(`Missing ${slot} weapon slot.`);
  }
  for (const key of actualKeys) {
    if (!expectedKeys.includes(key as WeaponSlot)) errors.push(`Unexpected loadout slot: ${key}.`);
  }

  for (const slot of WEAPON_SLOTS) {
    const equipped = value[slot];
    if (!isRecord(equipped) || typeof equipped.weaponId !== "string") {
      errors.push(`${slot} slot must contain a weaponId.`);
      continue;
    }
    const weapon = getWeaponDefinition(equipped.weaponId);
    if (!weapon) {
      errors.push(`Unknown weapon in ${slot} slot: ${equipped.weaponId}.`);
    } else if (weapon.slot !== slot) {
      errors.push(`${weapon.id} belongs in ${weapon.slot}, not ${slot}.`);
    }
    if (equipped.skinId !== undefined && typeof equipped.skinId !== "string") {
      errors.push(`${slot} skinId must be a string when supplied.`);
    }
  }

  return { valid: errors.length === 0, errors };
}

export function assertValidLoadout(value: unknown): asserts value is Loadout {
  const result = validateLoadout(value);
  if (!result.valid) throw new RangeError(result.errors.join(" "));
}

export interface TerrainMask {
  readonly width: number;
  readonly height: number;
  readonly cells: Uint8Array;
}

export interface CraterOperation {
  readonly kind: "crater";
  readonly centerX: number;
  readonly centerY: number;
  readonly radius: number;
}

export interface DigOperation {
  readonly kind: "dig";
  readonly fromX: number;
  readonly fromY: number;
  readonly toX: number;
  readonly toY: number;
  readonly radius: number;
}

export type TerrainOperation = CraterOperation | DigOperation;

export function createTerrainMask(
  width: number,
  height: number,
  fill: 0 | 1 = 0,
): TerrainMask {
  assertPositiveInteger(width, "terrain width");
  assertPositiveInteger(height, "terrain height");
  if (fill !== 0 && fill !== 1) throw new RangeError("Terrain fill must be 0 or 1.");
  const cells = new Uint8Array(width * height);
  if (fill === 1) cells.fill(1);
  return { width, height, cells };
}

export function cloneTerrainMask(mask: TerrainMask): TerrainMask {
  assertTerrainMask(mask);
  return { width: mask.width, height: mask.height, cells: mask.cells.slice() };
}

export function terrainCell(mask: TerrainMask, x: number, y: number): 0 | 1 {
  if (!Number.isInteger(x) || !Number.isInteger(y)) return 0;
  if (x < 0 || y < 0 || x >= mask.width || y >= mask.height) return 0;
  return mask.cells[y * mask.width + x] === 0 ? 0 : 1;
}

export function setTerrainCell(mask: TerrainMask, x: number, y: number, solid: boolean): void {
  if (!Number.isInteger(x) || !Number.isInteger(y)) {
    throw new TypeError("Terrain coordinates must be integers.");
  }
  if (x < 0 || y < 0 || x >= mask.width || y >= mask.height) return;
  mask.cells[y * mask.width + x] = solid ? 1 : 0;
}

/** Applies an integer circle or capsule subtraction and returns removed cell count. */
export function applyTerrainOperation(mask: TerrainMask, operation: TerrainOperation): number {
  assertTerrainMask(mask);
  switch (operation.kind) {
    case "crater":
      return subtractCircle(mask, operation);
    case "dig":
      return subtractCapsule(mask, operation);
  }
}

function subtractCircle(mask: TerrainMask, operation: CraterOperation): number {
  assertInteger(operation.centerX, "crater centerX");
  assertInteger(operation.centerY, "crater centerY");
  assertNonNegativeInteger(operation.radius, "crater radius");
  const radiusSquared = operation.radius * operation.radius;
  let removed = 0;
  const minX = Math.max(0, operation.centerX - operation.radius);
  const maxX = Math.min(mask.width - 1, operation.centerX + operation.radius);
  const minY = Math.max(0, operation.centerY - operation.radius);
  const maxY = Math.min(mask.height - 1, operation.centerY + operation.radius);

  for (let y = minY; y <= maxY; y += 1) {
    for (let x = minX; x <= maxX; x += 1) {
      const dx = x - operation.centerX;
      const dy = y - operation.centerY;
      if (dx * dx + dy * dy <= radiusSquared) {
        const index = y * mask.width + x;
        if (mask.cells[index] !== 0) {
          mask.cells[index] = 0;
          removed += 1;
        }
      }
    }
  }
  return removed;
}

function subtractCapsule(mask: TerrainMask, operation: DigOperation): number {
  assertInteger(operation.fromX, "dig fromX");
  assertInteger(operation.fromY, "dig fromY");
  assertInteger(operation.toX, "dig toX");
  assertInteger(operation.toY, "dig toY");
  assertNonNegativeInteger(operation.radius, "dig radius");

  const minX = Math.max(0, Math.min(operation.fromX, operation.toX) - operation.radius);
  const maxX = Math.min(mask.width - 1, Math.max(operation.fromX, operation.toX) + operation.radius);
  const minY = Math.max(0, Math.min(operation.fromY, operation.toY) - operation.radius);
  const maxY = Math.min(mask.height - 1, Math.max(operation.fromY, operation.toY) + operation.radius);
  let removed = 0;

  for (let y = minY; y <= maxY; y += 1) {
    for (let x = minX; x <= maxX; x += 1) {
      if (!pointInCapsule(x, y, operation)) continue;
      const index = y * mask.width + x;
      if (mask.cells[index] !== 0) {
        mask.cells[index] = 0;
        removed += 1;
      }
    }
  }
  return removed;
}

function pointInCapsule(x: number, y: number, operation: DigOperation): boolean {
  const lineX = operation.toX - operation.fromX;
  const lineY = operation.toY - operation.fromY;
  const pointX = x - operation.fromX;
  const pointY = y - operation.fromY;
  const lengthSquared = lineX * lineX + lineY * lineY;
  const radiusSquared = operation.radius * operation.radius;

  if (lengthSquared === 0) return pointX * pointX + pointY * pointY <= radiusSquared;
  const dot = pointX * lineX + pointY * lineY;
  if (dot <= 0) return pointX * pointX + pointY * pointY <= radiusSquared;
  if (dot >= lengthSquared) {
    const endX = x - operation.toX;
    const endY = y - operation.toY;
    return endX * endX + endY * endY <= radiusSquared;
  }

  // Compare squared rational distance without division or floating-point projection.
  const cross = pointX * lineY - pointY * lineX;
  return cross * cross <= radiusSquared * lengthSquared;
}

export interface FixedPointPosition {
  readonly x: number;
  readonly y: number;
}

export interface BallisticSample extends FixedPointPosition {
  readonly tick: number;
  readonly timeMs: number;
  readonly velocityX: number;
  readonly velocityY: number;
}

export interface BallisticImpact extends FixedPointPosition {
  readonly tick: number;
  readonly reason: "terrain" | "bounds" | "maxTicks";
  readonly velocityX: number;
  readonly velocityY: number;
}

export interface BallisticInput {
  readonly origin: FixedPointPosition;
  readonly angleMilliDegrees: number;
  readonly powerBasisPoints: number;
  /** Horizontal acceleration in fixed-point units per tick. */
  readonly windPerTick?: number;
  readonly terrain?: TerrainMask;
}

export interface BallisticResult {
  readonly samples: readonly BallisticSample[];
  readonly impact: BallisticImpact;
}

/**
 * Fixed-step, integer-integrated projectile sampling. Trigonometry is performed once
 * at the quantized protocol boundary and immediately rounded to integer velocity.
 */
export function sampleBallisticTrajectory(
  projectile: ProjectileDefinition,
  input: BallisticInput,
): BallisticResult {
  assertInteger(input.origin.x, "origin x");
  assertInteger(input.origin.y, "origin y");
  assertInteger(input.angleMilliDegrees, "angleMilliDegrees");
  assertInteger(input.powerBasisPoints, "powerBasisPoints");
  if (input.powerBasisPoints < 1 || input.powerBasisPoints > 10_000) {
    throw new RangeError("powerBasisPoints must be between 1 and 10000.");
  }
  const windPerTick = input.windPerTick ?? 0;
  assertInteger(windPerTick, "windPerTick");
  if (input.terrain) assertTerrainMask(input.terrain);

  const radians = (input.angleMilliDegrees * Math.PI) / 180_000;
  const launchSpeed = roundDivide(projectile.speedPerTick * input.powerBasisPoints, 10_000);
  let velocityX = Math.round(Math.cos(radians) * launchSpeed);
  let velocityY = -Math.round(Math.sin(radians) * launchSpeed);
  const windAcceleration = roundDivide(
    windPerTick * projectile.windScaleBasisPoints,
    10_000,
  );
  let x = input.origin.x;
  let y = input.origin.y;
  const samples: BallisticSample[] = [
    { tick: 0, timeMs: 0, x, y, velocityX, velocityY },
  ];

  for (let tick = 1; tick <= projectile.maxTicks; tick += 1) {
    velocityX += windAcceleration;
    velocityY += projectile.gravityPerTick;
    x += velocityX;
    y += velocityY;
    const sample: BallisticSample = {
      tick,
      timeMs: Math.round((tick * 1_000) / FIXED_TICK_RATE),
      x,
      y,
      velocityX,
      velocityY,
    };
    samples.push(sample);

    if (input.terrain) {
      const maxX = input.terrain.width * POSITION_SCALE;
      const maxY = input.terrain.height * POSITION_SCALE;
      if (x < 0 || x >= maxX || y >= maxY) {
        return { samples, impact: { tick, x, y, velocityX, velocityY, reason: "bounds" } };
      }
      if (y >= 0) {
        const cellX = Math.floor(x / POSITION_SCALE);
        const cellY = Math.floor(y / POSITION_SCALE);
        if (terrainCell(input.terrain, cellX, cellY) === 1) {
          return { samples, impact: { tick, x, y, velocityX, velocityY, reason: "terrain" } };
        }
      }
    }
  }

  return {
    samples,
    impact: {
      tick: projectile.maxTicks,
      x,
      y,
      velocityX,
      velocityY,
      reason: "maxTicks",
    },
  };
}

export function sampleWeaponTrajectory(
  weaponId: string,
  input: BallisticInput,
): BallisticResult {
  const weapon = getWeaponDefinition(weaponId);
  if (!weapon) throw new RangeError(`Unknown weapon: ${weaponId}.`);
  if (weapon.attack.kind !== "projectile") {
    throw new RangeError(`${weapon.id} is not a projectile weapon.`);
  }
  return sampleBallisticTrajectory(weapon.attack, input);
}

export interface AvatarAppearance {
  readonly bodyId?: string;
  readonly faceId?: string;
  readonly hairId?: string;
  readonly headgearId?: string;
  readonly outfitId?: string;
  readonly accessoryId?: string;
  readonly paletteId?: string;
}

export interface PlayerState {
  readonly id: string;
  readonly health: number;
  readonly position: FixedPointPosition;
  readonly loadout: Loadout;
  readonly ammo: Readonly<Record<string, number>>;
  /** Presentation-only and intentionally excluded from state hashing. */
  readonly avatar?: AvatarAppearance;
}

export interface SimulationState {
  readonly simulationVersion: number;
  readonly contentVersion: number;
  readonly tick: number;
  readonly turnId: string;
  readonly activePlayerId: string;
  readonly windPerTick: number;
  readonly terrain: TerrainMask;
  readonly players: Readonly<Record<string, PlayerState>>;
  readonly processedCommandIds: readonly string[];
}

export interface NewPlayerState {
  readonly id: string;
  readonly health?: number;
  readonly position: FixedPointPosition;
  readonly loadout: Loadout;
  readonly ammo?: Readonly<Record<string, number>>;
  readonly avatar?: AvatarAppearance;
}

export interface CreateSimulationStateInput {
  readonly terrain: TerrainMask;
  readonly players: readonly NewPlayerState[];
  readonly activePlayerId: string;
  readonly turnId?: string;
  readonly windPerTick?: number;
}

export function createSimulationState(input: CreateSimulationStateInput): SimulationState {
  assertTerrainMask(input.terrain);
  if (input.players.length === 0) throw new RangeError("At least one player is required.");
  const players: Record<string, PlayerState> = {};

  for (const player of input.players) {
    if (!player.id) throw new RangeError("Player id cannot be empty.");
    if (players[player.id]) throw new RangeError(`Duplicate player id: ${player.id}.`);
    assertValidLoadout(player.loadout);
    assertInteger(player.position.x, `${player.id} position x`);
    assertInteger(player.position.y, `${player.id} position y`);
    const health = player.health ?? 200;
    assertNonNegativeInteger(health, `${player.id} health`);
    const initialAmmo = createInitialAmmo(player.loadout);
    if (player.ammo) {
      for (const [weaponId, amount] of Object.entries(player.ammo)) {
        const weapon = getWeaponDefinition(weaponId);
        if (!weapon || weapon.ammo.kind !== "finite") {
          throw new RangeError(`Ammo override references a non-finite weapon: ${weaponId}.`);
        }
        assertNonNegativeInteger(amount, `${weaponId} ammo`);
        if (amount > weapon.ammo.capacity) {
          throw new RangeError(`${weaponId} ammo exceeds capacity ${weapon.ammo.capacity}.`);
        }
        initialAmmo[weaponId] = amount;
      }
    }
    players[player.id] = {
      id: player.id,
      health,
      position: { x: player.position.x, y: player.position.y },
      loadout: player.loadout,
      ammo: initialAmmo,
      avatar: player.avatar,
    };
  }

  if (!players[input.activePlayerId]) {
    throw new RangeError(`Unknown active player: ${input.activePlayerId}.`);
  }
  const windPerTick = input.windPerTick ?? 0;
  assertInteger(windPerTick, "windPerTick");

  return {
    simulationVersion: SIMULATION_VERSION,
    contentVersion: CONTENT_VERSION,
    tick: 0,
    turnId: input.turnId ?? "turn-1",
    activePlayerId: input.activePlayerId,
    windPerTick,
    terrain: cloneTerrainMask(input.terrain),
    players,
    processedCommandIds: [],
  };
}

export function createInitialAmmo(loadout: Loadout): Record<string, number> {
  assertValidLoadout(loadout);
  const ammo: Record<string, number> = {};
  for (const slot of WEAPON_SLOTS) {
    const weapon = getWeaponDefinition(loadout[slot].weaponId)!;
    if (weapon.ammo.kind === "finite") ammo[weapon.id] = weapon.ammo.capacity;
  }
  return ammo;
}

export function remainingAmmo(
  state: SimulationState,
  playerId: string,
  weaponId: string,
): number {
  const player = state.players[playerId];
  if (!player) throw new RangeError(`Unknown player: ${playerId}.`);
  const weapon = getWeaponDefinition(weaponId);
  if (!weapon) throw new RangeError(`Unknown weapon: ${weaponId}.`);
  return weapon.ammo.kind === "infinite" ? Number.POSITIVE_INFINITY : (player.ammo[weaponId] ?? 0);
}

export interface WeaponCommand {
  readonly commandId: string;
  readonly playerId: string;
  readonly expectedTurnId: string;
  readonly slot: WeaponSlot;
  readonly weaponId: string;
  readonly angleMilliDegrees?: number;
  readonly powerBasisPoints?: number;
  readonly target?: FixedPointPosition;
}

export type CommandRejectionReason =
  | "invalid-command"
  | "wrong-turn"
  | "stale-turn"
  | "loadout-mismatch"
  | "out-of-ammo"
  | "invalid-aim"
  | "out-of-range";

export interface CommandResult {
  readonly status: "accepted" | "duplicate" | "rejected";
  readonly state: SimulationState;
  readonly stateHash: string;
  readonly reason?: CommandRejectionReason;
  readonly ammoConsumed: number;
  readonly selfDamage: number;
  readonly trajectory?: BallisticResult;
  readonly terrainOperation?: TerrainOperation;
  readonly terrainCellsRemoved: number;
}

/**
 * Validates and applies one authoritative weapon command. The input state is never
 * mutated. Replaying a commandId is a no-op, including ammo and terrain.
 */
export function applyWeaponCommand(
  state: SimulationState,
  command: WeaponCommand,
): CommandResult {
  if (state.processedCommandIds.includes(command.commandId)) {
    return unchangedCommandResult("duplicate", state);
  }
  if (!command.commandId || !command.playerId || !command.expectedTurnId) {
    return unchangedCommandResult("rejected", state, "invalid-command");
  }
  if (command.playerId !== state.activePlayerId || !state.players[command.playerId]) {
    return unchangedCommandResult("rejected", state, "wrong-turn");
  }
  if (command.expectedTurnId !== state.turnId) {
    return unchangedCommandResult("rejected", state, "stale-turn");
  }

  const player = state.players[command.playerId]!;
  const equipped = player.loadout[command.slot];
  const weapon = getWeaponDefinition(command.weaponId);
  if (!weapon || weapon.slot !== command.slot || equipped.weaponId !== command.weaponId) {
    return unchangedCommandResult("rejected", state, "loadout-mismatch");
  }
  if (weapon.ammo.kind === "finite" && (player.ammo[weapon.id] ?? 0) <= 0) {
    return unchangedCommandResult("rejected", state, "out-of-ammo");
  }

  let trajectory: BallisticResult | undefined;
  let terrainOperation: TerrainOperation | undefined;
  let selfDamage = 0;
  const nextTerrain = cloneTerrainMask(state.terrain);

  if (weapon.attack.kind === "projectile") {
    if (
      !Number.isInteger(command.angleMilliDegrees) ||
      !Number.isInteger(command.powerBasisPoints) ||
      command.powerBasisPoints! < 1 ||
      command.powerBasisPoints! > 10_000
    ) {
      return unchangedCommandResult("rejected", state, "invalid-aim");
    }
    trajectory = sampleBallisticTrajectory(weapon.attack, {
      origin: player.position,
      angleMilliDegrees: command.angleMilliDegrees!,
      powerBasisPoints: command.powerBasisPoints!,
      windPerTick: state.windPerTick,
      terrain: state.terrain,
    });
    if (trajectory.impact.reason === "terrain") {
      terrainOperation = projectileTerrainOperation(weapon.attack, trajectory.impact);
    }
  } else {
    if (!command.target || !isIntegerPosition(command.target)) {
      return unchangedCommandResult("rejected", state, "invalid-aim");
    }
    const deltaX = command.target.x - player.position.x;
    const deltaY = command.target.y - player.position.y;
    if (deltaX * deltaX + deltaY * deltaY > weapon.attack.range * weapon.attack.range) {
      return unchangedCommandResult("rejected", state, "out-of-range");
    }
    if (weapon.attack.terrain?.kind === "dig") {
      terrainOperation = {
        kind: "dig",
        fromX: Math.floor(player.position.x / POSITION_SCALE),
        fromY: Math.floor(player.position.y / POSITION_SCALE),
        toX: Math.floor(command.target.x / POSITION_SCALE),
        toY: Math.floor(command.target.y / POSITION_SCALE),
        radius: weapon.attack.terrain.radiusCells,
      };
    }
    selfDamage = weapon.attack.selfDamage ?? 0;
  }

  const terrainCellsRemoved = terrainOperation
    ? applyTerrainOperation(nextTerrain, terrainOperation)
    : 0;
  const nextAmmo = { ...player.ammo };
  let ammoConsumed = 0;
  if (weapon.ammo.kind === "finite") {
    nextAmmo[weapon.id] = (nextAmmo[weapon.id] ?? 0) - 1;
    ammoConsumed = 1;
  }
  const nextPlayer: PlayerState = {
    ...player,
    health: Math.max(0, player.health - selfDamage),
    ammo: nextAmmo,
  };
  const elapsedTicks = trajectory?.impact.tick ?? 1;
  const nextState: SimulationState = {
    ...state,
    tick: state.tick + elapsedTicks,
    terrain: nextTerrain,
    players: { ...state.players, [player.id]: nextPlayer },
    processedCommandIds: [...state.processedCommandIds, command.commandId],
  };

  return {
    status: "accepted",
    state: nextState,
    stateHash: hashSimulationState(nextState),
    ammoConsumed,
    selfDamage,
    trajectory,
    terrainOperation,
    terrainCellsRemoved,
  };
}

function projectileTerrainOperation(
  projectile: ProjectileDefinition,
  impact: BallisticImpact,
): TerrainOperation | undefined {
  const impactX = Math.floor(impact.x / POSITION_SCALE);
  const impactY = Math.floor(impact.y / POSITION_SCALE);
  if (projectile.terrain.kind === "none") return undefined;
  if (projectile.terrain.kind === "crater") {
    return {
      kind: "crater",
      centerX: impactX,
      centerY: impactY,
      radius: projectile.terrain.radiusCells,
    };
  }

  const dominantVelocity = Math.max(Math.abs(impact.velocityX), Math.abs(impact.velocityY), 1);
  const distance = projectile.terrain.lengthCells;
  return {
    kind: "dig",
    fromX: impactX,
    fromY: impactY,
    toX: impactX + roundDivide(impact.velocityX * distance, dominantVelocity),
    toY: impactY + roundDivide(impact.velocityY * distance, dominantVelocity),
    radius: projectile.terrain.radiusCells,
  };
}

function unchangedCommandResult(
  status: "duplicate" | "rejected",
  state: SimulationState,
  reason?: CommandRejectionReason,
): CommandResult {
  return {
    status,
    state,
    stateHash: hashSimulationState(state),
    reason,
    ammoConsumed: 0,
    selfDamage: 0,
    terrainCellsRemoved: 0,
  };
}

/** Deterministic 64-bit FNV-1a hash over gameplay state and terrain bytes. */
export function hashSimulationState(state: SimulationState): string {
  const players = Object.values(state.players)
    .sort((left, right) => left.id.localeCompare(right.id))
    .map((player) => ({
      id: player.id,
      health: player.health,
      position: { x: player.position.x, y: player.position.y },
      loadout: {
        main: player.loadout.main.weaponId,
        offHand: player.loadout.offHand.weaponId,
        melee: player.loadout.melee.weaponId,
      },
      ammo: Object.entries(player.ammo)
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([weaponId, amount]) => [weaponId, amount]),
    }));
  const canonical = JSON.stringify({
    simulationVersion: state.simulationVersion,
    contentVersion: state.contentVersion,
    tick: state.tick,
    turnId: state.turnId,
    activePlayerId: state.activePlayerId,
    windPerTick: state.windPerTick,
    terrain: { width: state.terrain.width, height: state.terrain.height },
    players,
    processedCommandIds: [...state.processedCommandIds].sort(),
  });

  let hash = 14_695_981_039_346_656_037n;
  const prime = 1_099_511_628_211n;
  const mask = 0xffff_ffff_ffff_ffffn;
  for (const byte of new TextEncoder().encode(canonical)) {
    hash ^= BigInt(byte);
    hash = (hash * prime) & mask;
  }
  // Domain separator prevents metadata suffixes from aliasing terrain prefixes.
  hash ^= 0xffn;
  hash = (hash * prime) & mask;
  for (const cell of state.terrain.cells) {
    hash ^= BigInt(cell);
    hash = (hash * prime) & mask;
  }
  return hash.toString(16).padStart(16, "0");
}

export function hashTerrainMask(mask: TerrainMask): string {
  const emptyState: SimulationState = {
    simulationVersion: SIMULATION_VERSION,
    contentVersion: CONTENT_VERSION,
    tick: 0,
    turnId: "terrain",
    activePlayerId: "terrain",
    windPerTick: 0,
    terrain: mask,
    players: {},
    processedCommandIds: [],
  };
  return hashSimulationState(emptyState);
}

function createInitialAmmoForDefinition(weapon: WeaponDefinition): number | undefined {
  return weapon.ammo.kind === "finite" ? weapon.ammo.capacity : undefined;
}

// Keep this tiny exported helper useful to inventory/profile adapters without leaking
// the internal map construction into those layers.
export function initialAmmoForWeapon(weaponId: string): number {
  const weapon = getWeaponDefinition(weaponId);
  if (!weapon) throw new RangeError(`Unknown weapon: ${weaponId}.`);
  return createInitialAmmoForDefinition(weapon) ?? Number.POSITIVE_INFINITY;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isIntegerPosition(position: FixedPointPosition): boolean {
  return Number.isInteger(position.x) && Number.isInteger(position.y);
}

function assertTerrainMask(mask: TerrainMask): void {
  assertPositiveInteger(mask.width, "terrain width");
  assertPositiveInteger(mask.height, "terrain height");
  if (!(mask.cells instanceof Uint8Array) || mask.cells.length !== mask.width * mask.height) {
    throw new RangeError("Terrain cell buffer does not match its dimensions.");
  }
}

function assertInteger(value: number, label: string): void {
  if (!Number.isSafeInteger(value)) throw new TypeError(`${label} must be a safe integer.`);
}

function assertNonNegativeInteger(value: number, label: string): void {
  assertInteger(value, label);
  if (value < 0) throw new RangeError(`${label} cannot be negative.`);
}

function assertPositiveInteger(value: number, label: string): void {
  assertInteger(value, label);
  if (value <= 0) throw new RangeError(`${label} must be positive.`);
}

function roundDivide(numerator: number, denominator: number): number {
  if (denominator <= 0) throw new RangeError("roundDivide denominator must be positive.");
  return numerator < 0
    ? -Math.floor((-numerator + denominator / 2) / denominator)
    : Math.floor((numerator + denominator / 2) / denominator);
}

const rosterErrors = validateLaunchRoster();
if (rosterErrors.length > 0) {
  throw new Error(`Invalid Dungeon Barrage launch roster: ${rosterErrors.join(" ")}`);
}
