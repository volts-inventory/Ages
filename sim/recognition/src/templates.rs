//! `RecognitionLibrary::earth_like_default` — the M2 default
//! template set covering signatures the M1a + M1b physics layer
//! reliably produces. ~810 lines of template definitions; lives
//! in its own file so the smaller pieces of `lib.rs` (types,
//! `PlanetContext`, `scan`) aren't drowned by it.

use crate::{
    ChannelKind, ClimateBand, Field, FormTag, Hemisphere, RecognitionLibrary, RecognitionTemplate,
    Signature,
};
use sim_arith::Real;
use sim_physics::Substance;

impl RecognitionLibrary {
    /// The default M2 set covering signatures the M1a + M1b physics
    /// layer reliably produces. SI thresholds: temperatures
    /// in K, water depth in m. Charge stays in arbitrary sim-units
    /// pending follow-up Coulomb-law structuring.
    #[allow(clippy::too_many_lines)]
    pub fn earth_like_default() -> Self {
        Self {
            templates: vec![
                // Fire: hot enough to ignite (planet-derived
                // ignition threshold from `build_laws`, e.g. 500 K
                // on Oxidising / 700 K on Hazy / 900 K on Reducing
                // — atmospheres ignite at different temperatures)
                // + has fuel + has oxidiser. Same conditions the
                // combustion reaction fires on, surfaced here as an
                // observable phenomenon.
                RecognitionTemplate {
                    id: 1,
                    name: "fire",
                    signature: Signature::All(vec![
                        Signature::AboveIgnition,
                        Signature::Above(Field::Substance(Substance::Fuel), Real::ZERO),
                        Signature::Above(Field::Substance(Substance::Oxidiser), Real::ZERO),
                    ]),
                    tags: &[FormTag::Threshold],
                    channels: &[
                        ChannelKind::VisualLight,
                        ChannelKind::InfraredThermal,
                        ChannelKind::ChemicalTaste,
                        ChannelKind::Tactile,
                    ],
                },
                // Lightning buildup: |charge| nearly at the discharge
                // threshold. Recognition runs in its own phase
                // before EM, so this catches the moment before
                // discharge zeroes the charge.
                RecognitionTemplate {
                    id: 2,
                    name: "lightning_buildup",
                    signature: Signature::AbsAbove(Field::Charge, Real::from_int(40)),
                    tags: &[FormTag::Threshold, FormTag::ExponentialChange],
                    channels: &[ChannelKind::ElectricField, ChannelKind::Tactile],
                },
                // Ice presence: ice substance above a small floor.
                RecognitionTemplate {
                    id: 3,
                    name: "ice_present",
                    signature: Signature::Above(Field::Substance(Substance::Ice), Real::percent(1)),
                    tags: &[FormTag::Threshold],
                    channels: &[
                        ChannelKind::VisualLight,
                        ChannelKind::Tactile,
                        ChannelKind::InfraredThermal,
                    ],
                },
                // Vapour presence.
                RecognitionTemplate {
                    id: 4,
                    name: "vapour_present",
                    signature: Signature::Above(
                        Field::Substance(Substance::Vapour),
                        Real::percent(1),
                    ),
                    tags: &[FormTag::Threshold, FormTag::ExponentialChange],
                    channels: &[
                        ChannelKind::VisualLight,
                        ChannelKind::ChemicalTaste,
                        ChannelKind::Tactile,
                    ],
                },
                // Surface water: appreciable column above terrain
                // (>1 m of standing water).
                RecognitionTemplate {
                    id: 5,
                    name: "surface_water",
                    signature: Signature::Above(Field::WaterDepth, Real::ONE),
                    tags: &[FormTag::Threshold, FormTag::Polynomial],
                    channels: &[ChannelKind::VisualLight, ChannelKind::Tactile],
                },
                // Latent templates: perceivable only via tools
                // that grant the rare modality channels. Most species
                // can't sense these natively; on planets with the
                // right physics + a civ that builds the right tool,
                // they fire and contribute their tags to the form
                // vocabulary.
                // Reads the real magnetic vector field
                // magnitude (sqrt(B_q² + B_r²)) instead of the
                // earlier `Field::Charge` proxy. With
                // dipole_strength = 50 for Strong magnetosphere,
                // the equatorial cells hit |B| ≈ 50 and trip the
                // 20-K threshold; polar cells with `lat_factor → 0`
                // stay silent. A Weak (10) or None (0) magnetosphere
                // never reaches threshold anywhere — matching the
                // "strong" qualifier in the template name. The
                // earlier charge proxy fired on lightning build-up,
                // which was the *wrong* physics for the named
                // behaviour.
                RecognitionTemplate {
                    id: 6,
                    name: "magnetic_field_strong",
                    signature: Signature::AbsAbove(Field::MagneticMagnitude, Real::from_int(20)),
                    tags: &[FormTag::DistanceDecay, FormTag::PowerOrLog],
                    channels: &[ChannelKind::MagneticSense],
                },
                // Hot-side band relative to the planet's
                // mean. Earth's 310 K became `Hot` (above mean +
                // gradient/2) so a 230 K sub-surface ocean can
                // still observe hot zones at ~242 K — its own hot.
                RecognitionTemplate {
                    id: 7,
                    name: "thermal_gradient",
                    signature: Signature::InClimateBand(ClimateBand::Hot),
                    tags: &[FormTag::ExponentialChange, FormTag::Polynomial],
                    channels: &[ChannelKind::InfraredThermal],
                },
                // Flood zone: significant standing water (water_depth
                // > 5 m). Distinct from `surface_water` (which fires
                // on any water depth); flood is the dramatic /
                // habitable-disrupting form. Acoustic + tactile +
                // visual + chemical: most species can sense water.
                RecognitionTemplate {
                    id: 8,
                    name: "flood_zone",
                    signature: Signature::Above(Field::WaterDepth, Real::from_int(5)),
                    tags: &[FormTag::Threshold, FormTag::DistanceDecay],
                    channels: &[
                        ChannelKind::AcousticWater,
                        ChannelKind::Tactile,
                        ChannelKind::VisualLight,
                        ChannelKind::ChemicalTaste,
                    ],
                },
                // Cold zone: deep cold relative to planet
                // (T < mean - gradient). A 380 K desert civ's "cold
                // zone" sits at ~340 K — its own cold, not Earth's.
                RecognitionTemplate {
                    id: 9,
                    name: "cold_zone",
                    signature: Signature::InClimateBand(ClimateBand::DeepCold),
                    tags: &[FormTag::Threshold, FormTag::ExponentialChange],
                    channels: &[ChannelKind::Tactile, ChannelKind::InfraredThermal],
                },
                // Fertile land: high biomass density on a dry cell.
                // Multi-condition (`All`) signature mirrors fire's
                // structure: fuel + dry → fertile habitat. Visual
                // species spot vegetation; chemical species smell
                // it; tactile species feel growth.
                // Auroral activity: strong charge + visible-light
                // signature. Fires in cells where planetary
                // electromagnetic activity is doing something dramatic
                // — the night-sky displays of magnetosphere-rich
                // worlds. Distinct from `lightning_buildup` (a
                // pre-discharge precursor); this is the sustained
                // visible glow.
                RecognitionTemplate {
                    id: 11,
                    name: "auroral_activity",
                    signature: Signature::AbsAbove(Field::Charge, Real::from_int(15)),
                    tags: &[FormTag::Threshold, FormTag::Periodic],
                    channels: &[
                        ChannelKind::VisualLight,
                        ChannelKind::ElectricField,
                        ChannelKind::MagneticSense,
                        ChannelKind::RadioNative,
                    ],
                },
                // Harmonic resonance: dense atmosphere + temperature
                // band that supports acoustic oscillation modes.
                // The "field-and-resonance" archetype's bread and
                // butter — fires on worlds whose atmosphere carries
                // sound efficiently. Approximated here as a
                // temperature-band signature; a future pass could
                // refine with a per-cell pressure check once that's
                // available.
                // Productive band relative to planet (mean
                // ± gradient/2) so cold or hot worlds still get
                // their resonance band, just at their own
                // calibration.
                RecognitionTemplate {
                    id: 12,
                    name: "harmonic_resonance",
                    signature: Signature::InClimateBand(ClimateBand::ProductiveBand),
                    tags: &[FormTag::Periodic, FormTag::Polynomial],
                    channels: &[
                        ChannelKind::AcousticAir,
                        ChannelKind::AcousticWater,
                        ChannelKind::Tactile,
                        ChannelKind::Seismic,
                    ],
                },
                // Static field gradient: moderate-charge cells (below
                // lightning, above quiescent). The everyday
                // electromagnetic background that distributed-
                // nervous-system / electric-field-sensing species
                // perceive directly per the "field-and-resonance"
                // archetype.
                RecognitionTemplate {
                    id: 13,
                    name: "static_field_gradient",
                    signature: Signature::AbsAbove(Field::Charge, Real::from_int(5)),
                    tags: &[FormTag::Threshold, FormTag::DistanceDecay],
                    channels: &[
                        ChannelKind::ElectricField,
                        ChannelKind::MagneticSense,
                        ChannelKind::Tactile,
                    ],
                },
                // Tidal extremum: deep water cells. The sim's
                // physics doesn't directly model lunar gravity
                // tides yet (deferred); this signature
                // approximates "tidal" by firing on cells where
                // water_depth exceeds 200 m — i.e., the deep-
                // water belts that on Earth carry the most
                // pronounced tidal swing. A future Q-driven
                // SWE-momentum pass would replace this with a
                // moon_count-modulated gravity-coupled signal.
                RecognitionTemplate {
                    id: 14,
                    name: "tidal_extremum",
                    signature: Signature::Above(Field::WaterDepth, Real::from_int(200)),
                    tags: &[FormTag::Periodic, FormTag::Threshold],
                    channels: &[
                        ChannelKind::AcousticWater,
                        ChannelKind::Tactile,
                        ChannelKind::VisualLight,
                        ChannelKind::Seismic,
                    ],
                },
                RecognitionTemplate {
                    id: 10,
                    name: "fertile_land",
                    signature: Signature::All(vec![
                        Signature::Above(
                            Field::Substance(Substance::Fuel),
                            Real::from_ratio(3, 10),
                        ),
                        Signature::Below(Field::WaterDepth, Real::percent(1)),
                    ]),
                    tags: &[FormTag::Threshold, FormTag::Logistic],
                    channels: &[
                        ChannelKind::VisualLight,
                        ChannelKind::ChemicalPheromone,
                        ChannelKind::ChemicalTaste,
                        ChannelKind::Tactile,
                    ],
                },
                // ============================================
                // Planet-archetype-specific templates.
                // Signatures encode the conditions that only
                // co-occur on the right archetypes; no explicit
                // planet gating needed.
                // ============================================
                // Cryovolcanism — sub-surface ocean signature.
                // Ice-rich cell + significant charge + cold:
                // pressure-driven cryomagma erupting through
                // an ice crust. Europa / Enceladus phenomenon.
                RecognitionTemplate {
                    id: 15,
                    name: "cryovolcanism",
                    signature: Signature::All(vec![
                        Signature::Above(Field::Substance(Substance::Ice), Real::from_ratio(1, 2)),
                        Signature::Below(Field::Temperature, Real::from_int(240)),
                    ]),
                    tags: &[FormTag::Threshold, FormTag::ExponentialChange],
                    channels: &[
                        ChannelKind::VisualLight,
                        ChannelKind::Tactile,
                        ChannelKind::Seismic,
                        ChannelKind::InfraredThermal,
                    ],
                },
                // Ice quake — fracture in an ice shell. Mostly
                // ice + cold (thermal-stress driven, not
                // electrical). Sub-surface-ocean signature.
                RecognitionTemplate {
                    id: 16,
                    name: "ice_quake",
                    signature: Signature::All(vec![
                        Signature::Above(Field::Substance(Substance::Ice), Real::from_ratio(7, 10)),
                        Signature::Below(Field::Temperature, Real::from_int(230)),
                    ]),
                    tags: &[FormTag::Threshold, FormTag::Polynomial],
                    channels: &[
                        ChannelKind::Seismic,
                        ChannelKind::Tactile,
                        ChannelKind::AcousticAir,
                    ],
                },
                // Pressure storm — gaseous shell phenomenon.
                // Atmospheric vapour density extreme + high
                // temperature: Jupiter-style giant storm cells.
                RecognitionTemplate {
                    id: 17,
                    name: "pressure_storm",
                    signature: Signature::All(vec![
                        Signature::Above(Field::Substance(Substance::Vapour), Real::from_int(2)),
                        Signature::Above(Field::Temperature, Real::from_int(330)),
                    ]),
                    tags: &[FormTag::Periodic, FormTag::ExponentialChange],
                    channels: &[
                        ChannelKind::VisualLight,
                        ChannelKind::AcousticAir,
                        ChannelKind::Tactile,
                        ChannelKind::ElectricField,
                    ],
                },
                // Metallic hydrogen signal — gas-giant deep-
                // column signature. Charge threshold (14) is
                // low enough that even a weakly-magnetised
                // GaseousShell qualifies; temperature > 600K
                // and vapour > 4 are the GaseousShell-exclusive
                // gates (no rocky / ocean / sub-surface planet
                // reaches either). Distinct from pressure_storm
                // which fires for any vapour-rich, warm cell.
                RecognitionTemplate {
                    id: 18,
                    name: "metallic_hydrogen_signal",
                    signature: Signature::All(vec![
                        Signature::AbsAbove(Field::Charge, Real::from_int(14)),
                        Signature::Above(Field::Temperature, Real::from_int(600)),
                        Signature::Above(Field::Substance(Substance::Vapour), Real::from_int(4)),
                    ]),
                    tags: &[FormTag::PowerOrLog, FormTag::ExponentialChange],
                    channels: &[
                        ChannelKind::ElectricField,
                        ChannelKind::MagneticSense,
                        ChannelKind::RadioNative,
                    ],
                },
                // Piezoelectric pulse — moderate charge in dry,
                // fuel-rich land cells. Mechanical-stress-driven
                // crystal-bed signal characteristic of
                // piezoelectric crusts.
                RecognitionTemplate {
                    id: 19,
                    name: "piezoelectric_pulse",
                    signature: Signature::All(vec![
                        Signature::AbsAbove(Field::Charge, Real::from_int(8)),
                        Signature::Below(Field::Charge, Real::from_int(40)),
                        Signature::Below(Field::WaterDepth, Real::percent(1)),
                        Signature::Above(
                            Field::Substance(Substance::Fuel),
                            Real::from_ratio(2, 10),
                        ),
                    ]),
                    tags: &[FormTag::Periodic, FormTag::Polynomial],
                    channels: &[
                        ChannelKind::ElectricField,
                        ChannelKind::Tactile,
                        ChannelKind::Seismic,
                    ],
                },
                // Magnetic lodestone — a cell that holds
                // sustained moderate charge on dry land. Iron-
                // rich crust signature; species develop
                // navigation around these.
                RecognitionTemplate {
                    id: 20,
                    name: "magnetic_lodestone",
                    signature: Signature::All(vec![
                        Signature::AbsAbove(Field::Charge, Real::from_int(10)),
                        Signature::Below(Field::Charge, Real::from_int(20)),
                        Signature::Below(Field::WaterDepth, Real::percent(1)),
                    ]),
                    tags: &[FormTag::DistanceDecay, FormTag::PowerOrLog],
                    channels: &[ChannelKind::MagneticSense, ChannelKind::Tactile],
                },
                // Hydrocarbon seep — buried fossil deposits.
                // Reads `Substance::Fossil` directly; only
                // `Crust::Hydrocarbon` worldgen seeds it (4.0 on
                // land cells, 0.0 elsewhere), so any non-zero
                // fossil density distinguishes hydrocarbon crust
                // from every other crust archetype.
                RecognitionTemplate {
                    id: 21,
                    name: "hydrocarbon_seep",
                    signature: Signature::All(vec![
                        Signature::Above(Field::Substance(Substance::Fossil), Real::ZERO),
                        Signature::Above(Field::Substance(Substance::Vapour), Real::percent(1)),
                    ]),
                    tags: &[FormTag::Logistic, FormTag::Threshold],
                    channels: &[
                        ChannelKind::ChemicalTaste,
                        ChannelKind::ChemicalPheromone,
                        ChannelKind::VisualLight,
                    ],
                },
                // Superconductor resonance — cold + RareEarth-
                // band charge. Window is (5, 10): RareEarth crust
                // baseline is 6, with magnetosphere adding 0/1/3
                // (None/Weak/Strong) the imprint lands at 6/7/9 —
                // all inside the window. Hydrocarbon and Basaltic
                // baselines (0–5) stay below the floor;
                // Piezoelectric (12) and Ferrous (15) sit above
                // the ceiling; the discrimination the catalogue
                // depends on holds. Temperature gate < 250 K
                // means it only fires on cold-end planets.
                // Pinned by sim_world's
                // imprints_satisfy_discharge_and_template_invariants.
                // Cold band relative to planet, not absolute
                // 250 K. Cold-end planets fire it on their own
                // poles; warm planets simply never reach the deep-
                // cold band of their own gradient.
                RecognitionTemplate {
                    id: 22,
                    name: "superconductor_resonance",
                    signature: Signature::All(vec![
                        Signature::InClimateBand(ClimateBand::DeepCold),
                        Signature::AbsAbove(Field::Charge, Real::from_int(5)),
                        Signature::Below(Field::Charge, Real::from_int(10)),
                    ]),
                    tags: &[FormTag::Periodic, FormTag::PowerOrLog],
                    channels: &[
                        ChannelKind::ElectricField,
                        ChannelKind::MagneticSense,
                        ChannelKind::RadioNative,
                    ],
                },
                // Reducing storm — Titan-style methane weather.
                // High vapour + warm + low oxidiser. Warm
                // is relative to planet (Hot band), not Earth's
                // 290 K, so cold reducing-atmosphere worlds still
                // see their own storms.
                RecognitionTemplate {
                    id: 23,
                    name: "reducing_storm",
                    signature: Signature::All(vec![
                        Signature::Above(
                            Field::Substance(Substance::Vapour),
                            Real::from_ratio(8, 10),
                        ),
                        Signature::InClimateBand(ClimateBand::Hot),
                        Signature::Below(
                            Field::Substance(Substance::Oxidiser),
                            Real::from_ratio(1, 10),
                        ),
                    ]),
                    tags: &[FormTag::Periodic, FormTag::Threshold],
                    channels: &[
                        ChannelKind::ChemicalTaste,
                        ChannelKind::VisualLight,
                        ChannelKind::AcousticAir,
                        ChannelKind::Tactile,
                    ],
                },
                // Hazy obscuration — very high atmospheric
                // vapour at moderate temperatures. Visual
                // limitation phenomenon — species observe
                // limited sight ranges.
                // Productive band relative to planet, not
                // absolute 260-310 K Earth band.
                RecognitionTemplate {
                    id: 24,
                    name: "hazy_obscuration",
                    signature: Signature::All(vec![
                        Signature::Above(Field::Substance(Substance::Vapour), Real::from_int(1)),
                        Signature::InClimateBand(ClimateBand::ProductiveBand),
                    ]),
                    tags: &[FormTag::Logistic, FormTag::DistanceDecay],
                    channels: &[ChannelKind::VisualLight, ChannelKind::ChemicalTaste],
                },
                // ============================================
                // Seasonal templates. Combine the
                // `MonthIn` signature with existing physics
                // signatures so they fire only during the right
                // months of the year.
                // ============================================
                // Seasonal thaw — spring melt. Months 2-4
                // (March-May), warming temperatures.
                RecognitionTemplate {
                    id: 25,
                    name: "seasonal_thaw",
                    signature: Signature::All(vec![
                        Signature::MonthIn(2, 4),
                        Signature::Above(Field::Temperature, Real::from_int(273)),
                        Signature::Below(Field::Temperature, Real::from_int(290)),
                    ]),
                    tags: &[FormTag::Periodic, FormTag::Threshold],
                    channels: &[
                        ChannelKind::VisualLight,
                        ChannelKind::Tactile,
                        ChannelKind::InfraredThermal,
                        ChannelKind::ChemicalTaste,
                    ],
                },
                // Polar winter — deep cold during winter months.
                // Hemisphere-scoped: northern winter is Nov/Dec/Jan
                // (months 11/0/1, wrapping the year boundary);
                // southern winter is May/Jun/Jul (months 5-7). The
                // signature fires per-cell so a cell only counts as
                // wintering when both its hemisphere AND the
                // calendar align — otherwise both halves of the
                // planet would dim simultaneously, which is wrong.
                RecognitionTemplate {
                    id: 26,
                    name: "polar_winter",
                    signature: Signature::All(vec![
                        // Cold band relative to planet — not
                        // absolute 240 K — so a 200 K sub-surface
                        // ocean's polar winter still fires.
                        Signature::InClimateBand(ClimateBand::Cold),
                        Signature::Any(vec![
                            Signature::All(vec![
                                Signature::Hemisphere(Hemisphere::Northern),
                                Signature::MonthIn(11, 1),
                            ]),
                            Signature::All(vec![
                                Signature::Hemisphere(Hemisphere::Southern),
                                Signature::MonthIn(5, 7),
                            ]),
                        ]),
                    ]),
                    tags: &[FormTag::Periodic, FormTag::Threshold],
                    channels: &[
                        ChannelKind::Tactile,
                        ChannelKind::InfraredThermal,
                        ChannelKind::VisualLight,
                    ],
                },
                // Equatorial wet — monsoon-style standing water
                // during summer months 5-9.
                RecognitionTemplate {
                    id: 27,
                    name: "equatorial_wet",
                    signature: Signature::All(vec![
                        Signature::MonthIn(5, 9),
                        Signature::Above(Field::WaterDepth, Real::from_ratio(5, 10)),
                        Signature::Below(Field::WaterDepth, Real::from_int(3)),
                    ]),
                    tags: &[FormTag::Periodic, FormTag::Threshold],
                    channels: &[
                        ChannelKind::VisualLight,
                        ChannelKind::Tactile,
                        ChannelKind::AcousticWater,
                        ChannelKind::ChemicalTaste,
                    ],
                },
                // Axial extremum — solstice peak. Fires only
                // in months 0 and 6 when temperature is at an
                // extreme. The most pronounced "the year has
                // a shape" signal — visible only on tilted
                // planets.
                RecognitionTemplate {
                    id: 28,
                    name: "axial_extremum",
                    signature: Signature::All(vec![
                        Signature::MonthIn(0, 0),
                        // Deep cold relative to planet's own
                        // mean - gradient, not Earth's 250 K.
                        Signature::InClimateBand(ClimateBand::DeepCold),
                    ]),
                    tags: &[FormTag::Periodic, FormTag::Threshold],
                    channels: &[
                        ChannelKind::Tactile,
                        ChannelKind::InfraredThermal,
                        ChannelKind::VisualLight,
                    ],
                },
                // ============================================
                // Substrate-specific recognition templates.
                // The original catalog was Aqueous-centric (water
                // freeze/boil-derived); the substrate sampler adds
                // non-Aqueous substrates, and these templates give
                // those civs phenomena their species can
                // genuinely perceive on their own chemistry.
                // ============================================
                // Silicate resonance — silicon-substrate worlds at
                // their own hot productive band, with the high
                // charge baseline that Piezoelectric / RareEarth
                // crusts produce on silicate planets. The civ
                // observes crystalline-lattice resonance, the
                // dominant phenomenon for silicon-substrate life.
                RecognitionTemplate {
                    id: 29,
                    name: "silicate_resonance",
                    signature: Signature::All(vec![
                        Signature::Above(Field::Temperature, Real::from_int(800)),
                        Signature::Below(Field::Temperature, Real::from_int(1500)),
                        Signature::AbsAbove(Field::Charge, Real::from_int(5)),
                    ]),
                    tags: &[FormTag::Periodic, FormTag::PowerOrLog],
                    channels: &[
                        ChannelKind::Tactile,
                        ChannelKind::Seismic,
                        ChannelKind::ElectricField,
                        ChannelKind::RadioNative,
                    ],
                },
                // Methane seep — Hydrocarbon-substrate worlds at
                // their cold productive band with high fuel load.
                // Liquid methane / ethane bubbling out of crustal
                // hydrocarbon deposits — the substrate-equivalent
                // of the Aqueous `hydrocarbon_seep` template.
                RecognitionTemplate {
                    id: 30,
                    name: "methane_seep",
                    signature: Signature::All(vec![
                        Signature::Above(Field::Temperature, Real::from_int(90)),
                        Signature::Below(Field::Temperature, Real::from_int(180)),
                        Signature::Above(Field::Substance(Substance::Fuel), Real::from_int(2)),
                    ]),
                    tags: &[FormTag::Logistic, FormTag::Threshold],
                    channels: &[
                        ChannelKind::ChemicalTaste,
                        ChannelKind::ChemicalPheromone,
                        ChannelKind::AcousticAir,
                    ],
                },
                // Ammoniacal storm — Ammoniacal-substrate worlds.
                // Ammonia weather analogous to Aqueous water-cycle
                // storms. Cold + reducing-atmosphere vapour load.
                RecognitionTemplate {
                    id: 31,
                    name: "ammoniacal_storm",
                    signature: Signature::All(vec![
                        Signature::Above(Field::Temperature, Real::from_int(195)),
                        Signature::Below(Field::Temperature, Real::from_int(240)),
                        Signature::Above(
                            Field::Substance(Substance::Vapour),
                            Real::from_ratio(5, 10),
                        ),
                    ]),
                    tags: &[FormTag::Periodic, FormTag::Threshold],
                    channels: &[
                        ChannelKind::ChemicalTaste,
                        ChannelKind::AcousticAir,
                        ChannelKind::Tactile,
                    ],
                },
                // Tidally-locked terminator. Tidally-locked
                // worlds have one face perpetually toward their star;
                // civilisations cluster at the terminator band where
                // temperature is mild between perpetual day (hot) and
                // perpetual night (cold). The signature requires the
                // planet to be tidally locked AND the cell to sit in
                // the terminator longitude band — non-tidally-locked
                // planets never match.
                RecognitionTemplate {
                    id: 32,
                    name: "tidally_locked_terminator",
                    signature: Signature::All(vec![
                        Signature::TidallyLockedTerminator,
                        Signature::InClimateBand(ClimateBand::ProductiveBand),
                    ]),
                    tags: &[FormTag::Threshold, FormTag::DistanceDecay],
                    channels: &[
                        ChannelKind::VisualLight,
                        ChannelKind::InfraredThermal,
                        ChannelKind::Tactile,
                    ],
                },
                // Hydrocarbon-substrate cryogenic lakes.
                // Liquid-methane standing water at substrate's
                // liquid range (90-180 K). Titan's methane lakes
                // are the real-world analog — civs perceive them
                // via acoustic-water (waves), tactile (cold liquid),
                // and chemical-taste (methane).
                RecognitionTemplate {
                    id: 33,
                    name: "cryo_lake",
                    signature: Signature::All(vec![
                        Signature::Above(Field::Temperature, Real::from_int(90)),
                        Signature::Below(Field::Temperature, Real::from_int(180)),
                        Signature::Above(Field::WaterDepth, Real::from_ratio(5, 10)),
                    ]),
                    tags: &[FormTag::Threshold, FormTag::DistanceDecay],
                    channels: &[
                        ChannelKind::AcousticWater,
                        ChannelKind::Tactile,
                        ChannelKind::ChemicalTaste,
                    ],
                },
                // Silicate-substrate crystal growth. Hot
                // silicon-rich cells with sustained charge build
                // ordered crystalline structures. Earth's
                // crystalline-electronics analog at 1000 K+.
                // Distinct from `silicate_resonance` (id 29) which
                // is a vibrational signal; `crystal_growth` is the
                // structural signal driving silicon-substrate
                // biology.
                RecognitionTemplate {
                    id: 34,
                    name: "crystal_growth",
                    signature: Signature::All(vec![
                        Signature::Above(Field::Temperature, Real::from_int(800)),
                        Signature::Below(Field::Temperature, Real::from_int(1_500)),
                        Signature::AbsAbove(Field::Charge, Real::from_int(8)),
                    ]),
                    tags: &[FormTag::Logistic, FormTag::Polynomial],
                    channels: &[
                        ChannelKind::Tactile,
                        ChannelKind::Seismic,
                        ChannelKind::ElectricField,
                    ],
                },
                // Polar aurora — high-charge cells in cold
                // climate bands. Fires on any substrate where the
                // magnetosphere drives auroral ionisation in cold
                // polar latitudes. Distinct from the Aqueous-era
                // `auroral_activity` (id 11) which keys off charge
                // alone; this one requires the cold band so it
                // reads as a *polar* phenomenon rather than just
                // any high-charge cell.
                RecognitionTemplate {
                    id: 35,
                    name: "aurora_polar",
                    signature: Signature::All(vec![
                        Signature::AbsAbove(Field::Charge, Real::from_int(10)),
                        Signature::InClimateBand(ClimateBand::Cold),
                    ]),
                    tags: &[FormTag::Periodic, FormTag::Threshold],
                    channels: &[
                        ChannelKind::VisualLight,
                        ChannelKind::ElectricField,
                        ChannelKind::MagneticSense,
                        ChannelKind::RadioNative,
                    ],
                },
                // Substrate-neutral solvent-cycle templates. An
                // earlier version named these after Earth-water
                // equivalents (`tropical_moist`, `dry_zone`,
                // `storm_cell`); the current names describe the
                // *solvent* (water on Earth, methane
                // on Titan, ammonia on cold-Reducing worlds, etc.)
                // so civs on any substrate correctly label their
                // observations. The signatures read
                // `Field::Substance(Vapour)` and `WaterDepth` —
                // both of which are solvent-agnostic in our model
                // (`Vapour` is whatever phase-changes from the
                // planet's solvent; `WaterDepth` is the surface
                // column of that solvent regardless of chemistry).
                //
                // solvent_humid_band — high vapour density in
                // the Hot climate band. Recognisable via
                // chemical-taste, visual light (haze), tactile
                // (humidity).
                RecognitionTemplate {
                    id: 36,
                    name: "solvent_humid_band",
                    signature: Signature::All(vec![
                        Signature::Above(Field::Substance(Substance::Vapour), Real::from_int(5)),
                        Signature::InClimateBand(ClimateBand::Hot),
                    ]),
                    tags: &[FormTag::Threshold, FormTag::Polynomial],
                    channels: &[
                        ChannelKind::ChemicalTaste,
                        ChannelKind::VisualLight,
                        ChannelKind::Tactile,
                    ],
                },
                // desiccated_band — arid mid-latitude cells. Low
                // surface solvent and low atmospheric vapour in
                // the productive band. Captures rain shadows,
                // continental interiors, trade-wind deserts on
                // any substrate.
                RecognitionTemplate {
                    id: 37,
                    name: "desiccated_band",
                    signature: Signature::All(vec![
                        Signature::Below(Field::WaterDepth, Real::from_ratio(1, 10)),
                        Signature::Below(Field::Substance(Substance::Vapour), Real::ONE),
                        Signature::InClimateBand(ClimateBand::ProductiveBand),
                    ]),
                    tags: &[FormTag::Threshold, FormTag::DistanceDecay],
                    channels: &[
                        ChannelKind::Tactile,
                        ChannelKind::VisualLight,
                        ChannelKind::ChemicalTaste,
                    ],
                },
                // condensation_storm — high vapour density in
                // cool / cold cells where the solvent cloud is
                // actively dropping liquid. Recognisable via
                // electric field (cloud charge separation),
                // acoustic (thunder analogue), visual light
                // (lightning analogue).
                RecognitionTemplate {
                    id: 38,
                    name: "condensation_storm",
                    signature: Signature::All(vec![
                        Signature::Above(Field::Substance(Substance::Vapour), Real::from_int(10)),
                        Signature::InClimateBand(ClimateBand::Cold),
                    ]),
                    tags: &[FormTag::Periodic, FormTag::ExponentialChange],
                    channels: &[
                        ChannelKind::ElectricField,
                        ChannelKind::AcousticAir,
                        ChannelKind::VisualLight,
                    ],
                },
                // windy_strait — fast-moving air. Pure-physics
                // template (`WindMagnitude > 0.5`); already
                // substrate-neutral. Captures jet streams,
                // mountain passes, pressure-gradient gales on
                // any atmosphere-bearing world.
                RecognitionTemplate {
                    id: 39,
                    name: "windy_strait",
                    signature: Signature::Above(Field::WindMagnitude, Real::from_ratio(5, 10)),
                    tags: &[FormTag::Threshold, FormTag::Polynomial],
                    channels: &[ChannelKind::Tactile, ChannelKind::AcousticAir],
                },
                // ============================================
                // Per-substrate surface-solvent templates (Sprint 2
                // Item 8). The original `surface_water` (id 5) is
                // retained for legacy compatibility; these new
                // templates fire on `Field::WaterDepth` (a
                // solvent-agnostic measure of standing surface
                // liquid) with substrate-appropriate `Above`
                // thresholds:
                //   - water (id 50): standard 1 m floor
                //   - ammonia (id 51): same 1 m floor; ammonia is
                //     about as fluid as water at its own range
                //   - methane (id 52): shallow lakes — 0.3 m
                //     (Titan's `lakes' are typically < 1 m on
                //     average)
                //   - silicate melt (id 53): magma ponds are
                //     thin sheets — 0.2 m floor, and the cell
                //     must be hot enough for silicate to be
                //     liquid (> 1687 K) so warm-water cells on
                //     Earth-like planets never accidentally fire
                //     this template.
                RecognitionTemplate {
                    id: 50,
                    name: "surface_solvent_water",
                    signature: Signature::Above(Field::WaterDepth, Real::ONE),
                    tags: &[FormTag::Threshold, FormTag::Polynomial],
                    channels: &[ChannelKind::VisualLight, ChannelKind::Tactile],
                },
                RecognitionTemplate {
                    id: 51,
                    name: "surface_solvent_ammonia",
                    signature: Signature::Above(Field::WaterDepth, Real::ONE),
                    tags: &[FormTag::Threshold, FormTag::Polynomial],
                    channels: &[
                        ChannelKind::ChemicalTaste,
                        ChannelKind::Tactile,
                        ChannelKind::AcousticAir,
                    ],
                },
                RecognitionTemplate {
                    id: 52,
                    name: "surface_solvent_methane",
                    signature: Signature::Above(Field::WaterDepth, Real::from_ratio(3, 10)),
                    tags: &[FormTag::Threshold, FormTag::Polynomial],
                    channels: &[
                        ChannelKind::AcousticWater,
                        ChannelKind::Tactile,
                        ChannelKind::ChemicalTaste,
                    ],
                },
                RecognitionTemplate {
                    id: 53,
                    name: "surface_solvent_silicate_melt",
                    // Silicate melt is only liquid at silicate
                    // freeze (1687 K) and above; combine the
                    // depth floor with a temperature gate so
                    // earth-like cells with standing water can
                    // never accidentally trip this template.
                    signature: Signature::All(vec![
                        Signature::Above(Field::WaterDepth, Real::from_ratio(2, 10)),
                        Signature::Above(Field::Temperature, Real::from_int(1_687)),
                    ]),
                    tags: &[FormTag::Threshold, FormTag::Polynomial],
                    channels: &[
                        ChannelKind::Tactile,
                        ChannelKind::Seismic,
                        ChannelKind::VisualLight,
                    ],
                },
                // ============================================
                // Resonance-field templates. The new per-cell
                // resonance field (`state.resonance()`) surfaces as
                // the "field-and-resonance" archetype's primary
                // substrate signal. Field-sensing species perceive
                // it via the electric / magnetic / radio channels.
                // ============================================
                // Resonance field active — the field is present at
                // an appreciable level. Low floor (1 unit) mirrors
                // the way `magnetic_field_strong` gates on a modest
                // |B| threshold.
                RecognitionTemplate {
                    id: 54,
                    name: "resonance_field_active",
                    signature: Signature::Above(Field::Resonance, Real::from_int(1)),
                    tags: &[FormTag::Threshold],
                    channels: &[
                        ChannelKind::ElectricField,
                        ChannelKind::MagneticSense,
                        ChannelKind::RadioNative,
                    ],
                },
                // Attention coherence — strong resonance, the
                // sustained high-field state field-sensing species
                // read as coherent attention. Higher floor (5
                // units) distinguishes it from the everyday
                // background `resonance_field_active`.
                RecognitionTemplate {
                    id: 55,
                    name: "attention_coherence",
                    signature: Signature::Above(Field::Resonance, Real::from_int(5)),
                    tags: &[FormTag::Threshold],
                    channels: &[
                        ChannelKind::ElectricField,
                        ChannelKind::MagneticSense,
                        ChannelKind::RadioNative,
                    ],
                },
                // ============================================
                // Insolation templates. The diagnostic per-cell
                // stellar-insolation field (`state.insolation()`)
                // surfaces as the photonic archetype's substrate
                // signal. Light-sensing species perceive it via the
                // visual channels.
                // ============================================
                // Daylight present — insolation above a modest floor.
                RecognitionTemplate {
                    id: 56,
                    name: "daylight_strong",
                    signature: Signature::Above(Field::Insolation, Real::from_int(2)),
                    tags: &[FormTag::Threshold],
                    channels: &[ChannelKind::VisualLight, ChannelKind::VisualPolarization],
                },
                // Solar abundance — a brightly-lit cell, the
                // high-irradiance state photonic civilizations build on.
                RecognitionTemplate {
                    id: 57,
                    name: "solar_abundance",
                    signature: Signature::Above(Field::Insolation, Real::from_int(5)),
                    tags: &[FormTag::Threshold],
                    channels: &[ChannelKind::VisualLight, ChannelKind::VisualPolarization],
                },
                // ============================================
                // Tidal-stress templates. The diagnostic per-cell
                // tidal field (`state.tidal_stress()`) surfaces as the
                // gravitational archetype's substrate signal. Ground-
                // and motion-sensing species perceive it.
                // ============================================
                RecognitionTemplate {
                    id: 58,
                    name: "tidal_flexing",
                    signature: Signature::Above(Field::TidalStress, Real::from_int(1)),
                    tags: &[FormTag::Threshold],
                    channels: &[ChannelKind::Seismic],
                },
                RecognitionTemplate {
                    id: 59,
                    name: "strong_tides",
                    signature: Signature::Above(Field::TidalStress, Real::from_int(3)),
                    tags: &[FormTag::Threshold],
                    channels: &[ChannelKind::Seismic],
                },
                // ============================================
                // Surface-radiation templates. The diagnostic per-cell
                // ionizing-radiation field (`state.surface_radiation()`)
                // surfaces as the nuclear archetype's substrate signal.
                // Thermal-sensing biologies perceive its heat signature.
                // ============================================
                RecognitionTemplate {
                    id: 60,
                    name: "radiation_background",
                    signature: Signature::Above(Field::Radiation, Real::from_int(2)),
                    tags: &[FormTag::Threshold],
                    channels: &[ChannelKind::InfraredThermal],
                },
                RecognitionTemplate {
                    id: 61,
                    name: "radiation_hotspot",
                    signature: Signature::Above(Field::Radiation, Real::from_int(8)),
                    tags: &[FormTag::Threshold],
                    channels: &[ChannelKind::InfraredThermal],
                },
            ],
        }
    }
}
