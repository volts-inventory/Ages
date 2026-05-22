# Magic-Constant Ladder

This document enumerates every fitted / heuristic / dimensional
constant carried by the simulation. It exists to satisfy condition 5
of the post-fix astrophysics review (`docs/post-fix-astro-review.md`):
every numerical coefficient that drives the simulation must carry a
visible note about its **origin** (where the number came from) and its
**cross-planet extrapolation status** (whether the constant has been
validated against non-Earth substrates, or whether it is known to
break outside the Earth/Solar-System anchor).

The list is descriptive, not prescriptive — we have not changed any
value to write this doc. Where a constant is known to mis-extrapolate
(e.g. `tidal_dimensional_calibration` is ~25× shy on Europa) we note
that as a `known-bad` cross-planet status and surface it in the TODO
list at the bottom.

## Legend

**Origin** — how the number was chosen:

- `Earth-fitted` — tuned so an Earth-analog planet hits a published
  observation (`bolometric_scale_at_age` lands the modern Sun at
  ~1.0×, Earth jet velocity ∈ [10, 60] m/s, etc.).
- `Solar-system-fitted` — tuned against a non-Earth body in the solar
  system (Io anchors `tidal_dimensional_calibration`; Mars anchors the
  H/He fractionation ratio).
- `dimensional` — derived from a first-principles SI formula, possibly
  with a calibrated correction factor; the source comment gives the
  exact derivation chain.
- `literature` — published value with a direct physical interpretation
  (love number, Q factor, Lindeman 10:1 trophic efficiency).
- `empirical-best-fit` — a "what makes the simulation produce a
  realistic-looking signal" choice, with no anchor against a specific
  measurement.
- `per-substrate` — table-driven: one value per
  `MetabolicSubstrate` / `Habitat` / `CrustType`.
- `arithmetic` — pure unit conversion / numerical safety floor with no
  physical content (Q32.32 underflow clamps, CFL safety factor).

**Cross-planet status** — has the constant been pinned against a
non-Earth body in a test?

- `validated` — a test enforces the constant gives the right answer
  on at least one non-Earth target.
- `partial` — Earth anchor holds, one non-Earth target validates but
  another is known to deviate.
- `known-bad` — the constant is known to mis-extrapolate (e.g. Europa
  tidal-heating gap).
- `unvalidated` — only Earth or only the planet the constant was
  fitted to has been pinned; behaviour on other substrates is not
  asserted by any test.

---

## Table — every magic constant in the codebase

### `sim/physics/` — laws of motion, thermo, chemistry

#### Tidal heating (`tidal_heating.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `tidal_heating.rs:284-290` | `tidal_dimensional_calibration` | `1.75e8` | `Solar-system-fitted` (Io) on top of a `dimensional` ~3.27e7 base | **known-bad** | Empirical ~5.4× multiplier on top of the SI dimensional value. Lands Io at ~54 TW (in `[50, 200]` window) and Enceladus at ~10.7 GW (in `[1, 100]` window). Europa is `~25×` shy of literature `~10 TW` because 1-macro = 1-day cadence rounds 3.55-day period to 4 (`(4/3.55)⁵ ≈ 1.65×`) and the Io-anchored 5.4× multiplier absorbs melt-enhanced `k₂/Q` not present in icy moons. Sub-day macro or per-substrate scaling would close the gap. |
| `tidal_heating.rs:132-136` | `conduction_k` | `0.001 / tick` | `empirical-best-fit` | `unvalidated` | Surface ↔ subsurface conduction. Gives a 0.02 K/tick surface bump at 20 K gradient, so multi-tick warm-up — matches Europa-class lag of surface behind subsurface. Validated only against the qualitative "subsurface reservoir is warmer than surface" pattern. |
| `tidal_heating.rs:159-167` | `subsurface_heat_fraction` | Aqueous 0.90, Hydrocarbon 0.90, Ammoniacal 0.60, Silicate 0.30 | `per-substrate` from literature (Io ~95% surface; Europa ~95% subsurface) | `partial` | Aqueous/Hydrocarbon pinned by `subsurface_heat_fraction_per_substrate` test (`tidal_heating.rs:1100`). Silicate path validated against Io's surface-dominated heat budget. Ammoniacal (Enceladus / Ganymede) is best-guess mid-point. |
| `tidal_heating.rs:178-180` | `default_subsurface_heat_fraction` | 0.80 | `Earth-fitted` (matches astro-review "Direct 80% to subsurface") | `unvalidated` | Substrate-agnostic fallback. Production paths thread the actual substrate. |
| `tidal_heating.rs:189-191` | `love_number_rocky` (k₂) | 0.30 | `literature` (Earth k₂ ≈ 0.299) | `validated` | Used by `k2_over_q_rocky / icy` — both pinned by Io and Enceladus calibration tests. |
| `tidal_heating.rs:202-204` | `q_factor_rocky` | 100 | `literature` (Io's effective Q) | `validated` | Io calibration pins this. |
| `tidal_heating.rs:211-213` | `q_factor_icy` | 1000 | `literature` (Europa-class icy moons) | `partial` | Pins Enceladus within window; Europa is the `known-bad` from `tidal_dimensional_calibration`. |
| `tidal_heating.rs:322-324` | `orbital_energy_scale_per_e_squared` | 15_700 | `Earth-fitted` (preserves pre-P3.8 Earth-Moon damping rate `k ≈ 0.10/macro`) | `unvalidated` | Magic constant. Energy-conservation tautology `H = -dE/dt` holds *algebraically* because both rates trace through the same calibration; but the absolute scale is fitted, not derived from `(GMm/2a)`. |

#### Atmospheric escape (`atmospheric_escape.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `atmospheric_escape.rs:145-146` | `JEANS_BASE_NUM / DEN` | `1/100_000` per tick | `empirical-best-fit` | `unvalidated` | Per-tick Jeans-loss rate before the `exp(-λ)` suppression. Calibrated so Jeans loss stays < 1%/Gyr at permissive λ. Cadence-coupled (per-tick) so absolute rate is inflated by surface-T vs exobase-T issue noted at `:163-169`. |
| `atmospheric_escape.rs:153` | `JEANS_T_FLOOR_K` | 100 K | `arithmetic` (avoid division-by-zero on frozen cells) | n/a | Numerical safety. |
| `atmospheric_escape.rs:179` | `JEANS_COEFFICIENT` | 6 | `dimensional` (~60 physical / surface-T-to-exobase-T ratio ~10) | `validated` | Real coefficient is ~60 at the exobase. Divided by ~10 to apply at surface T. H/He fractionation test `h_vs_he_fractionation_ratio_above_thousand` and Mars CO2 test `mars_co2_retention_higher_than_h2o` pin discrimination behaviour. |
| `atmospheric_escape.rs:186` | `JEANS_LAMBDA_MAX` | 21 | `arithmetic` (Q32.32 underflow at λ ≈ 22) | n/a | `exp(-λ)` would underflow Q32.32 (2.33e-10 floor). Clamping at 21 keeps retention ratios finite. |
| `atmospheric_escape.rs:195-196` | `HYDRODYNAMIC_BASE_NUM / DEN` | `1/10` per tick | `Solar-system-fitted` (Mars primordial-atmosphere loss timescale) | `unvalidated` | Base hydrodynamic blow-off rate. Tuned so 100× early-Sun EUV strips a Mars-equivalent dramatically over Gyr scales. Magnitude of "young Mars lost most CO2" is the only anchor. |
| `atmospheric_escape.rs:203` | `HYDRODYNAMIC_T_REF_K` | 300 K | `Earth-fitted` (Earth surface T = 288 K → factor ~1.0) | `unvalidated` | Hydrodynamic thermal scaling. |
| `atmospheric_escape.rs:213-214` | `PHOTOCHEMICAL_BASE_NUM / DEN` | `1/100_000` per tick | `Solar-system-fitted` (Mars ~2 kg/s H from H2O photolysis) | `unvalidated` | Order-of-magnitude Mars photolysis anchor. |
| `atmospheric_escape.rs:219` | `PHOTOCHEMICAL_UV_REF_W_M2` | 100 | `Earth-fitted` (modern Earth ~90 W/m² near-UV → factor ~0.9) | `unvalidated` | UV reference flux. |
| `atmospheric_escape.rs:228-229` | `ION_BASE_NUM / DEN` | `1/10_000` per tick | `Solar-system-fitted` (Mars ~3 kg/s O ion pickup) | `unvalidated` | Principal Mars-vs-Earth differentiator (unmagnetised). |
| `atmospheric_escape.rs:253-269` | `substance_weight` (Methane 1.0, Vapour 0.9, Oxidiser 0.5, CO2 0.2) | per-Substance fractions | `dimensional` (gentle inverse-mass linear weight for non-Jeans channels) | `unvalidated` | Non-Jeans mass discrimination. Jeans itself now uses `molecular_mass_amu` exponentially. |
| `atmospheric_escape.rs:285-293` | `molecular_mass_amu` | CH4 16, H2O 18, O2 32, CO2 44 | `literature` (real molecular masses) | `validated` | True AMU values; physical. |
| `atmospheric_escape.rs:301, 305` | `HYDROGEN_MASS_AMU`, `HELIUM_MASS_AMU` | 1, 4 | `literature` | `validated` | For H/He fractionation test. |

#### Radiation / greenhouse (`radiation.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `radiation.rs:108-110` | `night_factor` | 0.05 | `literature` (synchronous-world antistellar limb ~5% input) | `unvalidated` | Synchronous-world night-side absorption fraction. |
| `radiation.rs:123-125` | `h2o_greenhouse_k` | 0.002 per unit density | `empirical-best-fit` (K-I runaway threshold at T ~ 350-400 K) | `unvalidated` | Lower K = higher runaway threshold. Calibrated against `saturation_vapour_cap` curve so feedback `d(T_eq)/dT` crosses unity around K-I temperatures. Venus runaway plateau test still missing. |
| `radiation.rs:133-135` | `co2_greenhouse_k` | 0.030 per unit density | `empirical-best-fit` (15× H2O coefficient; Earth 400 ppm contribution small, Venus dense atmosphere bumps T_eq) | `unvalidated` | Order-of-magnitude per-molecule strength ratio CO2/H2O. |
| `radiation.rs:141-143` | `ch4_greenhouse_k` | 0.025 per unit density | `empirical-best-fit` | `unvalidated` | Similar order to CO2; transient warming pulses. |
| `radiation.rs:152-154` | `ch4_decay_per_tick` | 0.999 | `literature` (Earth CH4 photolysis lifetime ~10 years; 1.0 density halves in ~700 ticks) | `unvalidated` | Per-tick CH4 photolysis decay. Could be coupled to UV in future. |
| `radiation.rs:174-176` | `greenhouse_cap_k` | 250 K | `empirical-best-fit` (Venus surface T ~735 K ≈ 300 K base + 400 K greenhouse + cap headroom) | `unvalidated` | Caps band-saturation. Hard cap exists to bound K-I feedback before Q32.32 overflow (~5300 K). Modelling choice, not a recovered observable. |
| `radiation.rs:185-187` | `ln_sigma` | -16.685 | `literature` (Stefan-Boltzmann σ = 5.67e-8 W/m²/K⁴) | `validated` | Pre-computed `ln(σ)`. |
| `radiation.rs:194-196` | `relaxation_rate` | 2% per tick | `empirical-best-fit` (~50-tick thermal-equilibration timescale) | `unvalidated` | Radiative relaxation rate. |
| `radiation.rs:203` | `SEASONS_PER_YEAR` | 12 | `arithmetic` (months in a year) | n/a | Pre-computed seasonal table size. |

#### Weathering (`weathering.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `weathering.rs:67-68` | `WEATHERING_BASE_NUM / DEN` | `1e-5` per tick | `Earth-fitted` (empirical "weathering ~10× per Gyr per K of warming") | `unvalidated` | Per-tick base weathering rate. CO2 steady-state ~280 ppm test still missing. |
| `weathering.rs:74` | `T_REF_K` | 290 K | `Earth-fitted` (modern Earth surface mean) | `unvalidated` | Arrhenius reference temperature. |
| `weathering.rs:83` | `EA_OVER_R_K` | 5000 | `literature` (silicate weathering Ea/R; per-Berner-Kothavala) | `unvalidated` | Earth-anchored activation energy. |
| `weathering.rs:89` | `ARRHENIUS_EXPONENT_CLAMP` | 15 | `arithmetic` (keep `exp` inside Q32.32 range) | n/a | Numerical safety on the Arrhenius exponent. |
| `weathering.rs:96-97` | `T_FACTOR_MIN_NUM / DEN` | 1/1000 | `arithmetic` (lower clamp on T_factor) | n/a | Avoid zero on cold cells. |
| `weathering.rs:104` | `T_FACTOR_MAX` | 100 | `arithmetic` (upper clamp on T_factor) | n/a | Prevents runaway on hot cells. |
| `weathering.rs:112` | `REF_HUMIDITY` | 1000 | `Earth-fitted` | `unvalidated` | Reference humidity for precipitation multiplier. |
| `weathering.rs:117` | `PRECIP_FACTOR_MAX` | 5 | `arithmetic` | n/a | Upper clamp on precipitation multiplier. |

#### Volcanism (`volcanism.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `volcanism.rs:34-36` | `VOLCANIC_CO2_NUM / DEN` | `1/100_000` per boundary cell per tick | `Earth-fitted` (Earth degassing ~1e8 tons CO2/yr from MOR; per-tick per-cell back-out) | `unvalidated` | Boundary CO2 emission rate. |
| `volcanism.rs:38-40` | `VOLCANIC_H2O_NUM / DEN` | `5/100_000` per boundary cell per tick | `Earth-fitted` (5× CO2 mass ratio in volcanic gas) | `unvalidated` | Boundary H2O emission rate. |
| `volcanism.rs:42-44` | `HOT_SPOT_PROBABILITY_NUM / DEN` | `10/1_000_000` (1e-5/tick) | `Earth-fitted` (LIPs every ~30 Myr → 1e-5 per cell per month) | `unvalidated` | Per-cell per-tick hot-spot trial probability. |
| `volcanism.rs:46-48` | `HOT_SPOT_CO2_NUM / DEN` | `1/10_000` | `empirical-best-fit` (10× boundary rate when it fires) | `unvalidated` | Per-eruption CO2 dose. |
| `volcanism.rs:50-52` | `HOT_SPOT_H2O_NUM / DEN` | `5/10_000` | `empirical-best-fit` (10× boundary rate) | `unvalidated` | Per-eruption H2O dose. |

#### Magnetism (`magnetism.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `magnetism.rs:111-112` | `LOCAL_FIELD_MAX_NUM / DEN` | 1.5 | `Solar-system-fitted` (Mars-highlands-class crustal remanence) | `unvalidated` | Max local-field intensity. |
| `magnetism.rs:124-125` | `REMANENCE_SCALE_NUM / DEN` | 0.5 | `Solar-system-fitted` (Mars-highlands umbrella above dipole) | `unvalidated` | Crustal-remanence per-cell scale. |
| `magnetism.rs:132` | `REMANENCE_REF_THICKNESS_KM` | 35 | `Earth-fitted` (mirrors `tectonics::CONTINENTAL_THICKNESS_KM`) | `unvalidated` | Reference thickness for remanence weighting. |
| `magnetism.rs:140-141` | `MIN_DIPOLE_STRENGTH_NUM / DEN` | 0.1 | `literature` (real geomagnetic excursions weaken to ~10% nominal) | `unvalidated` | Strength floor at reversal midpoint. Drives 5× cosmic-ray flux amplification. |
| `magnetism.rs:153-159` | `REVERSAL_TRIAL_NUM / DEN` | `1 / (250_000 × MONTHS_PER_YEAR)` = `1/3_000_000` per tick | `Earth-fitted` (one reversal per ~250 000 years on the per-month physics clock) | `validated` | Per-month trial probability. Scaled by `MONTHS_PER_YEAR = 12` so the per-year rate matches the Earth-like target regardless of the per-month cadence (T1). |
| `magnetism.rs:166` | `REVERSAL_DURATION_TICKS` | `1000 × MONTHS_PER_YEAR` = `12_000` ticks (~1000 years) | `Earth-fitted` | `validated` | Reversal-window duration in month-ticks. Scaled by `MONTHS_PER_YEAR = 12` so a 1000-year reversal stays 1000 years (T1). |

#### Tectonics (`tectonics.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `tectonics.rs:128` | `OCEANIC_THICKNESS_KM` | 7 | `literature` (Earth MORB) | `unvalidated` | |
| `tectonics.rs:132` | `CONTINENTAL_THICKNESS_KM` | 35 | `literature` (Earth continental crust) | `unvalidated` | |
| `tectonics.rs:137` | `MIN_PLATES` | 8 | `Earth-fitted` (Earth ~8 major plates) | `unvalidated` | |
| `tectonics.rs:143` | `MAX_PLATES` | 15 | `Earth-fitted` (Earth ~15 plates major+minor) | `unvalidated` | |
| `tectonics.rs:150` | `OCEANIC_PERCENT` | 60 | `Earth-fitted` (modern Earth ~70% ocean, 60% chosen as worldgen baseline with jitter) | `unvalidated` | |
| `tectonics.rs:163` | `SUBDUCTION_DT_TICKS` | 100 | `empirical-best-fit` (geological timescale per macro-step) | `unvalidated` | |
| `tectonics.rs:171` | `MIN_CRUST_THICKNESS_KM` | 1 | `arithmetic` (avoid zero) | n/a | |
| `tectonics.rs:181` | `RIDGE_DEPTH_PREFACTOR` | 350 | `literature` (Parsons-Sclater ridge subsidence ~350 m × √age_Myr) | `unvalidated` | |
| `tectonics.rs:194` | `AGE_TICK_SCALE` | 10_000 | `arithmetic` (convert ticks → Myr-like units for ridge depth) | n/a | Per-cell age scaling. |
| `tectonics.rs:204-205` | `OCEAN_DEPTH_K_NUM / DEN` | 1/100 | `empirical-best-fit` | `unvalidated` | Ocean-depth-from-elevation conversion. |
| `tectonics.rs:217-219` | `slab_pull_factor` | 1e-4 per cell-unit per tick | `empirical-best-fit` (Himalaya-scale uplift over thousands of ticks) | `unvalidated` | Per-tick slab-pull strength. |
| `tectonics.rs:230-232` | `slab_pull_density_contrast_oc_cont` | 0.22 | `literature` (oceanic 3.0 g/cm³ vs continental 2.7 g/cm³; effective pull higher because of overriding-plate buoyancy transfer) | `unvalidated` | |
| `tectonics.rs:243-245` | `slab_pull_density_contrast_oc_oc` | 0.05 | `empirical-best-fit` (both basaltic; cooler/denser side wins marginally) | `unvalidated` | |
| `tectonics.rs:255-257` | `max_plate_velocity` | 5 cells/tick per axis | `arithmetic` (2.5× worldgen initial `[-2, +2]` window) | n/a | Stability cap. |

#### Wind / fluid / hydrology (`wind.rs`, `hydrology.rs`, `fluid.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `wind.rs:90` | `MAX_WIND_SUB_STEPS` | 16 | `arithmetic` (CFL sub-step cap) | n/a | Stability cap on CFL subdivision. |
| `wind.rs:96` | `CFL_SAFETY` | 0.5 | `literature` (standard CFL practice ≤ 1, 0.5 leaves margin) | `validated` | Numerical-stability constant. |
| `wind.rs:104` | `GAMMA_GAS` | 7/5 = 1.4 | `literature` (diatomic ideal gas; Earth atmosphere) | `unvalidated` | Good to a few percent for Mars/Venus/Titan; CFL safety absorbs residual. |
| `hydrology.rs:113` | `DEFAULT_SCALE_HEIGHT_M` | 8400 | `literature` (Earth scale height) | `partial` | Per-planet override via `Hydrology::for_substrate(scale_height_m)`. |
| `hydrology.rs:123` | `SAT_CAP_T_REF_K` | 373 | `literature` (water boiling point at 1 atm) | `unvalidated` | Reference T for Clausius-Clapeyron-like quartic. |
| `hydrology.rs:132` | `SAT_CAP_C_BASE` | 50_000 | `empirical-best-fit` (5× old `10_000` floor; well below Q32.32 ceiling) | `unvalidated` | Peak vapour capacity at T_ref. |
| `hydrology.rs:139` | `SAT_CAP_FLOOR` | 100 | `arithmetic` (avoid div-by-zero downstream) | n/a | Safety floor. |

#### Hadley circulation (`hadley.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `hadley.rs:181` | `EARTH_RADIUS_M` | 6_371_000 | `literature` | `validated` | Planet radius reference. |
| `hadley.rs:185` | `DEFAULT_SCALE_HEIGHT_M` | 8400 | `literature` (Earth) | `partial` | Per-planet override available. |
| `hadley.rs:199` | `DEFAULT_DELTA_THETA_K` | 60 | `Earth-fitted` (equator-to-pole potential-temperature contrast) | `unvalidated` | Held-Hou closure input. |
| `hadley.rs:205` | `DEFAULT_T_EQ_K` | 300 | `Earth-fitted` (tropical mean) | `unvalidated` | |
| `hadley.rs:212` | `DEFAULT_TROPOPAUSE_M` | 12_000 | `Earth-fitted` (Earth tropopause) | `unvalidated` | |
| `hadley.rs:603` | `kick_fraction` | 1% per tick | `empirical-best-fit` (small enough that steady-state jet builds over many ticks; tuned for Earth jet ~30 m/s) | `unvalidated` | Per-tick band-velocity nudge. Test `:954` pins Earth jet ∈ [10, 60] m/s. Unit-system-absorbing — Ω·R not threaded through SI. |
| `hadley.rs:465-466` | `cell_count_thresholds` `[1.0, 2.3, 4.0, 6.0]` | Rossby-ratio bands | `empirical-best-fit` (deferred — not derived from baroclinic instability) | `unvalidated` | Open issue #4 in post-fix-astro-review "new gaps". |

#### Albedo (`albedo.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `albedo.rs:65-67` | `sigmoid_width_k` | 5 K | `empirical-best-fit` (narrow enough to amplify modest perturbations into snowball bifurcation; wide enough to keep integrator differentiable) | `unvalidated` | Freeze-line sigmoid width. |
| `albedo.rs:72-74` | `snow_peak_albedo` | 0.85 | `literature` (boreal / glacial snow) | `unvalidated` | |
| `albedo.rs:80-82` | `sea_ice_peak_albedo` | 0.55 | `literature` (mid of published 0.4-0.7 range) | `unvalidated` | |
| `albedo.rs:90-92` | `cloud_peak_albedo` | 0.40 | `literature` (canonical mid-cloud) | `unvalidated` | Type-agnostic peak. |
| `albedo.rs:100-102` | `stratus_peak_albedo` | 0.50 | `literature` (high end of stratiform range) | `unvalidated` | |
| `albedo.rs:109-111` | `cirrus_peak_albedo` | 0.20 | `literature` (high-altitude thin ice clouds) | `unvalidated` | |
| `albedo.rs:185-194` | `crust_base_albedo` per type | Basaltic 0.10, Granitic 0.20, Sedimentary 0.25, Icy 0.50, Hydrocarbon 0.15, Default 0.20 | `per-substrate` from `literature` (published per-surface reflectivity) | `validated` | Tested by `basaltic_crust_has_lower_base_albedo_than_granitic` and `icy_crust_has_high_base_albedo_even_without_snow`. |
| `albedo.rs:218-225` | water/veg base albedos | water 0.06, veg 0.15, else `crust_base_albedo` | `literature` (ocean ~0.06, vegetation 0.10-0.20) | `unvalidated` | |

#### Chemistry constants (`chemistry/constants.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `chemistry/constants.rs:7` | `C_P_WATER` | 4_186 J/kg/K | `literature` | `validated` | Specific heat of water. |
| `chemistry/constants.rs:11` | `C_P_AMMONIA` | 4_700 J/kg/K | `literature` | `validated` | |
| `chemistry/constants.rs:14` | `C_P_METHANE` | 3_500 J/kg/K | `literature` | `validated` | |
| `chemistry/constants.rs:17` | `C_P_SILICATE` | 1_300 J/kg/K | `literature` | `validated` | |
| `chemistry/constants.rs:19-34` | `L_FUSION_*`, `L_VAPORISATION_*` | Real latent heats | `literature` | `validated` | Per-substrate phase-change heats. |
| `chemistry/constants.rs:36, 42` | `COMBUSTION_ENTHALPY_WOOD / FOSSIL` | 16e6, 42e6 J/kg | `literature` | `validated` | |
| `chemistry/constants.rs:58` | `P_REF_ATM_PA` | 101_325 | `literature` (1 atm in Pa) | `validated` | |
| `chemistry/constants.rs:66` | `CELL_THERMAL_MASS_KG` | 539 | `empirical-best-fit` (per-cell thermal-mass calibration) | `unvalidated` | |

#### Other physics

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `isostasy.rs:79` | `CRUST_TYPE_THICKNESS_THRESHOLD_KM` | 20 | `empirical-best-fit` (oceanic/continental tie-break) | `unvalidated` | |
| `state.rs:33` | `N_SUBSTANCES` | 9 | `arithmetic` | n/a | Substance enum cardinality. |

### `sim/world/` — worldgen & stellar evolution

#### Star (`star.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `star.rs:86-91` | `flare_rate_per_tick` per class | M 100, K 10, G 1.0, F 0.3, A 0.1 | `literature` (relative spectral-type flare rates) | `unvalidated` | G-dwarf is baseline 1.0. |
| `star.rs:102-107` | `nominal_lifetime_gyr` per class | M 1000, K 25, G 10, F 5, A 2 | `literature` (canonical class-mean MS lifetimes) | `validated` (G anchor) | Modern-Sun anchor lands ~1.03× present-day luminosity in `bolometric_scale_at_age`. |
| `star.rs:116-128` | `nominal_luminosity_solar` per class | M 0.04, K 0.4, G 1.0, F 2.5, A 12 | `literature` | `unvalidated` | |
| `star.rs:148-187` | `sed_fractions` per class | M (3/2/10/85), K (2/5/33/60), G (1/8/41/50), F (2/16/47/35), A (5/30/50/15) | `literature` (per-class blackbody curve with EUV boost for cool stars) | `unvalidated` | Sums to ~1.0 modulo rounding. |
| `star.rs:353` | red-giant onset fraction | 0.95 × lifetime | `literature` (MS-end → red-giant transition timing) | `unvalidated` | |
| `star.rs:377` | HZ inner coefficient | 0.95 AU × √(L / 1361) | `literature` (Kasting moist-greenhouse boundary) | `validated` | Wired into habitability tests. |
| `star.rs:387` | HZ outer coefficient | 1.37 AU × √(L / 1361) | `literature` (Kasting maximum-greenhouse boundary) | `validated` | Wired into habitability tests. |
| `star.rs:413-423` | `bolometric_scale_at_age` ZAMS / MS-end | 0.70 / 1.40 | `literature` (faint-young-sun anchor; bright-old-sun ramp) | `validated` | Modern-Sun lands at ~1.03×; test `faint_young_sun_*` pins. |
| `star.rs:442` | red-giant ramp end | 1000× | `literature` (RGB luminosity peak) | `unvalidated` | |
| `star.rs:468-472` | `EUV_DECAY_GYR_NUM / DEN` | 0.1 Gyr | `literature` (Ribas-et-al / Sanz-Forcada `t^(-1.5)` X-ray/EUV decay) | `validated` | Test `euv_decay_follows_t_to_minus_1_5` pins shape. |
| `star.rs:502` | EUV decay exponent | -1.5 | `literature` (canonical power-law fit) | `validated` | |

#### Sampling (`sampling.rs`) — locking state

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `sampling.rs:21` | `LOCKING_SALT` | `0x4C6F636B696E6721` | `arithmetic` (SplitMix64 salt) | n/a | "Locking!" ASCII; isolates locking jitter from main RNG. |
| `sampling.rs:595` | locking-state mass threshold | `mass_relative_x100 > 10` | `Earth-fitted` (Earth-Moon mass ratio anchor) | `unvalidated` | Synchronous-lock heuristic input. |
| `sampling.rs:595` | locking-state period threshold | `orbital_period_macros < 100` | `Solar-system-fitted` (close-orbit threshold ~100 days) | `unvalidated` | |
| `sampling.rs:617` | resonance tolerance band | ±5% around 3:2 / 2:3 | `empirical-best-fit` (wider than strict equality; narrower than 10% to avoid over-claim) | `unvalidated` | |
| `sampling.rs:632` | resonance jitter rate | 5% (`u64::MAX / 20`) | `empirical-best-fit` (variety floor for Resonance population) | `unvalidated` | Salted with `LOCKING_SALT` for byte-replay stability. |
| `sampling.rs:714-715` | sampling jitter window | ±10% | `empirical-best-fit` | n/a | Per-channel jitter on category-derived baselines. |

#### Planet (`planet.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `planet.rs:17` | `EARTH_GRAVITY_MS2_X100` | 981 | `literature` (Earth g × 100) | `validated` | |
| `planet.rs:22` | `EARTH_RADIUS_M` | 6_371_000 | `literature` | `validated` | |

#### Habitability (`habitability.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `habitability.rs:56-57` | `CLAIM_HABITABILITY_THRESHOLD_NUM / DEN` | 5/100 = 0.05 | `empirical-best-fit` (5% habitability for civ to claim a cell) | `unvalidated` | |

### `sim/ecosystem/` — trophic dynamics & speciation

#### Lindeman pyramid (`lib.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `lib.rs:83` | `LINDEMAN_RATIO` | 1/10 = 0.10 | `literature` (canonical Lindeman 10:1 trophic-transfer efficiency) | `validated` | Terrestrial default. |
| `lib.rs:87-95` | `lindeman_assimilation_for_habitat` | Aquatic 1/30 ≈ 0.033, Terrestrial 1/10 = 0.10, Amphibious/Airborne 0.15 | `per-substrate` from `literature` (aquatic 30:1 fish pyramid; terrestrial 10:1) | `validated` | Tests `aquatic_habitat_uses_30_to_1_lindeman_ratio` and `terrestrial_habitat_uses_10_to_1_lindeman_ratio` pin both per-habitat values. |
| `lib.rs:97` | `LINDEMAN_OVERSHOOT_DEBUG_MAX` | 5 | `arithmetic` (debug-only invariant slack) | n/a | Permitted overshoot before invariant flags. |
| `lib.rs:102` | `K_HALF_SAT_DEFAULT` | 1/2 = 0.5 | `literature` (Holling Type-II default half-saturation) | `validated` | Only reached when `Interaction::half_saturation = ZERO`. |
| `lib.rs:105` | `HALF_SAT_APEX_PREDATOR` | 1/10 = 0.10 | `empirical-best-fit` (apex predators saturate fast on sparse prey) | `unvalidated` | |
| `lib.rs:106` | `HALF_SAT_SPECIALIST_PREDATOR` | 3/10 = 0.30 | `empirical-best-fit` (specialists like lynx-hare) | `unvalidated` | |
| `lib.rs:107` | `HALF_SAT_MUTUALISM` | 5/10 = 0.50 | `empirical-best-fit` | `unvalidated` | |
| `lib.rs:108` | `HALF_SAT_HABITAT_MOD` | 2/10 = 0.20 | `empirical-best-fit` | `unvalidated` | |
| `lib.rs:113` | `PRODUCER_GROWTH_RATE` | 2/100 = 0.02 per tick | `empirical-best-fit` (logistic toward carrying capacity) | `unvalidated` | |
| `lib.rs:118` | `CONSUMER_DECAY_RATE` | 1/100 = 0.01 per tick | `empirical-best-fit` (passive mortality between predation events) | `unvalidated` | |
| `lib.rs:125` | `KEYSTONE_CENTRALITY_THRESHOLD` | 15/100 = 0.15 | `empirical-best-fit` (network betweenness threshold) | `unvalidated` | |
| `lib.rs:134` | `SYNTROPHY_MIN_PARTNER_BIOMASS` | 1/100 = 0.01 | `empirical-best-fit` | `unvalidated` | |
| `lib.rs:143` | `SYNTROPHY_COLLAPSE_RATE` | 25/100 = 0.25 per tick | `empirical-best-fit` ("within a few ticks" cascade) | `unvalidated` | |
| `lib.rs:154` | `SEED_DISPERSER_BIOMASS_THRESHOLD` | 5/1000 = 0.005 | `empirical-best-fit` | `unvalidated` | |
| `lib.rs:160` | `SEED_DISPERSER_RANGE_BOOST` | 120/100 = 1.20 | `empirical-best-fit` (+20% extended-range bump) | `unvalidated` | |
| `lib.rs:171` | `POLLINATOR_BIOMASS_COUPLING` | 30 | `empirical-best-fit` | `unvalidated` | |
| `lib.rs:181` | `ENGINEER_MATCH_BOOST` | 10/100 = 0.10 | `empirical-best-fit` (+10% tolerance bump) | `unvalidated` | |
| `lib.rs:189` | `MACRO_FERTILITY_MULTIPLIER` | 10/100 = 0.10 | `literature` (chronic helminth burdens, field rule of thumb) | `unvalidated` | |
| `lib.rs:198-199` | `MICRO_SURVIVAL_PENALTY`, `MICRO_CROWDING_THRESHOLD` | 5/100, 5/100 | `empirical-best-fit` (crowding-disease scaling) | `unvalidated` | |
| `lib.rs:210-211` | `VIRUS_OUTBREAK_PERIOD`, `VIRUS_OUTBREAK_HOST_LOSS` | 100 ticks, 30/100 | `literature` (virgin-soil viral epidemic field rule -30%) | `unvalidated` | |
| `lib.rs:223` | `CHEMOAUTOTROPH_GROWTH_RATE` | 2/100 | `empirical-best-fit` (mirrors `PRODUCER_GROWTH_RATE`) | `unvalidated` | |
| `lib.rs:231, 239` | `EXTINCTION_THRESHOLD_FRAC`, `EXTINCTION_CONFIRMATION_TICKS` | 1/1000, 12 ticks | `empirical-best-fit` (1 sim-year of sustained collapse) | `unvalidated` | |
| `lib.rs:250` | `RESPIRATION_RATE` | 1/100 = 0.01 per tick | `empirical-best-fit` (biogeochem CO2 loop) | `unvalidated` | |
| `lib.rs:262` | `DECOMPOSITION_RATE` | 1/200 = 0.005 per tick | `empirical-best-fit` | `unvalidated` | |

#### Speciation (`speciation.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `speciation.rs:59` | `ALLOPATRIC_ISOLATION_TICKS` | 100 | `empirical-best-fit` (~1 sim-year minimum isolation) | `unvalidated` | |
| `speciation.rs:66` | `SYMPATRIC_COMPETITION_BIOMASS_FRAC` | 5/100 | `empirical-best-fit` | `unvalidated` | |
| `speciation.rs:73` | `SYMPATRIC_PRESSURE_TICKS` | 50 | `empirical-best-fit` (half of allopatric) | `unvalidated` | |
| `speciation.rs:80` | `POLYPLOID_PER_TICK_PROB_RECIP` | 100_000 | `literature` (plant polyploidy ~1e-5/generation) | `unvalidated` | |
| `speciation.rs:86` | `FOUNDER_BIOMASS_FRAC` | 1/100 | `literature` (founder-effect bottleneck rule of thumb) | `unvalidated` | |
| `speciation.rs:90` | `POST_EXTINCTION_RADIATION_MULTIPLIER` | 5 | `literature` (adaptive radiation post-mass-extinction) | `unvalidated` | |
| `speciation.rs:97` | `POST_EXTINCTION_BOOST_TICKS` | 100 | `empirical-best-fit` (~100 generations) | `unvalidated` | |
| `speciation.rs:105` | `DIVERGENCE_AXIS_RANGE` | 5/100 | `empirical-best-fit` (±5% per axis) | `unvalidated` | |
| `speciation.rs:111, 119` | `COSMIC_RAY_MULTIPLIER_FLOOR / CEILING` | 1, 10 | `empirical-best-fit` (clamp on dipole-shielding amplification) | `validated` | Test `cosmic_ray_multiplier_clamps_at_ceiling` pins. |
| `speciation.rs:129` | `INHERITED_INTERACTION_STRENGTH_FRAC` | 6/10 = 0.60 | `literature` (sister species share ~60% niche partners) | `unvalidated` | |
| `speciation.rs:136` | `SISTER_COMPETITION_STRENGTH` | 3/10 = 0.30 | `empirical-best-fit` | `unvalidated` | |
| `speciation.rs:147` | `TEMPERATURE_DISPLACEMENT_REFERENCE_K` | 300 | `Earth-fitted` (mid of aqueous 273-373 K range) | `unvalidated` | |
| `speciation.rs:154` | `RADIATION_DISPLACEMENT_REFERENCE` | 5/10 = 0.50 | `Earth-fitted` (aqueous-default radiation_max) | `unvalidated` | |
| `speciation.rs:164` | `TOLERANCE_DISPLACEMENT_FRAC` | 8/100 = 0.08 | `empirical-best-fit` (visible separation; inside ±20% sampling jitter) | `unvalidated` | |

#### HGT (`hgt.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `hgt.rs:82` | `HGT_BASE_RATE` | 1/10_000 per tick per pair | `empirical-best-fit` (~833 sim-years per acquisition at monthly cadence) | `unvalidated` | |
| `hgt.rs:90` | `SWEEP_THRESHOLD` | 5/100 = 0.05 | `literature` (5% selection coefficient → fixation) | `unvalidated` | |

### `sim/civ/` — civilisations

#### Catastrophes (`catastrophe/mod.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `catastrophe/mod.rs:54-58` | `*_COOLDOWN_TICKS` | Volcanic 200×12, Disease 500×12, Asteroid 5000×12, SolarFlare 800×12, IceAge 4000×12 yr→months | `empirical-best-fit` (sim-years per recurrence) | `unvalidated` | Year-equivalent recurrence at month cadence. |
| `catastrophe/mod.rs:59` | `DISEASE_AGE_FLOOR_TICKS` | 300 yr × 12 | `empirical-best-fit` | `unvalidated` | Minimum civ age before disease eligibility. |
| `catastrophe/mod.rs:62-66` | `*_POP_LOSS` | Volcanic 5%, Disease 30%, Asteroid 40%, SolarFlare 10%, IceAge 20% | `literature` (per-catastrophe historical pop-loss magnitudes) | `unvalidated` | Earth historical / paleontological anchors. |
| `catastrophe/mod.rs:82` | `DORMANCY_SEVERITY_FACTOR` | 1.0 (full severity) | `empirical-best-fit` (placeholder pending per-kind table) | `unvalidated` | All five kinds at full severity for now. |
| `catastrophe/mod.rs:89-91` | `baseline_radiation_flux` | 0.1 | `Earth-fitted` (sub-aqueous-radiation_max 0.5) | `unvalidated` | |
| `catastrophe/mod.rs:98-100` | `solar_flare_radiation_boost` | 1.0 | `empirical-best-fit` | `unvalidated` | |
| `catastrophe/mod.rs:107-109` | `ice_age_temp_drop_k` | 50 K | `literature` (cold-snap delta on Earth ice age) | `unvalidated` | |
| `catastrophe/mod.rs:113-115` | `pa_per_atm` | 101_325 | `literature` | `validated` | Unit conversion. |

#### Drift / castes (`drift.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `drift.rs:24` | `COLLECTIVE_QUORUM_POP` | 100 | `empirical-best-fit` (collective-form minimum) | `unvalidated` | |
| `drift.rs:52-59` | `CASTE_QUORUM_*` | Reproductive 1/100, Worker 50/100, Soldier 10/100, Nurse 10/100 | `literature` (eusocial colony composition; Hymenoptera anchor) | `unvalidated` | Per-caste population fractions. |

#### Conflict (`conflict.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `conflict.rs:509` | `GRUDGE_CEILING` | 60/100 = 0.60 | `empirical-best-fit` (max sustained inter-civ grudge) | `unvalidated` | |

#### Economy / transmission (`economy.rs`, `transmission.rs`, `religion.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `economy.rs:24` | `SURPLUS_UTILIZATION_FLOOR` | 70/100 | `empirical-best-fit` | `unvalidated` | |
| `economy.rs:31` | `SURPLUS_GAIN_PER_TICK` | 1/1000 | `empirical-best-fit` | `unvalidated` | |
| `economy.rs:36` | `SURPLUS_WAR_DRAIN_PER_TICK` | 15/10_000 | `empirical-best-fit` | `unvalidated` | |
| `economy.rs:41` | `SURPLUS_CATASTROPHE_DRAIN_FRAC` | 40/100 | `empirical-best-fit` | `unvalidated` | |
| `economy.rs:47` | `SURPLUS_CEILING_FRAC` | 5 | `empirical-best-fit` | `unvalidated` | |
| `economy.rs:56` | `SURPLUS_EMIT_DELTA_FLOOR` | 50 | `empirical-best-fit` | n/a | Event emit floor. |
| `economy.rs:63-64` | `SURPLUS_FOOD_BUFFER_FULL / BONUS` | 2, 20/100 | `empirical-best-fit` | `unvalidated` | |
| `economy.rs:69` | `SURPLUS_WAR_BONUS_CAP` | 15/100 | `empirical-best-fit` | `unvalidated` | |
| `economy.rs:162` | `TRADE_FLOW_PER_TICK` | 5/10_000 | `empirical-best-fit` | `unvalidated` | |
| `transmission.rs:28-29` | `DECAY_CONSTANT_TICKS`, `TRANSMIT_THRESHOLD` | 1000 yr × 12, 15/100 | `empirical-best-fit` (knowledge half-life ~1000 yr) | `unvalidated` | |
| `religion.rs:214` | `RELIGION_EMIT_THRESHOLD` | 20/100 | `empirical-best-fit` | `unvalidated` | |

#### Resilience / state (`lib.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `lib.rs:529-534` | `FOOD_CRISIS_THRESHOLD / STREAK_TICKS`, `PLATEAU_WINDOW_TICKS`, `CULTURAL_LOCK_*` | 30%, 100 yr, 500 yr, 85%, 250 yr | `empirical-best-fit` | `unvalidated` | Resilience-cycle bookkeeping. |
| `lib.rs:542-556` | `TINY_TERRITORY_*`, `DEPOPULATION_*` | 1 cell, 2 yr, 1 pop, 2 yr | `empirical-best-fit` | `unvalidated` | Collapse triggers. |
| `lib.rs:563-567` | `CIVIL_WAR_COHESION_FLOOR / STREAK_TICKS` | 10%, 75 yr | `empirical-best-fit` | `unvalidated` | |
| `lib.rs:572` | `COHESION_EMIT_THRESHOLD` | 5/100 | `empirical-best-fit` | n/a | |
| `lib.rs:580` | `RESILIENCE_EMIT_DELTA_FLOOR` | 5/100 | `empirical-best-fit` | n/a | |
| `lib.rs:587-604` | `COHESION_BREAKAWAY_*` | 35%, 40 yr, 30%, 85%, 15% | `empirical-best-fit` (breakaway-civ heuristic) | `unvalidated` | |
| `lib.rs:607-610` | `LITERACY_*` | 4%, 20%, 10%, 500 | `empirical-best-fit` | `unvalidated` | |
| `lib.rs:617-619` | `FOUNDING_MIN_POPULATION`, `RECENT_REMNANT_WINDOW_TICKS`, `FOUNDING_MIN_DARK_AGE_TICKS` | 100, 250 yr, 50 yr | `empirical-best-fit` | `unvalidated` | |
| `lib.rs:629-649` | `SUCCESSOR_DRIFT_*`, `SELECTION_BIAS_*` | 2%, 1 yr/step, 15%, 10 yr | `empirical-best-fit` | `unvalidated` | |

#### Demographics (`demographics.rs`)

| File:line | Constant | Value | Origin | Cross-planet | Notes |
|---|---|---|---|---|---|
| `demographics.rs:89` | `REFERENCE_PLANET_CELL_COUNT` | 1080 | `Earth-fitted` (Earth-sized hex grid) | `unvalidated` | Population scaling reference. |

---

## Cross-planet status summary

| Status | Count (approx) | Examples |
|---|---|---|
| `validated` | ~25 | `LINDEMAN_RATIO` (per-habitat), `bolometric_scale_at_age`, `EUV_DECAY_GYR`, HZ inner/outer coefficients, `K_HALF_SAT_DEFAULT`, crust albedos, Stefan-Boltzmann `ln_sigma`, molecular masses, chemistry latents, `COSMIC_RAY_MULTIPLIER_FLOOR / CEILING`, `CFL_SAFETY`, `EARTH_RADIUS_M` |
| `partial` | ~5 | `subsurface_heat_fraction` (per-substrate; Ammoniacal is best-guess), `q_factor_icy` (Enceladus OK, Europa miss), `DEFAULT_SCALE_HEIGHT_M` (Earth pinned, per-planet override exists) |
| `known-bad` | 1 | `tidal_dimensional_calibration` (Europa ~25× shy). `REVERSAL_TRIAL_DEN` / `REVERSAL_DURATION_TICKS` previously sat here for cadence inconsistency; T1 rescaled both by `MONTHS_PER_YEAR` so they now match the per-month physics clock. |
| `unvalidated` | majority | Most ecology / civ constants; most `empirical-best-fit` physics constants without a non-Earth anchor |
| `arithmetic` / numerical | many | Q32.32 clamps, CFL safety, neighbour direction tables — no physical content, do not need cross-planet validation |

---

## TODO / follow-up backlog

Every `unvalidated` and `known-bad` constant in the table above is a
candidate for a future calibration test. Below is the priority-ordered
backlog the astrophysics review identified for the most load-bearing
constants. Each item is what would need to happen to flip the
constant from `unvalidated` → `validated` (or `known-bad` →
`validated`).

### Physics — priority A (known-bad)

1. **`tidal_dimensional_calibration` Europa gap (~25× shy)**.
   Diagnosed at `tidal_heating.rs:78-104`. Fix paths:
   - (a) sub-day macro-step (so 3.55-day Europa orbit isn't rounded
     to 4 macros — recovers `(4/3.55)⁵ ≈ 1.65×`), or
   - (b) per-substrate calibration multiplier (so the Io-anchored
     5.4× empirical factor doesn't apply to icy moons). Currently
     the `europa_like_*` tests pin the *produced* range rather than
     the literature value with a `FIXME: calibration` flag.
2. ~~**`REVERSAL_TRIAL_DEN`, `REVERSAL_DURATION_TICKS` cadence
   inconsistency**~~ — **resolved (T1)**. Both constants are now
   scaled by `MONTHS_PER_YEAR = 12` in `magnetism.rs`, so the
   per-month physics clock produces the documented per-year cadence
   (one reversal per ~250 000 years, ~1000-year window). Markov-chain
   frequency and reversal-envelope tests verify the new scaling.

### Physics — priority B (anchors missing)

3. **Venus runaway plateau test missing**. `greenhouse_cap_k = 250 K`,
   `h2o_greenhouse_k = 0.002`, `co2_greenhouse_k = 0.030` jointly
   determine the runaway plateau. No test asserts the plateau lands
   at T ∈ [700, 770] K.
4. **Carbonate-silicate steady-state ~280 ppm CO2 missing**.
   `WEATHERING_BASE`, `T_REF_K`, `EA_OVER_R_K`, volcanic emission
   rates jointly determine the CO2 steady state. No test pins
   Earth-analog at ~280 ppm.
5. **Mars-MAVEN absolute escape rates ~2-3 kg/s per channel
   missing**. Hydrodynamic / photochemical / ion base rates only
   have ratio tests (Earth-vs-Mars), not absolute.
6. **Walker-Hays-Kasting snowball recovery timescale missing**.
   `sigmoid_width_k`, `snow_peak_albedo`, `relaxation_rate` jointly
   determine snowball bifurcation; no test enforces realistic
   recovery time.
7. **Earth jet velocity tightening**. `kick_fraction = 1%` test
   currently allows factor-of-2 slack (`[10, 60] m/s`); pin to
   30 m/s ±20%.
8. **Hadley cell-count thresholds `[1.0, 2.3, 4.0, 6.0]`** not
   derived from baroclinic instability. Either derive (Phillips
   stability criterion) or document as `empirical-best-fit`
   long-term.
9. **`orbital_energy_scale_per_e_squared = 15_700`** still
   Earth-Moon-fitted. Derive from `(GMm/2a)` first-principles so
   the value is automatic per moon mass/orbit, not magic.

### Physics — priority C (substrate coverage)

10. **`subsurface_heat_fraction` Ammoniacal at 0.60** is
    best-guess between Aqueous (0.90) and Silicate (0.30). No
    Enceladus/Ganymede-anchored test pins this.
11. **`cirrus_greenhouse_k` lapse-rate coupling**. Currently a
    constant ~15 K per unit cloud fraction; should derive from
    cloud-top T × lapse rate. Coupling gap #7 from astro review.
12. **`tide_k`, `wind_k` mass-radius decoupling**. Currently
    Earth-tuned scalars; should thread through `Planet::gravity()`.
    Coupling gap #2 from astro review.

### Ecology / civ — priority B

13. **`LINDEMAN_OVERSHOOT_DEBUG_MAX = 5`** is a debug-only invariant
    slack. Document or remove.
14. **Half-saturation per-pair anchors** (`HALF_SAT_APEX_PREDATOR`,
    `HALF_SAT_SPECIALIST_PREDATOR`, etc.) have qualitative tests
    (apex saturates faster) but no published-anchor pinning.
15. **`HGT_BASE_RATE = 1e-4`** per-tick rate not anchored to a
    measured horizontal-transfer event frequency. Per-planet/
    per-substrate scaling absent.
16. **`POLYPLOID_PER_TICK_PROB_RECIP = 1e-5`** plant-only anchor;
    no per-substrate variation despite different chemistry.
17. **Catastrophe `POP_LOSS` values** are Earth historical anchors;
    no extrapolation tests for alien planet populations.

### Bookkeeping

18. Audit all `unvalidated` constants and decide which graduate to
    `validated` (test added) vs documented as `empirical-best-fit`
    long-term acceptable. The current list is non-prioritised —
    a triage pass would help.
19. The 16-anchor backlog from the xeno side
    (`docs/post-fix-xeno-review.md`) — apex-predator cascade,
    mass-extinction recovery, etc. — overlaps with items 13-17
    here and should be merged into one calibration backlog.

---

## Provenance

- Source: every constant above was extracted by grep+Read across
  `sim/{physics,world,ecosystem,civ}/src/` on 2026-05-22 at the
  state of branch `claude/f4-magic-constants-doc`.
- This document is **descriptive**. It enumerates what is in the
  code; it does not change any value. Calibration changes (renames,
  re-tunes, additions of validation tests) belong in separate
  follow-up branches.
- Cross-references: `docs/post-fix-astro-review.md` (condition 5),
  `docs/post-fix-xeno-review.md`, `docs/post-implementation-fixes.md`,
  `docs/physics.md`.
