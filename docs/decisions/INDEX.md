# Decisions index — historical archive

> **This is a frozen archive of historical design rationale.**
> The per-feature docs in `docs/*.md` are the source of truth for
> current behavior. The Q files are kept for tracing *why* a
> given decision was made, not for finding *what is true today*.
>
> Some Q files have drifted from current code (especially older
> ones that pre-date the 1-tick = 1-month convention or the
> cosmology / religion split). Don't treat any individual Q file
> as authoritative for current behavior without cross-checking
> against the relevant per-feature doc and the code itself.
>
> **Don't add new Q files.** Capture new design decisions inline
> in the relevant per-feature doc.

Each entry below is `- [Q## — title](q##.md)`.

---

## Latest decisions

The most recent ten, newest first. For older work, scroll to the
themed sections below.

- [Q159 — Religion / customs as the fast cultural-divergence layer](q159.md)
- [Q158 — Belligerence-driven war: pressure + kinship instead of overlap](q158.md)
- [Q157 — Civ-built experiment apparatus: controlled-conditions intervention alongside passive observation](q157.md)
- [Q156 — Per-terrain habitability multipliers + claim-eligibility gate](q156.md)
- [Q155 — Distinct successor centroid + tiny-territory auto-collapse](q155.md)
- [Q154 — Poisson-disc peak placement for distributed terrain](q154.md)
- [Q153 — Viewport polish: redundant key entries, conflict log dedup, bigger default grid](q153.md)
- [Q150 — Per-civ sidebar panels](q150.md)
- [Q149 — Side-by-side viewport layout](q149.md)
- [Q148 — Log polish: legend glyphs, cosmology axis, conflict suppression](q148.md)
- [Q152 — Deterministic civ names](q152.md)
- [Q151 — README de-Q-ification](q151.md)
- [Q147 — Multi-peak terrain for varied elevation](q147.md)
- [Q146 — Centroid letter wins disputes; revert Q144 diff render](q146.md)
- [Q145 — More log events + `run.sh` seed-arg](q145.md)
- [Q144 — Diff-based viewport rendering](q144.md)
- [Q143 — Restore frame-start screen-clear inside atomic write](q143.md)
- [Q142 — Disputed-cell render order](q142.md)
- [Q141 — Revert Q140's `run.sh` grid auto-shrink](q141.md)
- [Q140 — Atomic frame paint + auto-fit grid height](q140.md)
- [Q139 — Featureless-cell fallback glyphs](q139.md)
- [Q138 — Viewport flicker, ANSI-aware centering, height trim](q138.md)
- [Q137 — Refresh stale `--help` text after Q130's grid bump](q137.md)
- [Q136 — Decisions index restructure](q136.md)
- [Q135 — `RunMetadata` event + `labels.rs` extraction](q135.md)
- [Q134 — `narrate.py` story script](q134.md)
- [Q133 — `./run.sh` launch script](q133.md)

---

## Open questions

- [Q9 — Cultural taboo strength (M4)](q09.md)
- [Q10 — Conflict resolution math (M5)](q10.md)
- [Q24 — Cosmology drift dynamics (M4)](q24.md)
- [Q25 — Focus priority dynamics (M4)](q25.md)
- [Q26 — Hypothesis suppression strength (M4)](q26.md)
- [Q46 — Post-run report contents (M6)](q46.md)
- [Q48 — Civ founding triggers (M4)](q48.md)
- [Q49 — Civ collapse triggers (M4)](q49.md)
- [Q50 — Inter-civ knowledge transmission mechanism (M4)](q50.md)
- [Q54 — Recognition-channel measurement noise + reduced-χ² (blocks on per-channel σ)](q54.md)

---

## Decided — sim & physics core (Q1–Q113, Q147, Q154–Q159)

The bulk of the sim lives here: foundations, world sampling, physics
laws, recognition templates, species/civ models, knowledge plumbing.
Within this section entries stay in numerical order. Q147 + Q154 are
late physics-pipeline additions (multi-peak terrain), Q155 is a
late civ-lifecycle fix (successor centroid + tiny-territory collapse),
and Q156 wires per-terrain habitability into the carrying-capacity /
claim-eligibility path — all sitting in the sim-core lineage even
though their numbers land past the viewport era.

- [Q1 — Does the rewrite capture intent?](q01.md)
- [Q2 — Anything missing from the planet seed?](q02.md)
- [Q3 — Named figures cap (refined by Q47)](q03.md)
- [Q4 — Lifespan model](q04.md)
- [Q6 — Discovery formula (extended by Q32, retargeted by Q40)](q06.md)
- [Q7 — Transmission-loss rates](q07.md)
- [Q11 — Snapshot retention](q11.md)
- [Q12 — Run-end condition](q12.md)
- [Q13 — Interesting moments surfacing](q13.md)
- [Q14 — Commit revisions as we go](q14.md)
- [Q18 — Species derivation order](q18.md)
- [Q19 — Doc structure](q19.md)
- [Q20 — Milestone structure (rewritten by Q39+Q40, expanded by Q47)](q20.md)
- [Q21 — Cohort-level baseline observations](q21.md)
- [Q22 — Population scaling with resources and tech (refined by Q47)](q22.md)
- [Q23 — Cultural influence on discovery (hooks)](q23.md)
- [Q27 — Decisions log](q27.md)
- [Q28 — Loss-formula coefficient tuning (placeholder; M3-tunable umbrella)](q28.md)
- [Q29 — Modality and manipulation taxonomy](q29.md)
- [Q30 — Split reach and persistence into two ladders](q30.md)
- [Q31 — Event ordering within a tick](q31.md)
- [Q32 — Symbolic vs quantitative discovery (retargeted by Q40)](q32.md)
- [Q33 — Functional form vocabulary (12 forms, tiered architecture; unlock table in Q53)](q33.md)
- [Q34 — Fit metric and tolerance formula (RMSE + exp confidence; reduced-χ² migration in Q54)](q34.md)
- [Q35 — Refinement triggers and rules (Occam-adjusted candidate selection, probation lifecycle)](q35.md)
- [Q36 — Determinism strategy for arithmetic (fixed-point, Q32.32 default)](q36.md)
- [Q39 — Drop UI (LLM also dropped, see Q51)](q39.md)
- [Q40 — Pivot to physics engine (Path D)](q40.md)
- [Q41 — Physics scope (5 families, M1 split into M1a + M1b)](q41.md)
- [Q42 — Spatial grid resolution (hex, ~2500 cells default)](q42.md)
- [Q43 — Time-stepping strategy (C1: operator splitting from M1a)](q43.md)
- [Q44 — Seeded law variation (C2-phased; revisit calibration M2/M3)](q44.md)
- [Q45 — Pattern recognition mechanism (template-driven signatures)](q45.md)
- [Q47 — Species as the persistent unit; civs rise and fall within](q47.md)
- [Q51 — Drop LLM from the project entirely](q51.md)
- [Q52 — Use real SI units and physical constants throughout](q52.md)
- [Q53 — Functional-form availability (derived from perceivable-template tags; no authored table)](q53.md)
- [Q55 — Sensorium-extending tech (tools grant channels; physically-grounded prereqs)](q55.md)
- [Q56 — Sensorium-tool unlock trigger (hybrid observation + literacy gate; replaces Q55 wall-clock)](q56.md)
- [Q57 — Settlement / per-region population (per-cell cohorts on a hex grid)](q57.md)
- [Q58 — Literacy mechanic (discovery-rate proxy with persistence-tier + lifespan modifiers)](q58.md)
- [Q59 — Figure charisma + curiosity (per-figure scalars; Q35 + Q50 wiring)](q59.md)
- [Q60 — Catastrophe events (volcanic + disease + asteroid + solar flare + ice age)](q60.md)
- [Q61 — Buildings as a sim primitive (settlement-tier persistence multiplier on Q50 transmission)](q61.md)
- [Q62 — Spatial-over-time rendering (post-run keyframes + live ASCII viewport, shared `render_world_frame`)](q62.md)
- [Q63 — Heterogeneous per-cell population dynamics (region cohorts evolve independently with cell-local seasonal capacity)](q63.md)
- [Q64 — Localized catastrophes (disease + asteroid target specific cells; volcanic already cell-targeted)](q64.md)
- [Q65 — Gradient-driven inter-cell migration (high-pressure cells shed pop to adjacent cells with headroom)](q65.md)
- [Q66 — Multi-tick wars with marching front (cells flip individually based on per-cell loser cohort vs defeat floor)](q66.md)
- [Q67 — Tick = month + seasonal physics from axial tilt (planet-derived seasonal capacity)](q67.md)
- [Q68 — Species + planet derive demographic constants (pop rates, Q50 decay, stress factor)](q68.md)
- [Q69 — Catastrophe severity / cadence from planet substrate (disease from biosphere, volcanic from crust, ice age from mean_temperature)](q69.md)
- [Q70 — Cognition-derived discovery cadence (high-cognition species cycle hypothesis attempts faster)](q70.md)
- [Q71 — Planet-archetype-specific recognition templates (10 templates + per-cell crust/atmosphere/magnetosphere/composition imprinting)](q71.md)
- [Q72 — Seasonal recognition templates (4 month-keyed + Signature::MonthIn + Hemisphere primitives)](q72.md)
- [Q73 — Measurement relations: continuous-y law recovery (alongside firing relations; recovers SI coefficients)](q73.md)
- [Q74 — Relativise recognition thresholds to planet climate (productive band derived from mean +/- gradient, not Earth-K)](q74.md)
- [Q75 — Temporal-derivative measurements (recover diffusion coefficients alpha / k from per-tick deltas)](q75.md)
- [Q76 — Compositional relations: confirmed laws as basis terms (residual auto-generation when measurements confirm; theory hierarchy)](q76.md)
- [Q77 — Substrate-derived demographics (founding pop, carrying capacity, migration, birth from biosphere x cognition x gravity)](q77.md)
- [Q78 — Inherited-knowledge re-validation (successors re-fit inherited relations after a 50-tick window; pass = `RelationRevalidated`, fail = `RelationLapsed`)](q78.md)
- [Q79 — Predict + falsify (confirmed relations track prediction drift; force-trigger refinement on sustained mispredictions)](q79.md)
- [Q80 — Calendar from planet orbit (per-planet `orbital_period_months` drives seasonal-template modulo)](q80.md)
- [Q81 — Parametric planet sampler (within-substrate correlation polish: temperature_gradient covaries with axial_tilt; full enum replacement deferred — Q82 took most of its air)](q81.md)
- [Q82 — MetabolicSubstrate axis: every seed produces life of some chemistry (Aqueous / Ammoniacal / Hydrocarbon / Silicate substrate-first sampler)](q82.md)
- [Q83 — Substrate-relative chemistry physics (per-substrate freeze/boil thresholds in `Chemistry`)](q83.md)
- [Q84 — Substrate-specific recognition templates (silicate_resonance, methane_seep, ammoniacal_storm)](q84.md)
- [Q85 — Substrate-specific Clausius-Clapeyron + per-substrate latent heats (every substrate gets pressure-varying boil + correct phase-change energy)](q85.md)
- [Q86 — Tidally-locked planets + terminator template (Planet::is_tidally_locked + Signature::TidallyLockedTerminator + template id 32)](q86.md)
- [Q87 — More substrate-native recognition templates (cryo_lake / crystal_growth / aurora_polar; ids 33-35)](q87.md)
- [Q88 — Multi-level residual chains (depth-capped auto-generation; civs build 3-level theory hierarchies)](q88.md)
- [Q89 — Report polish: per-civ scientific-lifecycle counters (revalidated / lapsed / falsified surfaced in chapter)](q89.md)
- [Q90 — Stellar-driven radiative balance (per-row Stefan-Boltzmann T_eq + per-tick relaxation; replaces "diffuse the seeded gradient forever")](q90.md)
- [Q91 — Atmospheric advection: pressure-gradient winds carrying heat (`Wind` law writes pressure + velocity fields, pair-flux upwind temperature transport)](q91.md)
- [Q92 — Hydrologic cycle: evaporation → vapour transport → precipitation (`Hydrology` law: surface-water + Vapour + wind-advection unified)](q92.md)
- [Q93 — Lunar gravitational tides (`Tides` law + `state.macro_step()` clock; sub-lunar / antipodal bulges sweep `water_depth` via pair-flux conservation)](q93.md)
- [Q94 — Planetary magnetic field as a vector (`Magnetism` law + `(B_q, B_r)` per-cell components; latitude-dependent dipole + diurnal modulation)](q94.md)
- [Q95 — `magnetic_field_strong` reads real Q94 vector magnitude (`Field::MagneticMagnitude` + signature swap; replaces the pre-Q94 `Field::Charge` proxy)](q95.md)
- [Q96 — Per-cell pressure-aware boil in Hydrology (barometric formula `P(h) = P_0 · exp(-h/H)` + substrate Clausius-Clapeyron; mountain cells boil at lower temperatures)](q96.md)
- [Q97 — Seasonal insolation swing in Radiation (`t_eq_per_row_per_season` 2D table + sub-solar-latitude shift via `axial_tilt × month-of-year` clocked off `state.macro_step()`)](q97.md)
- [Q98 — Lorentz coupling: wind × magnetic field deflects velocity (`Lorentz` law + per-cell `q · v × B` kick; closes the M1b coupling between charge, velocity, magnetic field)](q98.md)
- [Q99 — Charge advection along the wind velocity field (pair-flux upwind transport; closes the M1b coupling triangle by making charge actually ride the wind)](q99.md)
- [Q100 — Latent heat coupling in Hydrology (evaporation cools source, condensation warms receiver; closes the energy budget on Q92's cycle)](q100.md)
- [Q101 — Q32.32 sin/cos via Taylor series (replaces triangular profiles in Q90 cos_lat, Q93 tidal potential, Q94 dipole lat_factor, Q97 seasonal swing)](q101.md)
- [Q102 — Recognition templates for Q90-Q101 physics (`tropical_moist`, `dry_zone`, `storm_cell`, `windy_strait` + `Field::WindMagnitude`; surfaces the new physics to civs)](q102.md)
- [Q103 — Vertical magnetic-field component `B_z` (true 3D Lorentz; pole-to-equator field-magnitude ratio matches real dipoles; Q98 reads B_z directly)](q103.md)
- [Q104 — Per-atmosphere numeric scale height + density (`Atmosphere::scale_height_m`, `::density_x100`; Hydrology threads scale-height per-planet)](q104.md)
- [Q105 — Multi-moon mass/period + tidal superposition (`Moon` struct + `Planet::moons` + per-moon `cos(2θ_m)` sum in Tides; spring/neap-style interference)](q105.md)
- [Q106 — Coriolis deflection from planetary rotation (`Coriolis` law; mirror-symmetric N/S deflection of wind via `Ω_z = sin(lat) · k · 24/day_length`)](q106.md)
- [Q107 — Eccentric orbits: insolation swing across the year (`Planet::orbital_eccentricity_x100` + per-season `1/(1-e·cos)²` modulation in Radiation; asymmetric seasons emerge)](q107.md)
- [Q108 — Diurnal cycling: day/night insolation modulation (`Radiation` reads day_length_hours; tidally-locked planets get permanent day/night asymmetry, Earth-like averages out)](q108.md)
- [Q109 — Vertical atmosphere stack (1.5D MVP) (`upper_temperature` field + `VerticalConvection` law; per-cell lapse rate emerges from convective exchange + radiative cooling)](q109.md)
- [Q110 — Boris pusher: implicit-symplectic rotation for Lorentz / Coriolis (replace explicit Euler with `cos(θ)/sin(θ)` exact rotation; conserves `|v|` regardless of step size)](q110.md)
- [Q111 — Substrate-neutral solvent-cycle template names (rename Q102 `tropical_moist`/`dry_zone`/`storm_cell` to `solvent_humid_band`/`desiccated_band`/`condensation_storm`)](q111.md)
- [Q112 — Per-substrate cell thermal mass + specific heat (`SubstrateProperties.c_p` + `cell_thermal_mass_kg`; lifts the water-c_p hardcode in latent-heat plumbing)](q112.md)
- [Q113 — Vacuum guards on Wind / Hydrology / Coriolis (`has_atmosphere` short-circuit on `Atmosphere::None` worlds; airless planets stop running fluid dynamics)](q113.md)
- [Q147 — Multi-peak terrain for varied elevation (`init_planet` now sums 3–5 piecewise cones — primary anchored at `terrain_centre_q/r`, secondaries scattered via salted RNG — with `steep` summit slope + `shallow` ≤ 50 m/cell coastal slope; replaces the pre-Q147 single conical pyramid and the >100 m/cell drop that erased the renderer's `~` shallow-water band)](q147.md)
- [Q154 — Poisson-disc peak placement for distributed terrain (Q147 secondaries drew `(q, r)` once each from the salted RNG and clustered visibly on most seeds; Q154 wraps the secondary placement in a min-distance rejection-sampling loop — `min_dist = max(3, max(w, h) / (num_peaks × 2))` cells via `|dq| + |dr|`, up to 200 attempts per peak with a deterministic fall-back to the last attempted candidate; primary anchor + 80/20 height split + Q147 RNG salt all preserved)](q154.md)
- [Q155 — Distinct successor centroid + tiny-territory auto-collapse (`sim_civ::pick_successor_centroid` shifts a successor's `territory_centroid` off any collision with the parent's — preferring an adjacent hex neighbour, else any other claimed cell, else the figure-derived fallback — at both refound sites in `sim/core`, and drops the parent's centroid from the successor's claim so the viewport's smallest-claimed-cell heuristic resolves to distinct centroid letters; new `CollapseReason::TerritoryTooSmall` fires when `claimed_cells.len() == 1` for ≥ 24 ticks via a `tiny_territory_streak` counter on `Civ`, closes Q49's "auto-collapse on territory loss" sub-question)](q155.md)
- [Q156 — Per-terrain habitability multipliers + claim-eligibility gate (`sim_world::habitability_multiplier(glyph)` table — `≈`/`≡` 0.00, `~` 0.05, `░` 1.20, `·` 1.00, `▒` 0.90, `△` 0.60, `▲` 0.10 — read by `Civ::carrying_capacity_with_terrain` / `seasonal_carrying_capacity` / `cell_capacity` so deep ocean and gas band contribute zero capacity, coast gets a 1.20× boost, peaks contribute ~10%; `compute_territory` BFS in `sim/core` skips cells below the 0.05 claim threshold so deep ocean / gas band act as walls; new `pick_habitable_cell` helper relocates the figure-derived founding centroid off uninhabitable terrain at all three founding sites — inaugural civ, Q47 stateless-refound, Q48-v3 breakaway; closes the user's "shouldn't terrain type have implications for habitable for species?" question)](q156.md)
- [Q157 — Civ-built experiment apparatus: controlled-conditions intervention alongside passive observation (new `ToolKind::ExperimentApparatus` tier-2 tool gates on `MANIPULATION_PREREQ = ToolExtension` + obs threshold 30k + literacy 0.30 + confirmed `fire`; new `sim_civ::apparatus` module with `Apparatus { cell, clamp_channel, measure_channel }` records — one apparatus per civ at unlock, allocator picks lowest-population claimed cell. Pre-physics `write_apparatus_clamps` overwrites the apparatus cell's clamp channel with one of four ladder values keyed by `tick % 4`; post-physics `record_apparatus_samples` reads the cell's `measure_channel` and feeds the (clamp, response) pair into the first active figure's hypothesizer twice — `Hypothesizer::record_experimental_measurement` lazily inserts the `MeasurementCandidate` if the apparatus pair isn't in the Q73 catalogue and bumps `experimental_count_by_relation` so the eventual `MeasurementConfirmed` event carries `is_experimental: true`. Clamp ladders sized to keep heat / fluid / charge perturbations inside the linear-response regime so apparatus presence doesn't violate the planet's energy budget over long runs. Adds `is_experimental: bool` to `protocol::MeasurementConfirmed` (`#[serde(default)]` for back-compat). Closes the gap-#4 "civs can only observe, never intervene" item: a tactile-only ToolExtension-bearing species now derives its planet's heat-diffusion `α` from a clean clamped-T-then-relax experiment, while no-tool species stay observation-only forever — sustaining the project's "different sciences" goal.)](q157.md)
- [Q158 — Belligerence-driven war: pressure + kinship instead of overlap (replaces "any cell overlap → continuous war" with a per-pair `belligerence = drive × (1 − KINSHIP_DAMPENER · kinship)` score gating the existing 75-tick `conflict::resolve()` call. `drive = 0.45·pressure + 0.25·opportunity + 0.30·dominance`; `kinship` averages closeness across the hierarchical axis, the four non-hierarchical Cosmology axes, and literacy. War is now a stateful relationship bracketed by new `WarDeclared` / `PeaceConcluded` protocol events; hysteresis (declare 0.35 / end 0.20) prevents flapping. Contact is a hard prerequisite — `emitted_contacts` must contain the pair before war can fire. Tuned `KINSHIP_DAMPENER = 0.20` from the initial 0.6 because single-species runs have kinship near 1.0 throughout (every civ inherits Q172's `species.initial_cosmology` bias). Closes the user's "shouldn't differences and resource control drive war?" question.)](q158.md)
- [Q159 — Religion / customs as the fast cultural-divergence layer (adds a new three-axis `Religion` vector — theology / ritual / sacred_time — to every civ, on top of the existing five-axis `Cosmology`. Cosmology stays as the slow-drift species-anchored deep-worldview layer; its `push_for_*` magnitudes were halved and emit threshold raised 0.20 → 0.50. Religion is the fast-divergent layer founding figures + civ_id-keyed jitter pick the founding vector, every Q24 hypothesis hook now applies a religion push at 3× the reduced cosmology magnitude alongside the cosmology one. Q158 kinship calc weighted toward religion as dominant signal (0.60 weight vs. 0.15 cosmology) so intra-species civs can disperse in kinship without religion drifting indefinitely fast. New `ReligionShifted` protocol event at 0.20 L2 emit threshold. Closes the user's "what we have plus the new ones?" — what the sim was calling cosmology was really cultural-religious disposition; this splits the deep-cosmology layer from the fast-divergent religion / customs layer.)](q159.md)

---

## Decided — viewport & UX (Q114–Q132, Q148–Q150, Q153)

Live ASCII viewport evolution: from "spatial-timeline keyframes
in the post-run report" through every iteration of the live `--cli
viewport` mode. These compound — each Q assumes the previous ones
shipped.

- [Q114 — Viewport planet card + scrolling event log](q114.md)
- [Q115 — Compact viewport for mobile portrait](q115.md)
- [Q116 — Configurable grid size + compact viewport rendering](q116.md)
- [Q117 — Sparse two-row column header in compact viewport](q117.md)
- [Q118 — Viewport temperature unit (`TempUnit` + `--temperature-unit f|c|k`)](q118.md)
- [Q119 — Planet name + survivability badge + "no mag"](q119.md)
- [Q120 — Species line in the viewport](q120.md)
- [Q121 — Host-species wellbeing badge (substrate-relative)](q121.md)
- [Q122 — Phone-fit viewport defaults (compact + 24×16 by default)](q122.md)
- [Q123 — Cognition topology on species line + map legend](q123.md)
- [Q124 — Box-section viewport layout (`--` dividers; legend above log)](q124.md)
- [Q125 — Full-width centered viewport layout (40-col)](q125.md)
- [Q126 — Centered caption, expanded labels, biology line](q126.md)
- [Q127 — Themed card layout, descriptive labels, biochem](q127.md)
- [Q128 — Species section, friendly badges, axis separators](q128.md)
- [Q129 — Caption-into-planet, key-above-species, drop tens header](q129.md)
- [Q130 — Map-as-focus: full box, light dividers, terrain color](q130.md)
- [Q131 — Revert Q130's light dividers (heavy dividers everywhere)](q131.md)
- [Q132 — Drop vestigial `--viewport-compact` flag](q132.md)
- [Q148 — Log polish: legend glyphs, cosmology axis, conflict suppression (legend covers Q139's `·` and `≡`; cosmology shift names the dominant axis with signed magnitude via per-civ snapshot of prior `axes_q32`; `ConflictResolved` log line suppressed unless `loser_defeated=true`, so Q66 multi-tick wars surface once per outcome instead of once per skirmish)](q148.md)
- [Q149 — Side-by-side viewport layout (`VIEWPORT_WIDTH` 40 → 70; map zone left, vertical `|` rule at col 37, sidebar right carrying legend + species + per-civ panels; `+` corners on the map+key and log dividers close the rule cleanly; planet section + log section span full width; Q146 atomic clear-and-paint preserved)](q149.md)
- [Q150 — Per-civ sidebar panels (each currently-active civ gets a 3-line block: `─── {Civ name} ───` header from Q152, identity line `{letter}=cap · {digit}=civ`, stats line `y{founded_year} · {N} cells`; `civ_names` + `civ_founded_year` maps populated on `CivFounded` and cleared on `CivCollapsed`; `frame::centroid_symbol` and `frame::claim_symbol` bumped to `pub(crate)`)](q150.md)
- [Q153 — Viewport polish: redundant key entries, conflict log dedup, bigger default grid (drop `A=cap · 1=civ` from the global key now that every per-civ panel surfaces them; `wars_logged: BTreeSet<(u32, u32)>` dedups Q66's per-cell `ConflictResolved(loser_defeated=true)` flood to one line per pair, cleared on `CivCollapsed` so re-emerged civ_ids re-trigger; default grid 32×20 → 36×30 (`MAP_WIDTH` 36 → 40, `VIEWPORT_WIDTH` 70 → 74); `log_message` refactored from associated `fn` to `&mut self` method to mutate the dedup set)](q153.md)

---

## Decided — tooling & narrator (Q133–)

Out-of-process tools that consume the NDJSON event log: launch
scripts, post-run narrators, schema infrastructure that other
consumers (future LLM, dashboards) can build on.

- [Q133 — `./run.sh` launch script](q133.md)
- [Q134 — `narrate.py` story script](q134.md)
- [Q135 — `RunMetadata` event + `labels.rs` extraction (de-dup viewport ↔ narrator label tables + substrate ranges via NDJSON)](q135.md)
- [Q136 — Decisions index restructure (Latest / Open / sim-core / viewport / tooling / Superseded sections; archive collapsible)](q136.md)
- [Q137 — Refresh stale `--help` text after Q130's grid bump (defaults 24×16 → 32×20)](q137.md)
- [Q138 — Viewport flicker, ANSI-aware centering, height trim (drop full-screen clear before each frame; ANSI-aware visible-width fixes row-0 misalignment when civ markers carry color escapes; trim 3 rows from default viewport so phone-with-keyboard fits closer)](q138.md)
- [Q139 — Featureless-cell fallback glyphs (`≡` for gas-giant cloud bands, `·` for low-relief / sub-surface-ocean / oceanic-basin floors; replaces the bare blank-space fallback so an "empty" map shows texture instead of literal nothing)](q139.md)
- [Q140 — Atomic frame paint + auto-fit grid height (Vec<u8> buffer means each frame goes to stdout in one write+flush instead of ~42 line-by-line flushes — kills incremental-paint flicker; `run.sh` reads `tput lines` and shrinks `--grid-height` so the planet section doesn't scroll off on phone-keyboard-up terminals)](q140.md)
- [Q141 — Revert Q140's `run.sh` grid auto-shrink (user prefers the full 32×20 grid even when it means the planet section scrolls off the top; Q140's atomic frame paint stays, only the auto-shrink reverts)](q141.md)
- [Q142 — Disputed-cell render order (run the multi-owner check before the centroid letter so overlapping centroids surface as `#` instead of one civ's letter silently masking the other; fixes Lumen-h's missing `A` capital under Q47 succession)](q142.md)
- [Q143 — Restore frame-start screen-clear inside atomic write (`\x1b[H\x1b[2J` packed into Q140's atomic Vec<u8> buffer; pre-Q143 home-only redraw left top rows from prior frame visible on Termius — manifesting as missing planet name + duplicate column header)](q143.md)
- [Q144 — Diff-based viewport rendering (build frame, split into per-row strings, compare to previous frame, emit ANSI CUP + content + erase-line only for changed rows; unchanged rows are never touched so flicker becomes structurally impossible — same model vim/htop/btop use, no TUI deps)](q144.md)
- [Q145 — More log events + `run.sh` seed-arg (`CivContact` and `CosmologyShifted` now surface in the live log; `./run.sh 12345` replays seed 12345 while `./run.sh` keeps the random-seed default)](q145.md)
- [Q146 — Centroid letter wins disputes; revert Q144 diff render (older civ's `A` shows at contested capital instead of `#` via `entry().or_insert()` ordering; revert Q144's diff render back to Q143's atomic clear-and-paint after stale rows kept appearing on Termius)](q146.md)
- [Q151 — README de-Q-ification (scrub all `Q##` references from the user-facing `README.md`; replace with descriptive prose so first-time readers don't need to know the decision-doc system; `docs/decisions/INDEX.md` stays as the contributor-facing history archive)](q151.md)
- [Q152 — Deterministic civ names (`civ_name_from_seed(seed, civ_id)` in `sim/civ`; 64 kingdom-feeling stems × 6 endings → 384 names keyed by `seed XOR civ_id XOR magic` distinct from the Q119 / Q120 magics; `CivFounded::name` field added with `#[serde(default)]` for back-compat; `narrate.py` falls back to `civ {id}` when missing)](q152.md)

---

## Superseded

Kept for history; entries explain what replaced them.

<details>
<summary>Click to expand</summary>

- [Q5 — Phenomenon catalog size (superseded by Q40)](q05.md)
- [Q8 — Phenomenon catalog curation (superseded by Q40)](q08.md)
- [Q15 — `data/phenomena/` location (superseded by Q40)](q15.md)
- [Q16 — Catalog generation approach (superseded by Q40)](q16.md)
- [Q17 — Master catalog with per-planet filtering (superseded by Q40)](q17.md)

</details>
