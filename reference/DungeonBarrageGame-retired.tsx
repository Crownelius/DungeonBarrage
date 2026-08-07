"use client";

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

const VIEW_WIDTH = 960;
const VIEW_HEIGHT = 540;
const BODY_WIDTH = 52;
const TURN_SECONDS = 25;

type Slot = "main" | "secondary" | "melee";
type FighterIndex = 0 | 1;
type Facing = -1 | 1;

type Appearance = {
  hair: "crest" | "mop" | "hood";
  hairColor: string;
  outfitColor: string;
  accentColor: string;
  skinTone: string;
};

type Weapon = {
  id: string;
  name: string;
  slot: Slot;
  ammo: number;
  damage: number;
  range: number;
  crater: number;
  color: string;
  glyph: string;
  summary: string;
  tag: string;
  mode: "projectile" | "strike";
  gravity?: number;
  wind?: number;
  knockback?: number;
  selfDamage?: number;
};

const WEAPONS: readonly Weapon[] = [
  {
    id: "ramshot-cannon",
    name: "Ramshot Cannon",
    slot: "main",
    ammo: 3,
    damage: 62,
    range: 999,
    crater: 42,
    color: "#ffb34f",
    glyph: "RC",
    summary: "Reliable shell with brutal knockback.",
    tag: "CONCUSSION",
    mode: "projectile",
    gravity: 1,
    wind: 1,
    knockback: 42,
  },
  {
    id: "frostfall-mortar",
    name: "Frostfall Mortar",
    slot: "main",
    ammo: 3,
    damage: 48,
    range: 999,
    crater: 48,
    color: "#8de5ff",
    glyph: "FM",
    summary: "High arc. Chills anything near the core.",
    tag: "CHILL",
    mode: "projectile",
    gravity: 1.12,
    wind: 1.18,
    knockback: 24,
  },
  {
    id: "mole-drill",
    name: "Mole Drill",
    slot: "main",
    ammo: 2,
    damage: 58,
    range: 999,
    crater: 32,
    color: "#f3d476",
    glyph: "MD",
    summary: "Bores through soil before detonating.",
    tag: "TUNNEL",
    mode: "projectile",
    gravity: 0.82,
    wind: 0.55,
    knockback: 20,
  },
  {
    id: "cinder-cluster",
    name: "Cinder Cluster",
    slot: "main",
    ammo: 2,
    damage: 56,
    range: 999,
    crater: 26,
    color: "#ff6d4d",
    glyph: "CC",
    summary: "Five hot fragments chew up a wide area.",
    tag: "CLUSTER",
    mode: "projectile",
    gravity: 1.02,
    wind: 1.08,
    knockback: 18,
  },
  {
    id: "recurve-bow",
    name: "Recurve Bow",
    slot: "secondary",
    ammo: 5,
    damage: 32,
    range: 18 * BODY_WIDTH,
    crater: 0,
    color: "#b7eb91",
    glyph: "RB",
    summary: "Light, quiet, and highly wind-sensitive.",
    tag: "PRECISION",
    mode: "projectile",
    gravity: 0.74,
    wind: 1.55,
    knockback: 8,
  },
  {
    id: "longsword",
    name: "Longsword",
    slot: "secondary",
    ammo: Number.POSITIVE_INFINITY,
    damage: 24,
    range: 2.5 * BODY_WIDTH,
    crater: 0,
    color: "#e7edf8",
    glyph: "LS",
    summary: "The only infinite-use weapon. Double reach.",
    tag: "∞ AMMO · 2× REACH",
    mode: "strike",
    knockback: 8,
  },
  {
    id: "returning-boomerang",
    name: "Returning Boomerang",
    slot: "secondary",
    ammo: 3,
    damage: 32,
    range: 12 * BODY_WIDTH,
    crater: 0,
    color: "#ffcc7e",
    glyph: "BG",
    summary: "Curves out and back around light cover.",
    tag: "RETURNING",
    mode: "projectile",
    gravity: 0.25,
    wind: 0.4,
    knockback: 12,
  },
  {
    id: "needle-7",
    name: "Needle-7 Pistol",
    slot: "secondary",
    ammo: 6,
    damage: 26,
    range: 12 * BODY_WIDTH,
    crater: 0,
    color: "#d2bcff",
    glyph: "N7",
    summary: "A fictional flat-shooting 5.7 sidearm.",
    tag: "WINDPROOF",
    mode: "projectile",
    gravity: 0.08,
    wind: 0,
    knockback: 7,
  },
  {
    id: "heavy-revolver",
    name: "Heavy Revolver",
    slot: "secondary",
    ammo: 3,
    damage: 42,
    range: 8 * BODY_WIDTH,
    crater: 0,
    color: "#f7a873",
    glyph: "HR",
    summary: "Shorter range, fierce recoil and knockback.",
    tag: "RECOIL",
    mode: "projectile",
    gravity: 0.1,
    wind: 0,
    knockback: 30,
  },
  {
    id: "trench-spade",
    name: "Trench Spade",
    slot: "melee",
    ammo: 4,
    damage: 22,
    range: 1.25 * BODY_WIDTH,
    crater: 34,
    color: "#9de4c1",
    glyph: "TS",
    summary: "Cuts a pocket through soil and light cover.",
    tag: "DIG",
    mode: "strike",
    knockback: 7,
  },
  {
    id: "blood-maul",
    name: "Backlash Maul",
    slot: "melee",
    ammo: 2,
    damage: 52,
    range: 1.25 * BODY_WIDTH,
    crater: 15,
    color: "#ff6686",
    glyph: "BM",
    summary: "Huge impact. Deals 14 Backlash to its user.",
    tag: "14 BACKLASH",
    mode: "strike",
    knockback: 22,
    selfDamage: 14,
  },
  {
    id: "breach-pick",
    name: "Breach Pick",
    slot: "melee",
    ammo: 3,
    damage: 30,
    range: 1.25 * BODY_WIDTH,
    crater: 23,
    color: "#b9a5ff",
    glyph: "BP",
    summary: "Cracks reinforced dungeon stone.",
    tag: "BREACH",
    mode: "strike",
    knockback: 12,
  },
] as const;

const WEAPON_BY_ID = new Map(WEAPONS.map((weapon) => [weapon.id, weapon]));

type Fighter = {
  name: string;
  title: string;
  x: number;
  y: number;
  health: number;
  facing: Facing;
  status: string;
  appearance: Appearance;
};

type Loadout = Record<Slot, string>;

type Terrain = {
  mask: Uint8Array;
  canvas: HTMLCanvasElement;
  context: CanvasRenderingContext2D;
};

type Projectile = {
  x: number;
  y: number;
  vx: number;
  vy: number;
  age: number;
  owner: FighterIndex;
  weaponId: string;
  drillRemaining: number;
};

type BurstEffect = {
  x: number;
  y: number;
  radius: number;
  age: number;
  duration: number;
  color: string;
  kind: "burst" | "slash" | "spark";
};

type GameModel = {
  terrain: Terrain;
  players: [Fighter, Fighter];
  loadouts: [Loadout, Loadout];
  ammo: [Record<string, number>, Record<string, number>];
  weaponSkins: Record<Slot, string>;
  active: FighterIndex;
  selectedSlot: Slot;
  angle: number;
  power: number;
  wind: number;
  turn: number;
  timer: number;
  movement: number;
  phase: "planning" | "resolving" | "finished";
  winner: string;
  message: string;
  projectile: Projectile | null;
  effects: BurstEffect[];
  resolveAt: number;
  botAt: number;
  rng: number;
  actionLog: string[];
};

const DEFAULT_APPEARANCE: Appearance = {
  hair: "crest",
  hairColor: "#36283f",
  outfitColor: "#ef6e52",
  accentColor: "#ffc45f",
  skinTone: "#f1b58b",
};

const BOT_APPEARANCE: Appearance = {
  hair: "hood",
  hairColor: "#27334d",
  outfitColor: "#5888c8",
  accentColor: "#a8e6ff",
  skinTone: "#d99775",
};

function seededNoise(seed: number) {
  const value = Math.sin(seed * 12.9898) * 43758.5453;
  return value - Math.floor(value);
}

function nextRandom(model: GameModel) {
  model.rng = (Math.imul(model.rng, 1664525) + 1013904223) >>> 0;
  return model.rng / 4294967296;
}

function maskIndex(x: number, y: number) {
  return y * VIEW_WIDTH + x;
}

function isSolid(terrain: Terrain, x: number, y: number) {
  const ix = Math.round(x);
  const iy = Math.round(y);
  if (ix < 0 || ix >= VIEW_WIDTH || iy < 0 || iy >= VIEW_HEIGHT) return false;
  return terrain.mask[maskIndex(ix, iy)] === 1;
}

function paintTerrain(terrain: Terrain) {
  const image = terrain.context.createImageData(VIEW_WIDTH, VIEW_HEIGHT);
  const { data } = image;
  for (let y = 0; y < VIEW_HEIGHT; y += 1) {
    for (let x = 0; x < VIEW_WIDTH; x += 1) {
      if (!terrain.mask[maskIndex(x, y)]) continue;
      const index = maskIndex(x, y) * 4;
      const surface = y === 0 || !terrain.mask[maskIndex(x, y - 1)];
      const grain = Math.floor(seededNoise(x * 0.83 + y * 1.71) * 13);
      data[index] = surface ? 118 + grain : 59 + grain;
      data[index + 1] = surface ? 105 + grain : 54 + Math.floor(grain / 2);
      data[index + 2] = surface ? 106 + grain : 70 + grain;
      data[index + 3] = 255;
    }
  }
  terrain.context.clearRect(0, 0, VIEW_WIDTH, VIEW_HEIGHT);
  terrain.context.putImageData(image, 0, 0);
}

function carveCircle(terrain: Terrain, cx: number, cy: number, radius: number) {
  const minX = Math.max(0, Math.floor(cx - radius));
  const maxX = Math.min(VIEW_WIDTH - 1, Math.ceil(cx + radius));
  const minY = Math.max(0, Math.floor(cy - radius));
  const maxY = Math.min(VIEW_HEIGHT - 1, Math.ceil(cy + radius));
  const r2 = radius * radius;
  for (let y = minY; y <= maxY; y += 1) {
    for (let x = minX; x <= maxX; x += 1) {
      if ((x - cx) ** 2 + (y - cy) ** 2 <= r2) {
        terrain.mask[maskIndex(x, y)] = 0;
      }
    }
  }
}

function createTerrain(): Terrain {
  const canvas = document.createElement("canvas");
  canvas.width = VIEW_WIDTH;
  canvas.height = VIEW_HEIGHT;
  const context = canvas.getContext("2d", { alpha: true });
  if (!context) throw new Error("Dungeon Barrage requires Canvas 2D support.");
  const mask = new Uint8Array(VIEW_WIDTH * VIEW_HEIGHT);

  for (let x = 0; x < VIEW_WIDTH; x += 1) {
    const valley = 357 + Math.sin(x * 0.012) * 44 + Math.sin(x * 0.031) * 17;
    const leftRise = x < 195 ? -68 * (1 - x / 195) : 0;
    const rightRise = x > 770 ? -62 * ((x - 770) / 190) : 0;
    const surface = Math.max(245, Math.min(425, valley + leftRise + rightRise));
    for (let y = Math.floor(surface); y < VIEW_HEIGHT; y += 1) {
      mask[maskIndex(x, y)] = 1;
    }
  }

  // Two original overhangs give the level a dungeon silhouette without copying
  // the supplied reference art.
  for (let y = 205; y < 330; y += 1) {
    for (let x = 48; x < 305; x += 1) {
      const nx = (x - 160) / 145;
      const ny = (y - 270) / 72;
      if (nx * nx + ny * ny < 1) mask[maskIndex(x, y)] = 1;
    }
    for (let x = 680; x < 930; x += 1) {
      const nx = (x - 815) / 142;
      const ny = (y - 260) / 66;
      if (nx * nx + ny * ny < 1) mask[maskIndex(x, y)] = 1;
    }
  }

  const terrain = { mask, canvas, context };
  carveCircle(terrain, 485, 430, 76);
  carveCircle(terrain, 92, 302, 33);
  carveCircle(terrain, 870, 292, 36);
  paintTerrain(terrain);
  return terrain;
}

function findGround(terrain: Terrain, x: number, fromY = 0) {
  const ix = Math.max(2, Math.min(VIEW_WIDTH - 3, Math.round(x)));
  for (let y = Math.max(1, Math.round(fromY)); y < VIEW_HEIGHT - 2; y += 1) {
    if (isSolid(terrain, ix, y) && !isSolid(terrain, ix, y - 1)) return y - 19;
  }
  return VIEW_HEIGHT + 40;
}

function makeAmmo(loadout: Loadout) {
  const ammo: Record<string, number> = {};
  Object.values(loadout).forEach((weaponId) => {
    ammo[weaponId] = WEAPON_BY_ID.get(weaponId)?.ammo ?? 0;
  });
  return ammo;
}

function createModel(terrain: Terrain): GameModel {
  const playerLoadout: Loadout = {
    main: "ramshot-cannon",
    secondary: "longsword",
    melee: "trench-spade",
  };
  const botLoadout: Loadout = {
    main: "frostfall-mortar",
    secondary: "needle-7",
    melee: "blood-maul",
  };
  return {
    terrain,
    players: [
      {
        name: "Nova",
        title: "DELVER",
        x: 165,
        y: findGround(terrain, 165),
        health: 200,
        facing: 1,
        status: "READY",
        appearance: { ...DEFAULT_APPEARANCE },
      },
      {
        name: "Morrow",
        title: "RIVAL BOT",
        x: 805,
        y: findGround(terrain, 805),
        health: 200,
        facing: -1,
        status: "READY",
        appearance: { ...BOT_APPEARANCE },
      },
    ],
    loadouts: [playerLoadout, botLoadout],
    ammo: [makeAmmo(playerLoadout), makeAmmo(botLoadout)],
    weaponSkins: { main: "Foundry", secondary: "Moonsteel", melee: "Relic" },
    active: 0,
    selectedSlot: "main",
    angle: 48,
    power: 68,
    wind: 1.7,
    turn: 1,
    timer: TURN_SECONDS,
    movement: 100,
    phase: "planning",
    winner: "",
    message: "Your turn — read the wind, then make it count.",
    projectile: null,
    effects: [],
    resolveAt: 0,
    botAt: 0,
    rng: 0xd06e0f5,
    actionLog: ["MATCH START · Seed DB-0D06E0F5"],
  };
}

function equippedWeapon(model: GameModel, owner = model.active, slot = model.selectedSlot) {
  return WEAPON_BY_ID.get(model.loadouts[owner][slot]) ?? WEAPONS[0];
}

function settleFighter(model: GameModel, fighter: Fighter) {
  fighter.x = Math.max(14, Math.min(VIEW_WIDTH - 14, fighter.x));
  fighter.y = findGround(model.terrain, fighter.x, Math.max(0, fighter.y - 24));
  if (fighter.y > VIEW_HEIGHT) fighter.health = 0;
}

function addEffect(
  model: GameModel,
  x: number,
  y: number,
  radius: number,
  color: string,
  kind: BurstEffect["kind"] = "burst",
) {
  model.effects.push({ x, y, radius, age: 0, duration: kind === "slash" ? 0.38 : 0.7, color, kind });
}

function refreshLoadoutAmmo(model: GameModel, player: FighterIndex, slot: Slot, weaponId: string) {
  model.loadouts[player][slot] = weaponId;
  const weapon = WEAPON_BY_ID.get(weaponId);
  if (weapon && model.ammo[player][weaponId] === undefined) {
    model.ammo[player][weaponId] = weapon.ammo;
  }
}

function announce(model: GameModel, line: string) {
  model.message = line;
  model.actionLog = [line, ...model.actionLog].slice(0, 4);
}

function transitionTurn(model: GameModel, now: number) {
  if (model.phase === "finished") return;
  model.active = model.active === 0 ? 1 : 0;
  model.turn += 1;
  model.timer = TURN_SECONDS;
  model.movement = 100;
  model.phase = "planning";
  model.resolveAt = 0;
  model.wind = Math.round((nextRandom(model) * 8 - 4) * 10) / 10;
  model.players[model.active].status = "ACTIVE";
  model.players[model.active === 0 ? 1 : 0].status = "WAITING";
  if (model.active === 1) {
    model.botAt = now + 950;
    announce(model, `Morrow studies the ${Math.abs(model.wind).toFixed(1)} wind…`);
  } else {
    model.botAt = 0;
    announce(model, "Your turn — the dungeon remembers every crater.");
  }
}

function concludeIfNeeded(model: GameModel) {
  const [player, bot] = model.players;
  if (player.health > 0 && bot.health > 0) return false;
  model.phase = "finished";
  model.projectile = null;
  model.resolveAt = 0;
  model.winner = player.health <= 0 ? "Morrow" : "Nova";
  announce(model, `${model.winner.toUpperCase()} CLAIMS THE VAULT`);
  return true;
}

function applyBlast(
  model: GameModel,
  x: number,
  y: number,
  weapon: Weapon,
  owner: FighterIndex,
) {
  if (weapon.id === "cinder-cluster") {
    [-38, -19, 0, 19, 38].forEach((offset, index) => {
      const by = y - Math.abs(offset) * 0.2 + (index % 2) * 5;
      carveCircle(model.terrain, x + offset, by, weapon.crater * 0.68);
      addEffect(model, x + offset, by, 28, weapon.color, "spark");
    });
  } else if (weapon.crater > 0) {
    carveCircle(model.terrain, x, y, weapon.crater);
  }

  const radius = Math.max(24, weapon.crater * 1.65);
  model.players.forEach((fighter, index) => {
    const distance = Math.hypot(fighter.x - x, fighter.y - y);
    if (distance > radius) return;
    const falloff = Math.max(0.22, 1 - distance / radius);
    const damage = Math.max(1, Math.round(weapon.damage * falloff));
    fighter.health = Math.max(0, fighter.health - damage);
    const direction = fighter.x < x ? -1 : 1;
    fighter.x += direction * Math.round((weapon.knockback ?? 10) * falloff);
    fighter.status = index === owner ? "SCORCHED" : weapon.tag;
  });

  if (weapon.id === "frostfall-mortar") {
    const target = model.players[owner === 0 ? 1 : 0];
    if (Math.hypot(target.x - x, target.y - y) <= radius) target.status = "CHILLED";
  }

  model.players.forEach((fighter) => settleFighter(model, fighter));
  paintTerrain(model.terrain);
  addEffect(model, x, y, Math.max(32, weapon.crater + 12), weapon.color);
}

function resolveStrike(model: GameModel, owner: FighterIndex, weapon: Weapon, now: number) {
  const attacker = model.players[owner];
  const target = model.players[owner === 0 ? 1 : 0];
  const strikeX = attacker.x + attacker.facing * Math.min(weapon.range * 0.76, 74);
  const strikeY = attacker.y + 6;
  const distance = Math.hypot(target.x - attacker.x, target.y - attacker.y);
  const inFront = Math.sign(target.x - attacker.x) === attacker.facing;

  addEffect(model, strikeX, strikeY, Math.max(28, weapon.range * 0.35), weapon.color, "slash");
  if (weapon.crater > 0) {
    carveCircle(model.terrain, strikeX, strikeY + 15, weapon.crater);
    paintTerrain(model.terrain);
  }
  if (inFront && distance <= weapon.range + 18) {
    target.health = Math.max(0, target.health - weapon.damage);
    target.x += attacker.facing * (weapon.knockback ?? 8);
    target.status = weapon.tag;
    settleFighter(model, target);
    announce(model, `${attacker.name} lands ${weapon.name} for ${weapon.damage}.`);
  } else {
    announce(model, `${attacker.name} reshapes the dungeon with ${weapon.name}.`);
  }
  if (weapon.selfDamage) {
    attacker.health = Math.max(0, attacker.health - weapon.selfDamage);
    attacker.status = `${weapon.selfDamage} BACKLASH`;
  }
  model.phase = "resolving";
  model.resolveAt = now + 900;
  concludeIfNeeded(model);
}

function commitWeapon(
  model: GameModel,
  owner: FighterIndex,
  weapon: Weapon,
  angle: number,
  power: number,
  now: number,
) {
  if (model.phase !== "planning" || model.active !== owner) return false;
  const remaining = model.ammo[owner][weapon.id] ?? weapon.ammo;
  if (Number.isFinite(remaining) && remaining <= 0) {
    announce(model, `${weapon.name} is out of charges.`);
    return false;
  }
  if (Number.isFinite(remaining)) model.ammo[owner][weapon.id] = remaining - 1;

  model.phase = "resolving";
  model.timer = Math.max(0, model.timer);
  if (weapon.id === "heavy-revolver") {
    model.players[owner].x -= model.players[owner].facing * 24;
    settleFighter(model, model.players[owner]);
  }
  if (weapon.mode === "strike") {
    resolveStrike(model, owner, weapon, now);
    return true;
  }

  const fighter = model.players[owner];
  const radians = (angle * Math.PI) / 180;
  const baseSpeed = weapon.id === "needle-7" || weapon.id === "heavy-revolver"
    ? 610
    : 150 + power * 3.15;
  model.projectile = {
    x: fighter.x + fighter.facing * 26,
    y: fighter.y - 23,
    vx: Math.cos(radians) * baseSpeed * fighter.facing,
    vy: -Math.sin(radians) * baseSpeed,
    age: 0,
    owner,
    weaponId: weapon.id,
    drillRemaining: weapon.id === "mole-drill" ? 86 : 0,
  };
  announce(model, `${fighter.name} fires ${weapon.name}.`);
  return true;
}

function updateProjectile(model: GameModel, dt: number, now: number) {
  const projectile = model.projectile;
  if (!projectile) return;
  const weapon = WEAPON_BY_ID.get(projectile.weaponId) ?? WEAPONS[0];
  projectile.age += dt;

  if (weapon.id === "returning-boomerang" && projectile.age > 0.78) {
    const owner = model.players[projectile.owner];
    const dx = owner.x - projectile.x;
    const dy = owner.y - 24 - projectile.y;
    projectile.vx += Math.sign(dx) * 460 * dt;
    projectile.vy += Math.sign(dy) * 230 * dt;
  }

  projectile.vx += model.wind * 7.2 * (weapon.wind ?? 1) * dt;
  projectile.vy += 285 * (weapon.gravity ?? 1) * dt;
  projectile.x += projectile.vx * dt;
  projectile.y += projectile.vy * dt;

  const targetIndex: FighterIndex = projectile.owner === 0 ? 1 : 0;
  const target = model.players[targetIndex];
  const hitTarget = Math.hypot(projectile.x - target.x, projectile.y - (target.y - 9)) < 19;
  const hitTerrain = isSolid(model.terrain, projectile.x, projectile.y);

  if (hitTerrain && projectile.drillRemaining > 0) {
    carveCircle(model.terrain, projectile.x, projectile.y, 5);
    projectile.drillRemaining -= Math.hypot(projectile.vx * dt, projectile.vy * dt);
    projectile.vx *= 0.992;
    projectile.vy *= 0.992;
    return;
  }

  if (hitTarget || hitTerrain) {
    const impactX = projectile.x;
    const impactY = projectile.y;
    model.projectile = null;
    applyBlast(model, impactX, impactY, weapon, projectile.owner);
    announce(model, hitTarget ? `Direct hit — ${weapon.name} finds its mark.` : `${weapon.name} tears into the dungeon.`);
    if (!concludeIfNeeded(model)) model.resolveAt = now + 1100;
    return;
  }

  if (
    projectile.x < -90 ||
    projectile.x > VIEW_WIDTH + 90 ||
    projectile.y > VIEW_HEIGHT + 80 ||
    projectile.age > 8
  ) {
    model.projectile = null;
    announce(model, `${weapon.name} vanishes into the deep.`);
    model.resolveAt = now + 700;
  }
}

function fireBot(model: GameModel, now: number) {
  if (model.active !== 1 || model.phase !== "planning") return;
  model.botAt = 0;
  const weapon = equippedWeapon(model, 1, "main");
  const distance = Math.abs(model.players[1].x - model.players[0].x);
  const angle = 44 + nextRandom(model) * 12;
  const windCorrection = model.wind * 1.4;
  const power = Math.max(45, Math.min(90, 48 + distance / 15 - windCorrection + (nextRandom(model) - 0.5) * 8));
  commitWeapon(model, 1, weapon, angle, power, now);
}

function updateModel(model: GameModel, dt: number, now: number) {
  model.effects.forEach((effect) => {
    effect.age += dt;
  });
  model.effects = model.effects.filter((effect) => effect.age < effect.duration);

  if (model.projectile) updateProjectile(model, dt, now);
  if (model.phase === "planning") {
    model.timer = Math.max(0, model.timer - dt);
    if (model.timer <= 0) {
      announce(model, `${model.players[model.active].name} runs out of time.`);
      transitionTurn(model, now);
    } else if (model.active === 1 && model.botAt > 0 && now >= model.botAt) {
      fireBot(model, now);
    }
  }
  if (model.phase === "resolving" && model.resolveAt > 0 && now >= model.resolveAt) {
    transitionTurn(model, now);
  }
}

function drawBackdrop(context: CanvasRenderingContext2D) {
  const gradient = context.createLinearGradient(0, 0, 0, VIEW_HEIGHT);
  gradient.addColorStop(0, "#161329");
  gradient.addColorStop(0.52, "#29203a");
  gradient.addColorStop(1, "#0c0b14");
  context.fillStyle = gradient;
  context.fillRect(0, 0, VIEW_WIDTH, VIEW_HEIGHT);

  context.save();
  context.globalAlpha = 0.34;
  context.strokeStyle = "#6b5b79";
  context.lineWidth = 10;
  [95, 275, 685, 865].forEach((x, index) => {
    context.beginPath();
    context.moveTo(x - 66, VIEW_HEIGHT);
    context.lineTo(x - 66, 160 + (index % 2) * 34);
    context.arc(x, 160 + (index % 2) * 34, 66, Math.PI, 0);
    context.lineTo(x + 66, VIEW_HEIGHT);
    context.stroke();
  });
  context.lineWidth = 2;
  context.strokeStyle = "#b58f72";
  for (let y = 78; y < 380; y += 48) {
    context.beginPath();
    context.moveTo(0, y);
    context.lineTo(VIEW_WIDTH, y + 30);
    context.stroke();
  }
  context.restore();

  for (let i = 0; i < 38; i += 1) {
    const x = seededNoise(i * 9.2) * VIEW_WIDTH;
    const y = 55 + seededNoise(i * 18.3) * 260;
    const radius = 0.7 + seededNoise(i * 4.7) * 1.5;
    context.fillStyle = i % 4 === 0 ? "#ffc46c77" : "#d9c7ff35";
    context.beginPath();
    context.arc(x, y, radius, 0, Math.PI * 2);
    context.fill();
  }

  const haze = context.createRadialGradient(480, 280, 15, 480, 280, 310);
  haze.addColorStop(0, "#b45d4a30");
  haze.addColorStop(1, "#241a3300");
  context.fillStyle = haze;
  context.fillRect(0, 0, VIEW_WIDTH, VIEW_HEIGHT);
}

function drawTrajectory(context: CanvasRenderingContext2D, model: GameModel) {
  if (model.active !== 0 || model.phase !== "planning") return;
  const weapon = equippedWeapon(model);
  if (weapon.mode !== "projectile") return;
  const player = model.players[0];
  const radians = (model.angle * Math.PI) / 180;
  const speed = weapon.id === "needle-7" || weapon.id === "heavy-revolver"
    ? 610
    : 150 + model.power * 3.15;
  let x = player.x + player.facing * 26;
  let y = player.y - 23;
  let vx = Math.cos(radians) * speed * player.facing;
  let vy = -Math.sin(radians) * speed;
  context.save();
  for (let i = 0; i < 22; i += 1) {
    const dt = 0.045;
    vx += model.wind * 7.2 * (weapon.wind ?? 1) * dt;
    vy += 285 * (weapon.gravity ?? 1) * dt;
    x += vx * dt;
    y += vy * dt;
    if (i % 2 === 0) {
      context.globalAlpha = Math.max(0.12, 0.8 - i / 27);
      context.fillStyle = weapon.color;
      context.beginPath();
      context.arc(x, y, Math.max(1.5, 3.4 - i * 0.08), 0, Math.PI * 2);
      context.fill();
    }
    if (isSolid(model.terrain, x, y)) break;
  }
  context.restore();
}

function drawWeapon(context: CanvasRenderingContext2D, weapon: Weapon, facing: Facing) {
  context.save();
  context.scale(facing, 1);
  context.strokeStyle = weapon.color;
  context.fillStyle = "#211a29";
  context.lineWidth = 3;
  if (weapon.mode === "strike") {
    context.beginPath();
    context.moveTo(7, -17);
    context.lineTo(34, -38);
    context.stroke();
    context.fillStyle = weapon.color;
    context.fillRect(29, -43, 13, 6);
  } else {
    context.beginPath();
    context.roundRect(7, -29, 31, 11, 4);
    context.fill();
    context.stroke();
    context.fillStyle = weapon.color;
    context.fillRect(33, -26, 14, 4);
  }
  context.restore();
}

function drawFighter(context: CanvasRenderingContext2D, fighter: Fighter, weapon: Weapon) {
  const { appearance } = fighter;
  context.save();
  context.translate(fighter.x, fighter.y);
  context.shadowColor = "#00000088";
  context.shadowBlur = 10;
  context.fillStyle = "#08071077";
  context.beginPath();
  context.ellipse(0, 18, 24, 7, 0, 0, Math.PI * 2);
  context.fill();
  context.shadowBlur = 0;

  context.fillStyle = appearance.outfitColor;
  context.beginPath();
  context.roundRect(-18, -18, 36, 38, 12);
  context.fill();
  context.fillStyle = appearance.accentColor;
  context.fillRect(-18, 6, 36, 6);
  context.fillStyle = "#16121e";
  context.fillRect(-13, 15, 9, 7);
  context.fillRect(4, 15, 9, 7);

  context.fillStyle = appearance.skinTone;
  context.beginPath();
  context.arc(0, -30, 17, 0, Math.PI * 2);
  context.fill();

  context.fillStyle = appearance.hairColor;
  if (appearance.hair === "crest") {
    context.beginPath();
    context.moveTo(-17, -35);
    context.lineTo(-5, -52);
    context.lineTo(0, -42);
    context.lineTo(11, -55);
    context.lineTo(17, -33);
    context.closePath();
    context.fill();
  } else if (appearance.hair === "mop") {
    [-13, -5, 4, 12].forEach((x, index) => {
      context.beginPath();
      context.arc(x, -42 + (index % 2) * 3, 9, 0, Math.PI * 2);
      context.fill();
    });
  } else {
    context.beginPath();
    context.arc(0, -31, 23, Math.PI, 0);
    context.lineTo(21, -13);
    context.lineTo(-21, -13);
    context.closePath();
    context.fill();
  }

  context.fillStyle = "#211621";
  context.beginPath();
  context.arc(fighter.facing * 6 - 3, -29, 2.2, 0, Math.PI * 2);
  context.arc(fighter.facing * 6 + 4, -29, 2.2, 0, Math.PI * 2);
  context.fill();
  drawWeapon(context, weapon, fighter.facing);
  context.restore();

  context.save();
  context.textAlign = "center";
  context.font = "700 12px ui-monospace, monospace";
  context.fillStyle = "#ffffff";
  context.fillText(fighter.name, fighter.x, fighter.y - 73);
  context.fillStyle = "#090711aa";
  context.fillRect(fighter.x - 28, fighter.y - 66, 56, 6);
  const healthWidth = 56 * (fighter.health / 200);
  context.fillStyle = fighter.health > 70 ? "#93dc80" : "#ff6f6f";
  context.fillRect(fighter.x - 28, fighter.y - 66, healthWidth, 6);
  context.restore();
}

function drawEffects(context: CanvasRenderingContext2D, model: GameModel) {
  model.effects.forEach((effect) => {
    const progress = effect.age / effect.duration;
    context.save();
    context.globalAlpha = Math.max(0, 1 - progress);
    context.strokeStyle = effect.color;
    context.fillStyle = `${effect.color}55`;
    context.lineWidth = effect.kind === "slash" ? 7 : 4;
    if (effect.kind === "slash") {
      context.beginPath();
      context.arc(effect.x, effect.y, effect.radius * (0.6 + progress * 0.5), -1.2, 1.2);
      context.stroke();
    } else {
      context.beginPath();
      context.arc(effect.x, effect.y, effect.radius * (0.25 + progress), 0, Math.PI * 2);
      context.fill();
      context.stroke();
    }
    context.restore();
  });
}

function drawGame(canvas: HTMLCanvasElement, model: GameModel) {
  const context = canvas.getContext("2d");
  if (!context) return;
  context.clearRect(0, 0, VIEW_WIDTH, VIEW_HEIGHT);
  drawBackdrop(context);
  context.drawImage(model.terrain.canvas, 0, 0);
  drawTrajectory(context, model);
  model.players.forEach((fighter, index) => {
    const slot = index === model.active ? model.selectedSlot : "main";
    drawFighter(context, fighter, equippedWeapon(model, index as FighterIndex, slot));
  });

  if (model.projectile) {
    const weapon = WEAPON_BY_ID.get(model.projectile.weaponId) ?? WEAPONS[0];
    context.save();
    context.shadowColor = weapon.color;
    context.shadowBlur = 18;
    context.fillStyle = "#ffffff";
    context.beginPath();
    context.arc(model.projectile.x, model.projectile.y, weapon.id === "returning-boomerang" ? 7 : 5, 0, Math.PI * 2);
    context.fill();
    context.strokeStyle = weapon.color;
    context.lineWidth = 3;
    context.beginPath();
    context.moveTo(model.projectile.x, model.projectile.y);
    context.lineTo(
      model.projectile.x - model.projectile.vx * 0.045,
      model.projectile.y - model.projectile.vy * 0.045,
    );
    context.stroke();
    context.restore();
  }
  drawEffects(context, model);

  const vignette = context.createRadialGradient(480, 270, 230, 480, 270, 590);
  vignette.addColorStop(0, "#00000000");
  vignette.addColorStop(1, "#05040dcc");
  context.fillStyle = vignette;
  context.fillRect(0, 0, VIEW_WIDTH, VIEW_HEIGHT);
}

function AvatarPortrait({ appearance }: { appearance: Appearance }) {
  return (
    <div
      className={`avatar-portrait hair-${appearance.hair}`}
      style={
        {
          "--hair": appearance.hairColor,
          "--outfit": appearance.outfitColor,
          "--accent": appearance.accentColor,
          "--skin": appearance.skinTone,
        } as React.CSSProperties
      }
      aria-label="Live layered character preview"
    >
      <span className="avatar-glow" />
      <span className="avatar-body" />
      <span className="avatar-belt" />
      <span className="avatar-head" />
      <span className="avatar-hair" />
      <span className="avatar-eyes" />
      <span className="avatar-blade" />
    </div>
  );
}

function formatAmmo(value: number) {
  return Number.isFinite(value) ? String(Math.max(0, value)) : "∞";
}

export function DungeonBarrageGame() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const modelRef = useRef<GameModel | null>(null);
  const frameRef = useRef<number | null>(null);
  const [revision, setRevision] = useState(0);
  const [armoryOpen, setArmoryOpen] = useState(false);
  const [armoryTab, setArmoryTab] = useState<"loadout" | "appearance">("loadout");

  const bump = useCallback(() => setRevision((value) => value + 1), []);

  const startFreshMatch = useCallback(() => {
    const terrain = createTerrain();
    modelRef.current = createModel(terrain);
    setArmoryOpen(false);
    bump();
  }, [bump]);

  useEffect(() => {
    startFreshMatch();
  }, [startFreshMatch]);

  useEffect(() => {
    let last = performance.now();
    let uiClock = 0;
    const tick = (now: number) => {
      const model = modelRef.current;
      const canvas = canvasRef.current;
      const dt = Math.min(0.035, Math.max(0, (now - last) / 1000));
      last = now;
      if (model && canvas) {
        updateModel(model, dt, now);
        drawGame(canvas, model);
        uiClock += dt;
        if (uiClock >= 0.18) {
          uiClock = 0;
          setRevision((value) => value + 1);
        }
      }
      frameRef.current = requestAnimationFrame(tick);
    };
    frameRef.current = requestAnimationFrame(tick);
    return () => {
      if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    };
  }, []);

  const moveActive = useCallback(
    (direction: Facing) => {
      const model = modelRef.current;
      if (!model || model.active !== 0 || model.phase !== "planning" || model.movement <= 0) return;
      const fighter = model.players[0];
      fighter.facing = direction;
      fighter.x += direction * 18;
      fighter.y = findGround(model.terrain, fighter.x, Math.max(0, fighter.y - 28));
      model.movement = Math.max(0, model.movement - 18);
      bump();
    },
    [bump],
  );

  const fireCurrent = useCallback(() => {
    const model = modelRef.current;
    if (!model || model.active !== 0) return;
    commitWeapon(model, 0, equippedWeapon(model), model.angle, model.power, performance.now());
    bump();
  }, [bump]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target && ["INPUT", "SELECT", "BUTTON"].includes(target.tagName)) return;
      const model = modelRef.current;
      if (!model) return;
      if (event.key === "1" || event.key === "2" || event.key === "3") {
        model.selectedSlot = event.key === "1" ? "main" : event.key === "2" ? "secondary" : "melee";
        bump();
      } else if (event.key === "ArrowUp") {
        model.angle = Math.min(85, model.angle + 1);
        bump();
      } else if (event.key === "ArrowDown") {
        model.angle = Math.max(5, model.angle - 1);
        bump();
      } else if (event.key.toLowerCase() === "a") {
        moveActive(-1);
      } else if (event.key.toLowerCase() === "d") {
        moveActive(1);
      } else if (event.code === "Space") {
        event.preventDefault();
        fireCurrent();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [bump, fireCurrent, moveActive]);

  const model = modelRef.current;
  const activeWeapon = model ? equippedWeapon(model) : WEAPONS[0];
  const player = model?.players[0];
  const bot = model?.players[1];

  const weaponGroups = useMemo(
    () => ({
      main: WEAPONS.filter((weapon) => weapon.slot === "main"),
      secondary: WEAPONS.filter((weapon) => weapon.slot === "secondary"),
      melee: WEAPONS.filter((weapon) => weapon.slot === "melee"),
    }),
    [],
  );

  const changeEquippedWeapon = (slot: Slot, weaponId: string) => {
    const current = modelRef.current;
    if (!current) return;
    refreshLoadoutAmmo(current, 0, slot, weaponId);
    current.selectedSlot = slot;
    bump();
  };

  const changeAppearance = (patch: Partial<Appearance>) => {
    const current = modelRef.current;
    if (!current) return;
    Object.assign(current.players[0].appearance, patch);
    bump();
  };

  const currentAppearance = player?.appearance ?? DEFAULT_APPEARANCE;
  const canFire = Boolean(model && model.active === 0 && model.phase === "planning");

  return (
    <main className="barrage-app" data-revision={revision}>
      <header className="topbar">
        <div className="brand-lockup" aria-label="Dungeon Barrage">
          <span className="brand-rune" aria-hidden="true">DB</span>
          <span>
            <strong>DUNGEON</strong>
            <b>BARRAGE</b>
          </span>
        </div>
        <div className="match-chip">
          <span className="status-light" />
          LOCAL DUEL <em>·</em> VAULT 01
        </div>
        <nav className="top-actions" aria-label="Game actions">
          <button className="quiet-button" type="button" onClick={() => setArmoryOpen(true)}>
            <span aria-hidden="true">✦</span> Armory
          </button>
          <button className="primary-small" type="button" onClick={startFreshMatch}>
            New match
          </button>
        </nav>
      </header>

      <section className="game-frame" aria-label="Dungeon Barrage playable vertical slice">
        <div className="canvas-wrap">
          <canvas
            ref={canvasRef}
            width={VIEW_WIDTH}
            height={VIEW_HEIGHT}
            aria-label="Destructible dungeon battlefield. Use A and D to move, arrow keys to aim, number keys to switch weapons, and Space to fire."
          />

          <div className="battle-topline" aria-live="polite">
            <div className="round-marker">
              <span>TURN</span>
              <strong>{String(model?.turn ?? 1).padStart(2, "0")}</strong>
            </div>
            <div className="wind-meter">
              <span>WIND</span>
              <strong className={(model?.wind ?? 0) < 0 ? "wind-left" : ""}>
                <i aria-hidden="true">➜</i> {Math.abs(model?.wind ?? 0).toFixed(1)}
              </strong>
            </div>
            <div className={`turn-clock ${(model?.timer ?? 25) < 8 ? "urgent" : ""}`}>
              <span>{model?.active === 0 ? "YOUR MOVE" : "RIVAL THINKING"}</span>
              <strong>{Math.ceil(model?.timer ?? TURN_SECONDS)}</strong>
            </div>
          </div>

          <div className="fighter-card fighter-card-player">
            <span className="fighter-label">YOU · DELVER</span>
            <strong>{player?.name ?? "Nova"}</strong>
            <div className="health-track"><span style={{ width: `${(player?.health ?? 200) / 2}%` }} /></div>
            <small>{player?.health ?? 200} HP · {player?.status ?? "READY"}</small>
          </div>

          <div className="fighter-card fighter-card-bot">
            <span className="fighter-label">RIVAL · BOT</span>
            <strong>{bot?.name ?? "Morrow"}</strong>
            <div className="health-track"><span style={{ width: `${(bot?.health ?? 200) / 2}%` }} /></div>
            <small>{bot?.health ?? 200} HP · {bot?.status ?? "READY"}</small>
          </div>

          <div className="battle-message">{model?.message ?? "Preparing the dungeon…"}</div>

          {model?.phase === "finished" && (
            <div className="victory-panel" role="dialog" aria-modal="true" aria-label="Match complete">
              <span>THE VAULT ANSWERS TO</span>
              <h2>{model.winner}</h2>
              <p>{model.winner === "Nova" ? "Your barrage broke the rival line." : "Morrow takes this round. Rebuild and return."}</p>
              <button type="button" onClick={startFreshMatch}>Rematch</button>
            </div>
          )}
        </div>

        <div className="command-deck">
          <div className="movement-cluster">
            <span className="deck-label">POSITION</span>
            <div className="move-buttons">
              <button type="button" onClick={() => moveActive(-1)} disabled={!canFire} aria-label="Move left">A</button>
              <button type="button" onClick={() => moveActive(1)} disabled={!canFire} aria-label="Move right">D</button>
            </div>
            <div className="movement-readout"><i style={{ width: `${model?.movement ?? 100}%` }} /></div>
          </div>

          <div className="weapon-rack" role="tablist" aria-label="Equipped weapon slots">
            {(["main", "secondary", "melee"] as const).map((slot, index) => {
              const weapon = model ? equippedWeapon(model, 0, slot) : WEAPONS[index];
              const selected = model?.selectedSlot === slot;
              const ammo = model?.ammo[0][weapon.id] ?? weapon.ammo;
              return (
                <button
                  key={slot}
                  type="button"
                  role="tab"
                  aria-selected={selected}
                  className={selected ? "weapon-slot selected" : "weapon-slot"}
                  onClick={() => {
                    if (!modelRef.current) return;
                    modelRef.current.selectedSlot = slot;
                    bump();
                  }}
                >
                  <span className="slot-number">{index + 1}</span>
                  <i style={{ "--weapon-color": weapon.color } as React.CSSProperties}>{weapon.glyph}</i>
                  <span>
                    <small>{slot === "secondary" ? "SIDEARM" : slot.toUpperCase()}</small>
                    <strong>{weapon.name}</strong>
                  </span>
                  <b>{formatAmmo(ammo)}</b>
                </button>
              );
            })}
          </div>

          <div className="aim-controls">
            <label>
              <span><small>ANGLE</small><strong>{Math.round(model?.angle ?? 48)}°</strong></span>
              <input
                type="range"
                min="5"
                max="85"
                value={model?.angle ?? 48}
                disabled={!canFire || activeWeapon.mode === "strike"}
                onChange={(event) => {
                  if (!modelRef.current) return;
                  modelRef.current.angle = Number(event.target.value);
                  bump();
                }}
              />
            </label>
            <label>
              <span><small>POWER</small><strong>{Math.round(model?.power ?? 68)}</strong></span>
              <input
                type="range"
                min="25"
                max="100"
                value={model?.power ?? 68}
                disabled={!canFire || activeWeapon.mode === "strike"}
                onChange={(event) => {
                  if (!modelRef.current) return;
                  modelRef.current.power = Number(event.target.value);
                  bump();
                }}
              />
            </label>
          </div>

          <button className="fire-button" type="button" disabled={!canFire} onClick={fireCurrent}>
            <span>{activeWeapon.mode === "strike" ? "STRIKE" : "FIRE"}</span>
            <small>SPACE</small>
          </button>
        </div>

        <div className="weapon-intel">
          <span className="weapon-glyph" style={{ "--weapon-color": activeWeapon.color } as React.CSSProperties}>{activeWeapon.glyph}</span>
          <div>
            <small>{activeWeapon.tag}</small>
            <strong>{activeWeapon.name}</strong>
            <p>{activeWeapon.summary}</p>
          </div>
          <dl>
            <div><dt>DAMAGE</dt><dd>{activeWeapon.damage}</dd></div>
            <div><dt>TERRAIN</dt><dd>{activeWeapon.crater ? activeWeapon.crater : "—"}</dd></div>
            <div><dt>AMMO</dt><dd>{formatAmmo(model?.ammo[0][activeWeapon.id] ?? activeWeapon.ammo)}</dd></div>
          </dl>
          <button type="button" onClick={() => setArmoryOpen(true)}>Edit loadout</button>
        </div>

        {armoryOpen && (
          <div className="armory-scrim" role="presentation" onMouseDown={(event) => {
            if (event.currentTarget === event.target) setArmoryOpen(false);
          }}>
            <aside className="armory-panel" role="dialog" aria-modal="true" aria-labelledby="armory-title">
              <header>
                <div>
                  <small>VAULT LOADOUT</small>
                  <h2 id="armory-title">Armory</h2>
                </div>
                <button type="button" aria-label="Close armory" onClick={() => setArmoryOpen(false)}>×</button>
              </header>

              <div className="armory-tabs" role="tablist">
                <button type="button" role="tab" aria-selected={armoryTab === "loadout"} onClick={() => setArmoryTab("loadout")}>Loadout</button>
                <button type="button" role="tab" aria-selected={armoryTab === "appearance"} onClick={() => setArmoryTab("appearance")}>Appearance</button>
              </div>

              {armoryTab === "loadout" ? (
                <div className="loadout-editor">
                  <div className="loadout-rule">
                    <b>3 / 3</b>
                    <span>Every delver carries exactly one Main, one Sidearm, and one Melee/Tool.</span>
                  </div>
                  {(["main", "secondary", "melee"] as const).map((slot, index) => {
                    const equipped = model ? equippedWeapon(model, 0, slot) : weaponGroups[slot][0];
                    return (
                      <section className="armory-slot" key={slot}>
                        <div className="armory-slot-heading">
                          <span>0{index + 1}</span>
                          <div>
                            <small>{slot === "secondary" ? "SIDEARM" : slot.toUpperCase()}</small>
                            <strong>{equipped.name}</strong>
                          </div>
                          <i style={{ "--weapon-color": equipped.color } as React.CSSProperties}>{equipped.glyph}</i>
                        </div>
                        <select
                          aria-label={`Choose ${slot} weapon`}
                          value={equipped.id}
                          onChange={(event) => changeEquippedWeapon(slot, event.target.value)}
                        >
                          {weaponGroups[slot].map((weapon) => (
                            <option key={weapon.id} value={weapon.id}>{weapon.name} · {weapon.tag}</option>
                          ))}
                        </select>
                        <p>{equipped.summary}</p>
                        <div className="skin-row">
                          <span>SKIN</span>
                          {["Foundry", "Moonsteel", "Relic"].map((skin) => (
                            <button
                              key={skin}
                              type="button"
                              className={model?.weaponSkins[slot] === skin ? "active" : ""}
                              onClick={() => {
                                if (!modelRef.current) return;
                                modelRef.current.weaponSkins[slot] = skin;
                                bump();
                              }}
                            >
                              {skin}
                            </button>
                          ))}
                        </div>
                      </section>
                    );
                  })}
                  <p className="cosmetic-note">Skins change only color, trail, impact decal, and sound. Range, sockets, damage, ammo, and terrain behavior stay identical.</p>
                </div>
              ) : (
                <div className="appearance-editor">
                  <AvatarPortrait appearance={currentAppearance} />
                  <div className="appearance-copy">
                    <small>LAYERED FIGHTER</small>
                    <strong>Nova · Delver</strong>
                    <p>Hair, face, outfit, accent, and equipped weapon render as separate layers.</p>
                  </div>
                  <fieldset>
                    <legend>Hair shape</legend>
                    <div className="segmented-options">
                      {(["crest", "mop", "hood"] as const).map((hair) => (
                        <button key={hair} type="button" className={currentAppearance.hair === hair ? "active" : ""} onClick={() => changeAppearance({ hair })}>{hair}</button>
                      ))}
                    </div>
                  </fieldset>
                  <fieldset>
                    <legend>Hair color</legend>
                    <div className="swatches">
                      {["#36283f", "#d68b45", "#7fd4cc", "#e7d4f6", "#232940"].map((color) => (
                        <button key={color} type="button" aria-label={`Hair color ${color}`} className={currentAppearance.hairColor === color ? "active" : ""} style={{ background: color }} onClick={() => changeAppearance({ hairColor: color })} />
                      ))}
                    </div>
                  </fieldset>
                  <fieldset>
                    <legend>Outfit dye</legend>
                    <div className="swatches">
                      {["#ef6e52", "#6d86d8", "#63b896", "#b06ac7", "#d39b4a"].map((color) => (
                        <button key={color} type="button" aria-label={`Outfit color ${color}`} className={currentAppearance.outfitColor === color ? "active" : ""} style={{ background: color }} onClick={() => changeAppearance({ outfitColor: color })} />
                      ))}
                    </div>
                  </fieldset>
                  <fieldset>
                    <legend>Accent dye</legend>
                    <div className="swatches">
                      {["#ffc45f", "#9ce7ff", "#c4ff8f", "#ff8bb8", "#eee5d6"].map((color) => (
                        <button key={color} type="button" aria-label={`Accent color ${color}`} className={currentAppearance.accentColor === color ? "active" : ""} style={{ background: color }} onClick={() => changeAppearance({ accentColor: color })} />
                      ))}
                    </div>
                  </fieldset>
                </div>
              )}

              <footer>
                <span>Changes apply instantly</span>
                <button type="button" onClick={() => setArmoryOpen(false)}>Return to battle</button>
              </footer>
            </aside>
          </div>
        )}
      </section>

      <footer className="app-footer">
        <span><i /> DETERMINISTIC LOCAL SIMULATION</span>
        <span>A / D MOVE · ↑ / ↓ AIM · 1 / 2 / 3 SWITCH · SPACE FIRE</span>
        <span>WEB VERTICAL SLICE · BUILD 0.1</span>
      </footer>
    </main>
  );
}
