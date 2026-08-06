/**
 * Durable schema for accounts, progression, and the economy.
 *
 * Implements the model in `docs/PROGRESSION.md` §6 and the data rules in
 * `docs/SECURITY_BASELINE.md` §7. The D1 binding is still `null` in
 * `.openai/hosting.json`; this schema defines the shape and is activated at the milestone
 * that genuinely needs persistence, per `PLATFORM_STRATEGY.md` §9.
 *
 * ## Two principles the shape enforces
 *
 * **Balances are derived, never authoritative.** `economyTransaction` is an append-only
 * ledger and is the source of truth. A stored balance is a cache that can be rebuilt by
 * summing the ledger, and a disagreement is an alertable fault where the ledger wins. A
 * mutable balance column as the source of truth makes double-grant bugs unfalsifiable
 * after the fact — you cannot tell whether a total is wrong because you have nothing to
 * compare it against.
 *
 * **Every grant is idempotent by construction.** `idempotencyKey` is UNIQUE, so a
 * duplicate match-completion message, a retried level claim, or a double-clicked purchase
 * fails at the database rather than relying on application logic to notice. This is an
 * acceptance requirement (`PRODUCT_SPEC.md` §11: "Duplicate completion messages cannot
 * grant XP or currency twice").
 *
 * ## Not stored here
 *
 * Emails and external provider subjects live in the identity tables only and never join
 * to match or replay data (`SECURITY_BASELINE.md` §7). Match payloads carry opaque player
 * ids exclusively.
 */

import { sql } from "drizzle-orm";
import {
  check,
  index,
  integer,
  primaryKey,
  sqliteTable,
  text,
  uniqueIndex,
} from "drizzle-orm/sqlite-core";

/** Epoch-millisecond timestamp column helper. */
const timestampMs = (name: string) => integer(name, { mode: "timestamp_ms" });

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/**
 * A player account.
 *
 * `id` is a server-generated opaque UUID and is the only identifier that ever appears in
 * match state, events, or replays. External identities map *to* it via
 * {@link externalIdentity}; they are never used as the primary key, so a platform change
 * cannot orphan progression.
 */
export const playerProfile = sqliteTable(
  "player_profile",
  {
    id: text("id").primaryKey(),
    displayName: text("display_name").notNull(),
    /** True while the account is an unconverted guest. Guests are first-class. */
    isGuest: integer("is_guest", { mode: "boolean" }).notNull().default(true),
    /** Cached level. Derived from {@link totalXp}; the XP curve is authoritative. */
    level: integer("level").notNull().default(0),
    /** Cached lifetime XP. Derived from the ledger. */
    totalXp: integer("total_xp").notNull().default(0),
    /** Cached spendable balance. Derived from the ledger. */
    shardBalance: integer("shard_balance").notNull().default(0),
    /** Progression rules version the cached values were computed under. */
    progressionVersion: integer("progression_version").notNull().default(1),
    createdAt: timestampMs("created_at").notNull(),
    updatedAt: timestampMs("updated_at").notNull(),
  },
  (table) => [
    // The level cap is a product rule (PROGRESSION.md §1), enforced in the schema so an
    // application bug cannot persist an out-of-range level.
    check("player_level_range", sql`${table.level} >= 0 AND ${table.level} <= 55`),
    check("player_xp_non_negative", sql`${table.totalXp} >= 0`),
    // A negative balance must be impossible by constraint, not by application logic —
    // this is the last line of defence against a spend/credit race
    // (PROGRESSION.md §6.4).
    check("player_shards_non_negative", sql`${table.shardBalance} >= 0`),
    index("player_profile_level_idx").on(table.level),
  ],
);

/**
 * A link from an external platform identity to an internal player.
 *
 * Kept in its own table so a player can hold several links, and so the sensitive subject
 * value stays out of {@link playerProfile} and therefore out of every query that joins
 * profiles to match data.
 */
export const externalIdentity = sqliteTable(
  "external_identity",
  {
    id: text("id").primaryKey(),
    playerProfileId: text("player_profile_id")
      .notNull()
      .references(() => playerProfile.id, { onDelete: "cascade" }),
    /** e.g. `"chatgpt"`, `"steam"`, `"google"`. */
    provider: text("provider").notNull(),
    /** The provider's subject identifier. Never rendered, never sent to a client. */
    providerSubject: text("provider_subject").notNull(),
    createdAt: timestampMs("created_at").notNull(),
    lastLoginAt: timestampMs("last_login_at"),
  },
  (table) => [
    // One provider subject maps to exactly one internal player. Without this, two
    // profiles could claim the same external identity and progression would fork.
    uniqueIndex("external_identity_provider_subject_idx").on(
      table.provider,
      table.providerSubject,
    ),
    index("external_identity_player_idx").on(table.playerProfileId),
  ],
);

// ---------------------------------------------------------------------------
// Economy ledger
// ---------------------------------------------------------------------------

/**
 * Append-only ledger of every XP and shard movement.
 *
 * Rows are never updated or deleted. A correction is a new compensating row, so the
 * history of what was granted and why survives intact — which is what makes an economy
 * dispute or an audit answerable.
 */
export const economyTransaction = sqliteTable(
  "economy_transaction",
  {
    id: text("id").primaryKey(),
    playerProfileId: text("player_profile_id")
      .notNull()
      .references(() => playerProfile.id, { onDelete: "cascade" }),
    /**
     * Deterministic, not random. Formats are fixed in `PROGRESSION.md` §6.2, e.g.
     * `match:{matchId}:{playerId}:{sourceType}`. Determinism is the point: a retry
     * regenerates the same key and collides with the original.
     */
    idempotencyKey: text("idempotency_key").notNull(),
    /** `"xp"` or `"shards"`. Ledgers are strictly separate and never exchange. */
    ledger: text("ledger", { enum: ["xp", "shards"] }).notNull(),
    /** Signed delta. Credits are positive, spends negative. */
    amountDelta: integer("amount_delta").notNull(),
    sourceType: text("source_type", {
      enum: [
        "match_completion",
        "match_victory",
        "daily_first_win",
        "performance_bonus",
        "level_choice",
        "purchase",
        "admin_adjustment",
      ],
    }).notNull(),
    /** Match id, level number, or shop item id, depending on `sourceType`. */
    sourceId: text("source_id"),
    /** Required for `admin_adjustment`; surfaced in the audit log. */
    actorId: text("actor_id"),
    progressionVersion: integer("progression_version").notNull(),
    createdAt: timestampMs("created_at").notNull(),
  },
  (table) => [
    // The single most important constraint in this file. Everything else in the economy
    // integrity story rests on this being a database-level guarantee.
    uniqueIndex("economy_transaction_idempotency_idx").on(table.idempotencyKey),
    index("economy_transaction_player_ledger_idx").on(table.playerProfileId, table.ledger),
    index("economy_transaction_source_idx").on(table.sourceType, table.sourceId),
    // A zero-delta row carries no information and usually indicates a computation bug
    // that would otherwise pass silently.
    check("economy_delta_non_zero", sql`${table.amountDelta} != 0`),
  ],
);

// ---------------------------------------------------------------------------
// Level-up choices
// ---------------------------------------------------------------------------

/**
 * One player's choice at one level (`PROGRESSION.md` §4).
 *
 * The composite primary key `(playerProfileId, level)` is what makes claiming idempotent:
 * a replayed claim violates the key rather than granting a second reward. Unclaimed rows
 * are the queue, so a player who levels twice in one match has two pending rows.
 */
export const levelChoice = sqliteTable(
  "level_choice",
  {
    playerProfileId: text("player_profile_id")
      .notNull()
      .references(() => playerProfile.id, { onDelete: "cascade" }),
    /** The level this choice was granted for, 1..=55. */
    level: integer("level").notNull(),
    /** Null until claimed. */
    chosenOption: text("chosen_option", {
      enum: ["cosmetic", "shards", "weapon"],
    }),
    /** The cosmetic or weapon id granted. Null for the shard option. */
    grantedItemId: text("granted_item_id"),
    grantedAt: timestampMs("granted_at").notNull(),
    claimedAt: timestampMs("claimed_at"),
  },
  (table) => [
    primaryKey({ columns: [table.playerProfileId, table.level] }),
    check("level_choice_range", sql`${table.level} >= 1 AND ${table.level} <= 55`),
    // A claimed row must record both what was chosen and when. Half-written claims are
    // how a reward gets granted twice: the second attempt sees no `claimedAt` and
    // proceeds.
    check(
      "level_choice_claim_complete",
      sql`(${table.claimedAt} IS NULL AND ${table.chosenOption} IS NULL)
          OR (${table.claimedAt} IS NOT NULL AND ${table.chosenOption} IS NOT NULL)`,
    ),
    index("level_choice_unclaimed_idx").on(table.playerProfileId, table.claimedAt),
  ],
);

// ---------------------------------------------------------------------------
// Ownership
// ---------------------------------------------------------------------------

/**
 * A weapon a player has unlocked.
 *
 * Ownership gates *casual* loadouts only. Rated modes normalize the arsenal for everyone
 * (`PROGRESSION.md` §5), so this table must never be consulted when building a ranked
 * match loadout.
 */
export const playerWeapon = sqliteTable(
  "player_weapon",
  {
    playerProfileId: text("player_profile_id")
      .notNull()
      .references(() => playerProfile.id, { onDelete: "cascade" }),
    /** Matches a `WeaponDefinition.id` in the simulation core roster. */
    weaponId: text("weapon_id").notNull(),
    acquisitionSource: text("acquisition_source", {
      enum: ["starter", "level_choice", "purchase", "admin_grant"],
    }).notNull(),
    acquiredAt: timestampMs("acquired_at").notNull(),
  },
  (table) => [
    // Owning a weapon twice is meaningless and would let a double-grant hide.
    primaryKey({ columns: [table.playerProfileId, table.weaponId] }),
  ],
);

/** A cosmetic a player owns. Cosmetics carry no gameplay values, in any mode. */
export const playerCosmetic = sqliteTable(
  "player_cosmetic",
  {
    playerProfileId: text("player_profile_id")
      .notNull()
      .references(() => playerProfile.id, { onDelete: "cascade" }),
    cosmeticId: text("cosmetic_id").notNull(),
    acquisitionSource: text("acquisition_source", {
      enum: ["starter", "level_choice", "purchase", "admin_grant"],
    }).notNull(),
    acquiredAt: timestampMs("acquired_at").notNull(),
  },
  (table) => [primaryKey({ columns: [table.playerProfileId, table.cosmeticId] })],
);

/** A player's saved three-slot loadout. */
export const loadout = sqliteTable(
  "loadout",
  {
    id: text("id").primaryKey(),
    playerProfileId: text("player_profile_id")
      .notNull()
      .references(() => playerProfile.id, { onDelete: "cascade" }),
    name: text("name").notNull(),
    mainWeaponId: text("main_weapon_id").notNull(),
    secondaryWeaponId: text("secondary_weapon_id").notNull(),
    meleeToolWeaponId: text("melee_tool_weapon_id").notNull(),
    /** Cosmetic selections. JSON, because cosmetics never need relational integrity. */
    cosmeticSlots: text("cosmetic_slots", { mode: "json" }),
    updatedAt: timestampMs("updated_at").notNull(),
  },
  (table) => [index("loadout_player_idx").on(table.playerProfileId)],
);

// ---------------------------------------------------------------------------
// Match records
// ---------------------------------------------------------------------------

/**
 * Summary of a completed match.
 *
 * Deliberately a summary, not a tick log. Replays are reconstructed from the
 * authoritative event stream (`SECURITY_BASELINE.md` §7); writing render state here would
 * bloat the durable store for no benefit.
 */
export const matchSummary = sqliteTable(
  "match_summary",
  {
    id: text("id").primaryKey(),
    mode: text("mode").notNull(),
    mapId: text("map_id").notNull(),
    /** Recorded so an old replay stays interpretable after a balance change. */
    simulationVersion: integer("simulation_version").notNull(),
    contentVersion: integer("content_version").notNull(),
    randomSeed: text("random_seed").notNull(),
    startedAt: timestampMs("started_at").notNull(),
    completedAt: timestampMs("completed_at"),
    /** Active match seconds. Gates reward eligibility (`PROGRESSION.md` §6.3). */
    durationSeconds: integer("duration_seconds"),
    /**
     * Whether the authoritative host certified this match as reward-eligible. Stored
     * rather than recomputed so a later rule change cannot retroactively make a paid-out
     * match ineligible.
     */
    rewardEligible: integer("reward_eligible", { mode: "boolean" })
      .notNull()
      .default(false),
    finalStateHash: text("final_state_hash"),
  },
  (table) => [index("match_summary_completed_idx").on(table.completedAt)],
);

/** One player's participation and outcome in a match. */
export const matchParticipant = sqliteTable(
  "match_participant",
  {
    matchId: text("match_id")
      .notNull()
      .references(() => matchSummary.id, { onDelete: "cascade" }),
    playerProfileId: text("player_profile_id")
      .notNull()
      .references(() => playerProfile.id, { onDelete: "cascade" }),
    teamIndex: integer("team_index").notNull(),
    placement: integer("placement"),
    /** Committed valid actions. Gates reward eligibility (`PROGRESSION.md` §6.3). */
    actionsCommitted: integer("actions_committed").notNull().default(0),
    damageDealt: integer("damage_dealt").notNull().default(0),
    terrainCellsRemoved: integer("terrain_cells_removed").notNull().default(0),
    /** Loadout as equipped, so a later balance patch cannot rewrite history. */
    initialLoadout: text("initial_loadout", { mode: "json" }),
    connectionStatus: text("connection_status", {
      enum: ["completed", "disconnected", "forfeited"],
    }).notNull(),
  },
  (table) => [
    primaryKey({ columns: [table.matchId, table.playerProfileId] }),
    index("match_participant_player_idx").on(table.playerProfileId),
  ],
);

// ---------------------------------------------------------------------------
// Security audit
// ---------------------------------------------------------------------------

/**
 * Security events, on a channel separate from gameplay and chat
 * (`SECURITY_BASELINE.md` §9).
 *
 * Separation is deliberate: mixing authorization denials into the gameplay event stream
 * makes them impossible to alert on without also parsing every shot fired, and it lets
 * chat volume bury an attack.
 */
export const securityEvent = sqliteTable(
  "security_event",
  {
    id: text("id").primaryKey(),
    eventType: text("event_type").notNull(),
    severity: text("severity", {
      enum: ["info", "warning", "critical"],
    }).notNull(),
    /** Nullable: pre-authentication events have no known player. */
    playerProfileId: text("player_profile_id"),
    matchId: text("match_id"),
    /** Structured context. Must never contain tokens, secrets, or email addresses. */
    detail: text("detail", { mode: "json" }),
    createdAt: timestampMs("created_at").notNull(),
  },
  (table) => [
    index("security_event_type_time_idx").on(table.eventType, table.createdAt),
    index("security_event_severity_idx").on(table.severity, table.createdAt),
  ],
);
