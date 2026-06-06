# `engine_no_time_in_sim`

A [dylint] lint — and the **pilot** for the zone-aware rulebook. It flags
wall-clock / monotonic time reads (`Instant::now`, `SystemTime::now`) inside a
`#[sim]` zone.

### Why

Deterministic simulation must be a pure function of its seeded inputs. Sampling
the environment clock makes a tick non-reproducible — the same inputs replay to
a different result. Time must enter the sim as an explicit input (a fixed tick /
step), never be read here.

### How the zone is detected

`crates/axiom-zones`' `#[sim]` attribute injects a greppable marker
(`const __engine_zone_sim: () = ();`) into the function or module it marks. This
lint walks the enclosing item chain (functions and inline modules) for that
marker const, so a `#[sim]` *module* zones everything inside it, not just
individually-marked functions. Test code, non-engine paths (`apps/`, `xtask`,
`axiom-zones`), and non-`src` files are exempt — same scoping as the other
engine lints.

The `ui/` fixtures hand-write the marker exactly as the macro injects it, so they
need no dependency on the markers crate. The zone-detection helpers here
(`in_zone`, `item_has_marker`, `is_engine_file`) are the ones that will be lifted
into a shared `engine_lint_helpers` crate as the rest of the zone rulebook lands.

### Running it

```sh
cargo dylint --all -- --all-targets
```

[dylint]: https://github.com/trailofbits/dylint
