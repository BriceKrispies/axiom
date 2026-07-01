# Zanzoban — Level Format

Levels are plain, hand-editable **TOML** (no bespoke format). The schema is small
and is round-tripped by `src/level_codec.rs` (`to_toml` / `from_toml`). The
canonical example is [`levels/001-button-door.toml`](levels/001-button-door.toml).

## Schema

```toml
title  = "Button Door"   # string, required — human-facing level name
width  = 10              # integer > 0 and ≤ 256, required — grid columns
height = 10              # integer > 0 and ≤ 256, required — grid rows

[player]
start = [1, 5]           # [x, y], required — the single entrance / start cell

[exit]
position = [8, 5]        # [x, y], required — the single goal cell

# Zero or more solid wall cells.
[[walls]]
position = [0, 0]        # [x, y]

# Zero or more pressure buttons.
[[buttons]]
position = [4, 5]        # [x, y]
group    = "main"        # wiring group (non-empty)

# Zero or more doors.
[[doors]]
position = [7, 5]        # [x, y]
group    = "main"        # wiring group — opened by any pressed button of this group
```

### Coordinates

`[x, y]` where `x` is the column (0 at the left) and `y` is the row (0 at the
top, increasing downward — matching the top-down view). All positions must lie
inside the grid (`0 ≤ x < width`, `0 ≤ y < height`).

### Cells are exclusive

Each cell is exactly one thing: floor (the default — omit it), wall, entrance,
exit, button, or door. Two placed objects in the same cell is a validation error.
Any cell not named by `[player]`, `[exit]`, `[[walls]]`, `[[buttons]]`, or
`[[doors]]` is floor.

### Wiring groups

A door is **open** whenever any solid actor (player or ghost) stands on a button
whose `group` matches the door's `group`. Use the same `group` string to wire a
button to a door (e.g. `"main"`). Every door's group must have at least one
button.

## Validation rules

A level is **invalid** if any of these hold (each is reported by
`level_validation::validate_level`, and shown live in the editor):

| Rule | Error |
| --- | --- |
| `width` or `height` is zero | `ZeroWidth` / `ZeroHeight` |
| `width` or `height` exceeds 256 | `WidthTooLarge` / `HeightTooLarge` |
| not exactly one entrance | `NoEntrance` / `MultipleEntrances` |
| not exactly one exit | `NoExit` / `MultipleExits` |
| a button has an empty `group` | `EmptyButtonGroup` |
| a door has an empty `group` | `EmptyDoorGroup` |
| a door's group has no matching button | `DoorWithoutButton` |
| an object is placed outside the grid | `OutsideGrid` |
| two exclusive objects share a cell | `OverlappingObjects` |
| the player start is on a wall | `PlayerStartBlocked` |

> The `[player]`/`[exit]` tables are single-valued in TOML, so a *parsed* level
> always has exactly one entrance and one exit; the "not exactly one
> entrance/exit" rules become reachable in the **editor**, whose paint grid can
> hold zero or many. The single `LevelValidationReport` covers both paths (see
> `ARCHITECTURE.md`).

## Editing in the browser

The in-app editor reads and writes exactly this format: **Export** serializes the
painted grid to TOML, and **Import** parses TOML back into the grid. The level
must validate before you can switch to playtest.
