import assert from "node:assert/strict";
import test from "node:test";

import {
  BASE_MELEE_RANGE,
  FIXED_TICK_RATE,
  LAUNCH_WEAPONS,
  POSITION_SCALE,
  WEAPON_DEFINITIONS,
  applyTerrainOperation,
  applyWeaponCommand,
  cloneTerrainMask,
  createSimulationState,
  createTerrainMask,
  hashSimulationState,
  hashTerrainMask,
  remainingAmmo,
  sampleWeaponTrajectory,
  setTerrainCell,
  terrainCell,
  validateLaunchRoster,
  validateLoadout,
  type Loadout,
  type TerrainMask,
} from "../lib/game/simulation.ts";

const finiteLoadout: Loadout = {
  main: { weaponId: "ramshot-cannon", skinId: "ramshot-bronze" },
  offHand: { weaponId: "recurve-bow", skinId: "bow-ashwood" },
  melee: { weaponId: "trench-spade", skinId: "spade-field" },
};

const swordLoadout: Loadout = {
  main: { weaponId: "frostfall-mortar", skinId: "frostfall-default" },
  offHand: { weaponId: "longsword", skinId: "longsword-silver" },
  melee: { weaponId: "blood-maul", skinId: "maul-iron" },
};

function terrainWithGround(width = 80, height = 48, groundY = 32): TerrainMask {
  const terrain = createTerrainMask(width, height);
  for (let y = groundY; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) setTerrainCell(terrain, x, y, true);
  }
  return terrain;
}

function makeState(loadout: Loadout = finiteLoadout, terrain = terrainWithGround()) {
  return createSimulationState({
    terrain,
    activePlayerId: "p1",
    turnId: "turn-7",
    windPerTick: 3,
    players: [
      {
        id: "p1",
        position: { x: 10 * POSITION_SCALE, y: 24 * POSITION_SCALE },
        loadout,
        avatar: { hairId: "hair-a", outfitId: "coat-a", paletteId: "warm" },
      },
    ],
  });
}

test("the canonical launch roster contains four mains, five off-hands, and three melee/tools", () => {
  assert.deepEqual(validateLaunchRoster(), []);
  assert.equal(LAUNCH_WEAPONS.length, 12);
  assert.deepEqual(
    LAUNCH_WEAPONS.map((weapon) => weapon.id),
    [
      "ramshot-cannon",
      "frostfall-mortar",
      "mole-drill",
      "cinder-cluster",
      "recurve-bow",
      "longsword",
      "returning-boomerang",
      "needle-7",
      "heavy-revolver",
      "trench-spade",
      "blood-maul",
      "breach-pick",
    ],
  );
  assert.deepEqual(
    ["main", "offHand", "melee"].map(
      (slot) => LAUNCH_WEAPONS.filter((weapon) => weapon.slot === slot).length,
    ),
    [4, 5, 3],
  );
  assert.ok(
    LAUNCH_WEAPONS.filter((weapon) => weapon.slot === "main").every(
      (weapon) => weapon.specialEffects.length > 0,
    ),
  );
});

test("Longsword is the sole infinite weapon and has exactly twice standard melee reach", () => {
  const infinite = LAUNCH_WEAPONS.filter((weapon) => weapon.ammo.kind === "infinite");
  assert.deepEqual(infinite.map((weapon) => weapon.id), ["longsword"]);
  const longsword = WEAPON_DEFINITIONS.longsword;
  assert.equal(longsword.attack.kind, "strike");
  if (longsword.attack.kind !== "strike") assert.fail("Longsword must be a strike weapon");
  assert.equal(longsword.attack.range, BASE_MELEE_RANGE * 2);
});

test("loadout validation requires exactly one correctly typed weapon in every slot", () => {
  assert.deepEqual(validateLoadout(finiteLoadout), { valid: true, errors: [] });

  const missingMelee = validateLoadout({
    main: finiteLoadout.main,
    offHand: finiteLoadout.offHand,
  });
  assert.equal(missingMelee.valid, false);
  assert.match(missingMelee.errors.join(" "), /Missing melee weapon slot/);

  const extraSlot = validateLoadout({ ...finiteLoadout, utility: { weaponId: "longsword" } });
  assert.equal(extraSlot.valid, false);
  assert.match(extraSlot.errors.join(" "), /Unexpected loadout slot: utility/);

  const crossWired = validateLoadout({
    ...finiteLoadout,
    main: { weaponId: "longsword" },
  });
  assert.equal(crossWired.valid, false);
  assert.match(crossWired.errors.join(" "), /belongs in offHand, not main/);

  const unknown = validateLoadout({
    ...finiteLoadout,
    melee: { weaponId: "mystery-stick" },
  });
  assert.equal(unknown.valid, false);
  assert.match(unknown.errors.join(" "), /Unknown weapon/);
});

test("state hashing ignores weapon skins and layered avatar cosmetics", () => {
  const terrain = terrainWithGround();
  const first = makeState(finiteLoadout, terrain);
  const reskinnedLoadout: Loadout = {
    main: { weaponId: "ramshot-cannon", skinId: "ramshot-royal" },
    offHand: { weaponId: "recurve-bow", skinId: "bow-neon" },
    melee: { weaponId: "trench-spade", skinId: "spade-gilded" },
  };
  const second = createSimulationState({
    terrain,
    activePlayerId: "p1",
    turnId: "turn-7",
    windPerTick: 3,
    players: [
      {
        id: "p1",
        position: { x: 10 * POSITION_SCALE, y: 24 * POSITION_SCALE },
        loadout: reskinnedLoadout,
        avatar: {
          bodyId: "body-b",
          hairId: "hair-b",
          outfitId: "armor-b",
          accessoryId: "wings-b",
          paletteId: "cool",
        },
      },
    ],
  });

  assert.equal(hashSimulationState(first), hashSimulationState(second));

  const changedTerrain = cloneTerrainMask(terrain);
  setTerrainCell(changedTerrain, 0, 0, true);
  const gameplayChange = makeState(finiteLoadout, changedTerrain);
  assert.notEqual(hashSimulationState(first), hashSimulationState(gameplayChange));
});

test("ballistic sampling is fixed-step, integer-valued, and reproducible", () => {
  const terrain = terrainWithGround();
  const input = {
    origin: { x: 8 * POSITION_SCALE, y: 22 * POSITION_SCALE },
    angleMilliDegrees: 47_500,
    powerBasisPoints: 8_250,
    windPerTick: 4,
    terrain,
  };
  const first = sampleWeaponTrajectory("ramshot-cannon", input);
  const second = sampleWeaponTrajectory("ramshot-cannon", input);

  assert.deepEqual(first, second);
  assert.equal(first.samples.length, first.impact.tick + 1);
  assert.ok(first.samples.length > 2);
  for (const [index, sample] of first.samples.entries()) {
    assert.equal(sample.tick, index);
    assert.equal(sample.timeMs, Math.round((index * 1_000) / FIXED_TICK_RATE));
    assert.ok(Number.isInteger(sample.x));
    assert.ok(Number.isInteger(sample.y));
    assert.ok(Number.isInteger(sample.velocityX));
    assert.ok(Number.isInteger(sample.velocityY));
  }

  const stillAir = sampleWeaponTrajectory("needle-7", { ...input, windPerTick: 0 });
  const windy = sampleWeaponTrajectory("needle-7", { ...input, windPerTick: 50 });
  assert.deepEqual(stillAir, windy, "Needle-7 is canonically wind-neutral");
  const bowStill = sampleWeaponTrajectory("recurve-bow", { ...input, windPerTick: 0 });
  const bowWindy = sampleWeaponTrajectory("recurve-bow", { ...input, windPerTick: 50 });
  assert.notDeepEqual(bowStill.samples[1], bowWindy.samples[1]);
});

test("crater and dig terrain operations are deterministic integer-mask subtraction", () => {
  const craterA = createTerrainMask(20, 20, 1);
  const craterB = createTerrainMask(20, 20, 1);
  const crater = { kind: "crater" as const, centerX: 10, centerY: 10, radius: 3 };
  assert.equal(applyTerrainOperation(craterA, crater), 29);
  assert.equal(applyTerrainOperation(craterB, crater), 29);
  assert.equal(applyTerrainOperation(craterA, crater), 0, "reapplying an operation is stable");
  assert.equal(hashTerrainMask(craterA), hashTerrainMask(craterB));
  assert.equal(terrainCell(craterA, 10, 10), 0);
  assert.equal(terrainCell(craterA, 13, 10), 0);
  assert.equal(terrainCell(craterA, 14, 10), 1);

  const digA = createTerrainMask(20, 20, 1);
  const digB = createTerrainMask(20, 20, 1);
  const dig = {
    kind: "dig" as const,
    fromX: 2,
    fromY: 10,
    toX: 17,
    toY: 10,
    radius: 1,
  };
  assert.equal(applyTerrainOperation(digA, dig), 50);
  assert.equal(applyTerrainOperation(digB, dig), 50);
  assert.equal(hashTerrainMask(digA), hashTerrainMask(digB));
});

test("accepted finite attacks consume exactly one charge and duplicate commands are no-ops", () => {
  const initial = makeState();
  const initialHash = hashSimulationState(initial);
  const command = {
    commandId: "shot-1",
    playerId: "p1",
    expectedTurnId: "turn-7",
    slot: "main" as const,
    weaponId: "ramshot-cannon",
    angleMilliDegrees: 45_000,
    powerBasisPoints: 8_000,
  };
  const accepted = applyWeaponCommand(initial, command);

  assert.equal(accepted.status, "accepted");
  assert.equal(accepted.ammoConsumed, 1);
  assert.equal(remainingAmmo(initial, "p1", "ramshot-cannon"), 3);
  assert.equal(remainingAmmo(accepted.state, "p1", "ramshot-cannon"), 2);
  assert.notEqual(accepted.stateHash, initialHash);
  assert.ok(accepted.trajectory);

  const duplicate = applyWeaponCommand(accepted.state, command);
  assert.equal(duplicate.status, "duplicate");
  assert.equal(duplicate.ammoConsumed, 0);
  assert.strictEqual(duplicate.state, accepted.state);
  assert.equal(duplicate.stateHash, accepted.stateHash);
  assert.equal(remainingAmmo(duplicate.state, "p1", "ramshot-cannon"), 2);
});

test("Longsword consumes no ammo, while Blood Maul deterministically damages its user", () => {
  const swordState = makeState(swordLoadout);
  const sword = applyWeaponCommand(swordState, {
    commandId: "sword-1",
    playerId: "p1",
    expectedTurnId: "turn-7",
    slot: "offHand",
    weaponId: "longsword",
    target: {
      x: swordState.players.p1!.position.x + BASE_MELEE_RANGE * 2,
      y: swordState.players.p1!.position.y,
    },
  });
  assert.equal(sword.status, "accepted");
  assert.equal(sword.ammoConsumed, 0);
  assert.equal(remainingAmmo(sword.state, "p1", "longsword"), Number.POSITIVE_INFINITY);

  const maulState = makeState(swordLoadout);
  const maul = applyWeaponCommand(maulState, {
    commandId: "maul-1",
    playerId: "p1",
    expectedTurnId: "turn-7",
    slot: "melee",
    weaponId: "blood-maul",
    target: {
      x: maulState.players.p1!.position.x + BASE_MELEE_RANGE,
      y: maulState.players.p1!.position.y,
    },
  });
  assert.equal(maul.status, "accepted");
  assert.equal(maul.selfDamage, 14);
  assert.equal(maul.state.players.p1!.health, 186);
  assert.equal(remainingAmmo(maul.state, "p1", "blood-maul"), 1);
});

test("dig attacks modify cloned terrain once and reject targets beyond melee reach", () => {
  const solid = createTerrainMask(30, 30, 1);
  const state = createSimulationState({
    terrain: solid,
    activePlayerId: "p1",
    turnId: "dig-turn",
    players: [
      {
        id: "p1",
        position: { x: 8 * POSITION_SCALE, y: 10 * POSITION_SCALE },
        loadout: finiteLoadout,
      },
    ],
  });
  const digCommand = {
    commandId: "dig-1",
    playerId: "p1",
    expectedTurnId: "dig-turn",
    slot: "melee" as const,
    weaponId: "trench-spade",
    target: { x: 12 * POSITION_SCALE, y: 10 * POSITION_SCALE },
  };
  const dug = applyWeaponCommand(state, digCommand);
  assert.equal(dug.status, "accepted");
  assert.equal(dug.terrainOperation?.kind, "dig");
  assert.ok(dug.terrainCellsRemoved > 0);
  assert.equal(hashTerrainMask(state.terrain), hashTerrainMask(solid), "input terrain is not mutated");
  assert.notEqual(hashTerrainMask(dug.state.terrain), hashTerrainMask(state.terrain));

  const duplicate = applyWeaponCommand(dug.state, digCommand);
  assert.equal(duplicate.status, "duplicate");
  assert.equal(duplicate.terrainCellsRemoved, 0);
  assert.equal(hashTerrainMask(duplicate.state.terrain), hashTerrainMask(dug.state.terrain));

  const tooFar = applyWeaponCommand(state, {
    ...digCommand,
    commandId: "dig-far",
    target: { x: state.players.p1!.position.x + BASE_MELEE_RANGE + 1, y: state.players.p1!.position.y },
  });
  assert.equal(tooFar.status, "rejected");
  assert.equal(tooFar.reason, "out-of-range");
  assert.equal(remainingAmmo(tooFar.state, "p1", "trench-spade"), 4);
});

test("independent runs of the same command log produce the same gameplay hash", () => {
  const stateA = makeState();
  const stateB = makeState();
  const command = {
    commandId: "replay-shot-1",
    playerId: "p1",
    expectedTurnId: "turn-7",
    slot: "main" as const,
    weaponId: "ramshot-cannon",
    angleMilliDegrees: 62_500,
    powerBasisPoints: 7_300,
  };
  const resultA = applyWeaponCommand(stateA, command);
  const resultB = applyWeaponCommand(stateB, command);
  assert.equal(resultA.status, "accepted");
  assert.equal(resultB.status, "accepted");
  assert.equal(resultA.stateHash, resultB.stateHash);
  assert.equal(hashTerrainMask(resultA.state.terrain), hashTerrainMask(resultB.state.terrain));
  assert.deepEqual(resultA.trajectory, resultB.trajectory);
});
