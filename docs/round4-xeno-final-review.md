# Round-4 xenobiology FINAL ship-readiness review

Companion to:
- `docs/post-implementation-xeno-review.md` (Round 1 — **NEEDS_REWORK**, C1-C7)
- `docs/post-fix-xeno-review.md` (Round 2 — **APPROVED_WITH_CONDITIONS**, N1-N3)
- `docs/round3-xeno-review.md` (Round 3 — **APPROVED**, N5-N6 follow-ups)

Reviewer lens: xenobiology. Scope: final go/no-go for
arbitrary-planet credibility after T-wave (PRs #109-#130).

## 1. Overall ship-readiness verdict

**SHIP_WITH_KNOWN_GAPS.** The biology stack is now integrated
end-to-end across the substrate menagerie the project advertises.
Aqueous, Hydrocarbon, Ammoniacal, Silicate, super-Earth, hot
Jupiter, M-dwarf-locked, and subsurface-ocean worlds each have an
end-to-end test that builds a planet, runs the tick loop without
overflow or NaN, and produces non-zero ecosystem traffic. The
remaining gaps are calibration constants (T11 Venus plateau, T12
MAVEN absolute escape, T13 snowball recovery, T19 Ganymede flux)
— honestly flagged in code as FIXMEs with literature ranges
quoted, not silently fudged. These are tuning slips, not
structural integration gaps. Shipping this is honest.

## 2. Cross-planet biology credibility table

| Substrate / class | Credible end-to-end? | Test coverage |
|---|---|---|
| **Aqueous (Earth-like)** | Yes | seed-1024 16k-tick canary (`tests.rs:494`), Earth jet velocity ±20% (#111), relation/recognition emission (`tests.rs:269, 297`) |
| **Hydrocarbon (Titan, 94 K)** | Yes — runs, biology populates against Hydrocarbon tolerance envelope, recognition channel set differs from water (`recognition/tests.rs:201-226`) | `titan_analog_run_produces_credible_state` (`tests.rs:1076`) |
| **Ammoniacal** | Yes — substrate-specific tolerance derivation exercised; tier-0 biomass non-zero | `ammoniacal_analog_run_produces_credible_state` (`tests.rs:2083`) |
| **Silicate (lava, 2000 K)** | Yes — extreme-T window honoured (`ecosystem/tests.rs:2530`), recognition gated on depth+T (`recognition/tests.rs:233`) | `lava_world_runs_with_silicate_substrate` (`tests.rs:1846`) |
| **Super-Earth (2.22 g)** | Yes — no overflow at 2× gravity, capacity link survives | `super_earth_run_with_2g_gravity_does_not_overflow` (`tests.rs:872`) |
| **Hot Jupiter (300 M_E, 1500 K)** | Structural only — no overflow; biology suppressed by tolerance, as expected | `hot_jupiter_extreme_params_do_not_overflow` (`tests.rs:1629`) |
| **M-dwarf HZ locked** | Yes — 100× G-flare rate wired (#124), tolerance-gated catastrophe path active | `m_dwarf_hz_locked_planet_runs_cleanly` (`tests.rs:1450`) |
| **Subsurface ocean (Europa/Ganymede/Callisto)** | Yes for Europa (Round 3 closed [5,20] TW); Ganymede 6-12× under, Callisto credible | T19 in `tidal_heating.rs:1317, 1389`; Europa fit in Round 3 |

Substrate variety is real, not Earth-with-knobs. Tolerance
envelopes per substrate (`ecosystem/src/lib.rs:1887, 2039`) drive
catastrophe survival differently per world, and recognition
channels gate detection by chemistry.

## 3. Items completed since Round 3

All T-wave items landed and trace to PR + test:

- **N4 closed** by T8 #114 — `cosmic_amp` clamp is bidirectional;
  strong magnetospheres now suppress below 1×.
- **N5 closed** by T2 #112 — `apply_catastrophe_at_cell` wired to
  all five catastrophe kinds, not just volcanic. The Round-3
  one-screen fix landed.
- **N6 closed** by T7 #118 — ecosystem-role → Lifecycle mapping
  refined; Macro-parasites and consumer tiers no longer all
  collapse onto Vertebrate.
- Round-3 deferred sprint items 2-3 also done: T9 #115
  biome-class-weighted cell biomass, T1 #109 per-month magnetic
  reversal, T3 #113 gravity-threaded tide/wind, T4 #116 exobase
  T, T5 #117 cirrus greenhouse, T14 #111 Earth jet tightened.
- New event surface (#129): SpeciesExtinct, SpeciationOccurred,
  HGT, CivResilienceTick, stellar class, HZ band, rotation state
  reach viewport/digest; latent `score()`-without-`scored_line()`
  bug for SpeciesExtinct/SpeciationOccurred fixed.
- Narration streaming + replay (#130) — operator-grade
  observability for live and post-hoc runs.

Round-3's "infrastructure exists, only the call site needs
swapping" verdict on N5 is fully discharged.

## 4. Remaining actionable bugs

None that are structural. The four remaining concrete tuning
slips, all already named in code:

- `physics/src/radiation.rs:1509,1612` — T11 Venus runaway
  plateaus at ~559 K vs literature 700-770 K. Single constant
  (greenhouse coupling) miscalibrated; test pins to produced
  range with FIXME.
- `physics/src/atmospheric_escape.rs:1499,1662` — T12 Mars MAVEN
  absolute escape rate 4-5 OOM high; relative-loss anchors all
  correct. Fix is a per-channel `kg_per_s_at` scalar (named in
  FIXME).
- `physics/src/weathering.rs:593,631` — T13 snowball recovery
  test `#[ignore]`'d; `co2_greenhouse_k` 3 OOM too small to
  drive recovery within 1M ticks. Constant adjustment, not
  algorithm.
- `physics/src/tidal_heating.rs:1317` — T19 Ganymede ~0.16 TW vs
  literature ~1-2 TW (6-12× under). Callisto in range.

Each is a one-constant swap with a literature anchor in the
FIXME. None gate biology integration.

## 5. Deferred (extensions, not bugs)

- Directional adaptive radiation fill; endosymbiosis one-shot;
  body-size `mass^0.75` allometric metabolic demand; Red Queen
  co-evolution; detritus-driven decomposer cap (S7); uniform-
  irradiance producer competition (S3). All Round-2 deferrals,
  still defensible defaults.
- Polyploid speciation among non-producer plants (none exist in
  the role taxonomy — forecloses fungi-worlds).
- Per-channel absolute-escape recalibration (T12 follow-up) and
  global greenhouse recalibration (T11/T13) as one coordinated
  pass rather than four spot patches.

## 6. Calibration-gap honesty audit

**Honest engineering, not technical debt.** Four tests
(Venus-T11, Mars-T12, snowball-T13, Ganymede-T19) ship with the
calibration miss documented in-source, the literature target
quoted, and the test either pinning the produced range or marked
`#[ignore]` with the reason. This is the right pattern: the
alternative — silently re-tuning every fractional anchor across
the physics module to chase one absolute number — would break
every existing relative-loss test and hide the miss. The FIXMEs
name the fix (per-channel scalar, greenhouse coupling constant),
so the next sprint has a worklist, not an archaeology project.
Shipping a regression guard that says "we produce 559 K, real
Venus is 700-770 K, fix is constant X" beats shipping nothing or
shipping a passing test that lies. Approved as documented debt.

## 7. Approval status

**APPROVED — SHIP_WITH_KNOWN_GAPS.**

Justification: all four prior-round structural blockers (C1-C7,
N1-N3, N5-N6) are closed in production with canaries. The
substrate menagerie has end-to-end coverage across eight planet
classes. New observability (events + narration streaming +
replay) makes live debugging tractable post-ship. Remaining
gaps are isolated single-constant calibrations, each with a
literature target and named fix path in the source. Biology
layer is genuinely integrated end-to-end across substrate
variety; individual constants need tuning but that is iterative
work appropriate to a v1.x patch series, not a v1.0 blocker.

This is the cleanest the project has been across four rounds.
Ship it.
