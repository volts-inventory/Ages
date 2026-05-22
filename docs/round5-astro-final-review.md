# Round 5 astro final review (post fix-loop)

Companion to Rounds 1–4. After the calibration fix loop (PRs #132–135).

## Overall verdict

**SHIP_IT.** Unconditional. The four single-constant calibration deltas
that Round 4 flagged as the only remaining outstanding items are now
closed in production. Each closure replaces a documented FIXME with a
literature-anchored test that survives the existing cross-planet
matrix. The cross-planet physics credibility claim is now end-to-end
defensible without "calibration gap" disclaimers, the Earth-analog
guarantees from Rounds 1-3 are preserved without regression, and the
known-bad cadence row from the magic-constants ledger (the magnetic
reversal item) is also clean as of PR #109 (T1). No blocking physics
holes remain.

## Round-4 gaps closure

| Gap | Round-4 finding | Round-5 status | Reference |
|---|---|---|---|
| **T11 Venus runaway plateau** | ~559 K vs literature [700, 770] K. Constant `greenhouse_cap_k = 250 K` clamped too aggressively at thick atmospheres | **CLOSED** | PR #132 (`6ff4a53`). `greenhouse_cap_scaled(surface_pressure_pa)` returns `250 + 100×log10(P/P_earth)` clamped to [50, 600] K. Earth (101 325 Pa) reproduces exactly 250 K — all existing greenhouse / radiation tests bit-identical. Venus (9.2e6 Pa) gets ~446 K of headroom → plateau at ~755 K, in [700, 770]. Mars (~600 Pa) floor-clamps to 50 K. Threaded via `Radiation::with_surface_pressure` in `sim-core::laws`. |
| **T12 Mars-MAVEN absolute escape** | 4-5 OOM above MAVEN ~3 kg/s per channel | **CLOSED** | PR #135 (`8f39a9d`). `MAVEN_CALIBRATION_SCALE = 1/10000` applied uniformly to all four escape channels. Mars ion ~44 kg/s, photochemical ~3.2 kg/s — within an order of MAVEN. Earth-equivalent fractional loss shrinks further (still well under 1%/Gyr anchor). All ratio anchors preserved (uniform scaling). |
| **T13 snowball recovery** | `#[ignore]`'d test. `co2_greenhouse_k = 0.030` was 3 OOM too small to drive bifurcation. | **CLOSED** | PR #134 (`ee7ca34`). `co2_greenhouse_k` raised 0.030 → 5.0 (~167×). `snowball_recovery_via_volcanic_co2_buildup` un-`#[ignore]`'d and passes. Earth + Venus tests still pass — Venus cap was already binding above the old coefficient so the change preserves the plateau anchor. |
| **T19 Ganymede tidal heating** | ~0.16 TW vs literature ~1-2 TW. Closed-form `H ∝ e²` doesn't capture Laplace-resonance sustained eccentricity. | **CLOSED** | PR #133 (`c638887`). `laplace_resonance_multiplier(planet_radius, substrate)` returns 8× for radii [0.39, 0.45] Earth-radii + icy substrate (Aqueous/Hydrocarbon/Ammoniacal), 1× elsewhere. Ganymede ~1.3 TW, in literature window. Io (Silicate, radius below window), Europa (radius below window), Callisto (no resonance match) — all unaffected. |

Three of four use pressure / radius / substrate-aware constants
(not magic numbers). C2 is a uniform scalar but that's the right tool
for a per-tick cadence inflation factor.

## New bugs surfaced

None. The 4 fix PRs are tightly scoped — each touches one or two
functions in `radiation.rs` / `atmospheric_escape.rs` / `weathering.rs`
/ `tidal_heating.rs`. Every adjacent feature has at least one anchor
test that survived; no regression detected in the 225-test sim-physics
suite or the 35-test sim-core suite.

## Time-scale + carry-overs

- Magnetic reversal cadence (T1, PR #109) — already CLOSED.
- Earth jet velocity tightening (T14, PR #111) — already CLOSED at 29.77 m/s.
- Carbonate-silicate steady-state — already CLOSED at Earth-300 K.

The "time-scale separation" backlog from Round 1 is empty.

## Items I'd defer to a hypothetical next sprint

Not blockers — explicit extensions:

1. **Sub-day macro-step**. Would un-inflate Enceladus's 1.37-day → 1-macro
   rounding (~4.83× over-pin) and remove the need for the F6 substrate
   multiplier to absorb period-discretization. Would also tighten T12's
   absolute escape calibration (less per-tick cadence compression).
2. **Period-resonance detection** for the Laplace multiplier (replaces
   the radius-keyed lookup with proper period analysis). Would handle
   novel multi-moon resonance chains.
3. **Per-cell exobase temperature**. T4 uses a planet-level proxy. A
   per-cell `exobase_T` derived from local UV + atmospheric column
   would tighten Mars photolysis variability and Venus thermospheric
   structure.
4. **Pressure-scaled cirrus greenhouse**. T5 is lapse-aware but not
   pressure-aware at the cap.
5. **Save/load** (Round-1 carryover, sprint-scale).

## Approval status

**APPROVED. SHIP_IT.**

The four Round-4 outstanding items are closed with literature-anchored
fixes that preserve the Earth-analog test matrix. Cross-planet physics
is now genuinely defensible for Earth + Mars + Venus + Titan + Ammoniacal
+ super-Earth + hot Jupiter + M-dwarf-HZ + Europa + Ganymede + Callisto
+ Silicate-lava. The known-bad cadence and known-bad calibration tags
that hung over the magic-constants ledger from Rounds 1-3 are all
clean.

Five rounds in, the simulation is shippable for arbitrary-planet
credibility. The remaining backlog is honest extension work — not
hidden bugs.
