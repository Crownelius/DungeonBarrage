# Dungeon Barrage UI Overhaul — Retro Arcade Franchise System

Status: **implementation started**
Reference direction: [Jae-hak Park's Gunbound Season 4 UI/UX presentation](https://comde.artstation.com/projects/rNel2)
Concept board: [`design/retro-arcade-menu-concept-v1.png`](./design/retro-arcade-menu-concept-v1.png)

## Product intent

Dungeon Barrage's launch cast is the foundation of a larger video-game franchise. The interface must
introduce Leslie, Crow, Erus, and Kreena as recognizable heroes before it asks the player to parse
systems. The current utility screens prove the flow, but their flat red bars, placeholder initials,
and debug-first copy do not communicate a game identity.

The replacement language is **retro arcade dungeon fantasy**: a dark stone cabinet, ember-red focus,
electric-cyan navigation, coin-gold rewards, chunky pixel corners, restrained CRT scanlines, and
clear modern typography. It must feel playful and competitive without becoming noisy or nostalgic
at the expense of readability.

The ArtStation work is inspiration for hierarchy only: branded frames, character-forward panels,
large mode cards, persistent navigation, and decisive calls to action. Dungeon Barrage must not copy
its pastel dream treatment, logos, characters, icons, layouts, or source assets.

## Experience contract

1. **Boot/title:** establish the logo, four-character roster, and primary mode within one glance.
2. **Mode selection:** Local Duel is immediately playable; future franchise modes are visibly
   intentional but clearly locked, never fake buttons.
3. **Arena setup:** show the selected map as a stage card, identify Human vs CPU, and explain that
   damage and ring-out are both viable win routes.
4. **Character select:** replace initials with animated production sprites; compare role, health,
   movement, and all three actions without leaving the screen.
5. **Match HUD:** preserve one authoritative dotted aim guide, reserve gold for predicted hits and red
   for misses, reduce debug text, and keep Shot 1 / Shot 2-or-Melee freely switchable.
6. **Results:** turn the terminal overlay into a victory cabinet panel with rematch and return routes,
   while preserving authority-owned outcome and state provenance.
7. **Settings/accessibility:** apply the same frame system to settings, localization, contrast, text
   scale, reduced motion, audio, and performance controls.

## Reusable visual system

- `RetroArcadeUi` owns palette, backdrop, scanlines, pixel corners, framed panels, centered labels,
  status pills, buttons, and screen headers.
- Screen code supplies semantic content and interaction state; it does not invent one-off colors.
- Gold means selected, confirmed, or predicted hit. Red means danger, miss, or destructive action.
  Cyan means navigation and informational focus. Purple is reserved for special/SS content.
- Focus must remain visible at 1280×720 and with the existing high-contrast/text-scale settings.
- Motion is cosmetic and respects Reduce Motion. Gameplay authority remains entirely in Rust.

## Delivery slices

### U1 — opening flow (in progress)

- Shared retro-arcade primitives.
- Franchise title screen with Local Duel and honest future-mode locks.
- Arena setup stage card and Human-vs-CPU match card.
- Four-card character select using real sprite sheets and complete kit information.
- Preserve keyboard, mouse, smoke-test, and back-navigation contracts.

### U2 — combat readability

- Replace the debug-first HUD with player/team plates, timer, wind, action rail, and concise prompts.
- Keep one dotted authority preview; visually verify both gold-hit and red-miss states against visible
  character sprites.
- Make normal-action and charged-SS availability legible without implying finite secondary ammo.

### U3 — results and shell

- Franchise-quality victory/draw panel, rematch, return-to-roster, and return-to-title.
- Retro-arcade settings and accessibility screens wired to the existing C7 settings model.
- Boot transition, reduced-motion alternative, audio hooks, and consistent controller focus.

### U4 — production art and release QA

- Replace concept-only graphics with original production logo, portraits, cabinet decals, and icons.
- Test 16:9 scaling, contrast modes, text scale, controller-only completion, and window resize.
- Export and visually inspect C5, C6, timeout, and C7 renderer evidence. Headless success alone is
  insufficient for a UI milestone.

## Acceptance gates for every slice

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- locked .NET restore, Release build, tests, and format verification
- clean Godot import and Windows release export
- renderer-backed screenshots at 1280×720, manually inspected
- C6 terminal match, all maps, falling blocks, rematch, and disposal remain successful
- no new gameplay decisions or collision logic in C#
