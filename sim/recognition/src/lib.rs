//! `sim-recognition` — pattern recognition layer that turns
//! emergent physics state into named recognized phenomena.
//!
//! Starting position: template-driven signature matching.
//! Each template defines a signature pattern (threshold crossing,
//! co-occurrence of multiple fields, etc.); the recognition layer
//! scans physics state per tick and fires recognised-phenomenon
//! events for each cell where a signature matches.
//!
//! M2 foundation: simple per-cell signatures only (threshold and
//! co-occurrence). Spatial-temporal clustering for multi-cell or
//! multi-tick phenomena (tides as periodic oscillation, weather
//! systems as cell clusters) lands as recognition matures.

#![allow(clippy::module_name_repetitions)]

mod templates;

use sim_arith::transcendental::sqrt;
use sim_arith::Real;
use sim_physics::{CellId, PhysicsState, Substance};

/// A recognized-phenomenon firing — emitted for each cell where a
/// template's signature matches this tick. The id is the
/// template's id; the cell index identifies where it fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Firing {
    pub template_id: u32,
    pub cell: u32,
}

/// Field selector for templates that read a single named field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Temperature,
    Charge,
    WaterDepth,
    Substance(Substance),
    /// Scalar magnitude of the per-cell magnetic vector field
    /// `(B_q, B_r)`. `state.magnetic_field_magnitude(cell)`
    /// derives `sqrt(B_q² + B_r²)` for templates that key on
    /// "compass strength" without caring about direction. Earlier,
    /// the `magnetic_field_strong` template proxied this through
    /// `Field::Charge`, which fired on lightning build-up rather
    /// than on the dipole — the wrong physics for the named
    /// behaviour.
    MagneticMagnitude,
    /// Scalar magnitude of the wind velocity vector
    /// `(v_q, v_r)` — `sqrt(v_q² + v_r²)`. Computed on demand.
    /// Templates that key on "fast wind" / "still air" without
    /// caring about direction read this. Direction-aware
    /// templates (downwind shadow, leeward) need a separate
    /// signature variant once those land.
    WindMagnitude,
    /// Per-cell resonance field (`state.resonance()`). The
    /// "field-and-resonance" archetype's primary substrate signal —
    /// field-sensing species perceive it directly and civ science
    /// fits laws over it.
    Resonance,
}

impl Field {
    fn read(self, state: &PhysicsState, cell: usize) -> Real {
        match self {
            Field::Temperature => state.temperature()[cell],
            Field::Charge => state.charge()[cell],
            Field::WaterDepth => state.water_depth()[cell],
            Field::Substance(s) => state.substance(s.idx())[cell],
            Field::MagneticMagnitude => state.magnetic_field_magnitude(cell),
            Field::WindMagnitude => {
                let (vq, vr) = state.fluid_velocity();
                let q = vq[cell];
                let r = vr[cell];
                sqrt(q * q + r * r)
            }
            Field::Resonance => state.resonance()[cell],
        }
    }
}

/// Northern / southern half of the planet. The grid is row-major
/// `(r, q)` with periodic boundaries; we treat row index relative to
/// `height / 2` as a hemisphere proxy. Used by `Signature::Hemisphere`
/// so seasonal templates that flip phase across the equator (e.g.
/// `polar_winter`) compose cleanly without a per-template hemisphere
/// flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hemisphere {
    Northern,
    Southern,
}

/// Climate-relative band selectors. Templates that name climate
/// zones (productive, polar, hot, deep cold) read these bands from
/// the per-run `PlanetContext` instead of absolute Kelvin values, so
/// a 200 K sub-surface ocean's "polar winter" is colder than its own
/// gradient predicts — not silently never-fires because 200 < 240.
///
/// Bands derive from a normalised offset
/// `o = (T - mean) / gradient` ∈ approximately [-0.5, +0.5] across
/// the planet (the spatial gradient runs `mean ± gradient/2` from
/// equator to pole, plus seasonal swing). Quartering that range:
///
/// - `DeepCold`       → `o ≤ -0.5` (the polar boundary)
/// - `Cold`           → `o < -0.25` (subpolar; includes `DeepCold`)
/// - `ProductiveBand` → `-0.25 ≤ o ≤ 0.25` (mid-latitude band)
/// - `Hot`            → `o > 0.25` (subtropical and equator)
///
/// The normalisation means seasonal templates don't need different
/// thresholds per archetype — every world's polar/temperate/tropical
/// cells map to the same band labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClimateBand {
    DeepCold,
    Cold,
    ProductiveBand,
    Hot,
}

/// Per-run context handed to `RecognitionLibrary::scan`. Carries
/// the planet-derived calibration values that climate-relative
/// signatures and combustion-ignition signatures need. Built once at
/// run start in `sim/core` from the sampled planet; never mutated
/// during the run.
#[derive(Debug, Clone, Copy)]
pub struct PlanetContext {
    pub mean_temperature: Real,
    pub temperature_gradient: Real,
    /// Combustion auto-ignition threshold (planet-derived in
    /// `sim_core::build_laws` from atmosphere richness). Replaces the
    /// Earth-fixed 500 K threshold previously hardcoded in the `fire`
    /// template.
    pub ignition_threshold: Real,
    /// Months per orbital period for this planet.
    /// `Signature::MonthIn` modulos by this — seasonal templates
    /// fire on the planet's actual year length, not on a baseline
    /// 12-month standard. Defaults to 12 in `earth_like()` for unit
    /// tests that don't sample a specific planet.
    pub orbital_period_months: u32,
    /// Tidally-locked planets have one face perpetually toward
    /// their star. The `tidally_locked_terminator` template (id 32)
    /// only fires on planets where this is true.
    pub is_tidally_locked: bool,
}

impl PlanetContext {
    /// Earth-equivalent default for unit tests that don't care about
    /// the planet identity. mean = 288 K (Earth surface), gradient =
    /// 40 K (Earth equator-pole), ignition = 500 K (Earth oxidising).
    #[must_use]
    pub fn earth_like() -> Self {
        Self {
            mean_temperature: Real::from_int(288),
            temperature_gradient: Real::from_int(40),
            ignition_threshold: Real::from_int(500),
            orbital_period_months: 12,
            is_tidally_locked: false,
        }
    }

    fn band_match(self, t: Real, band: ClimateBand) -> bool {
        // Guard against zero-gradient planets (perfectly uniform —
        // no band has meaning). Treat every cell as productive in
        // that degenerate case.
        if self.temperature_gradient <= Real::ZERO {
            return matches!(band, ClimateBand::ProductiveBand);
        }
        let quarter = self.temperature_gradient / Real::from_int(4);
        let lower_warm = self.mean_temperature - quarter;
        let upper_warm = self.mean_temperature + quarter;
        let deep_cold_ceiling =
            self.mean_temperature - self.temperature_gradient / Real::from_int(2);
        match band {
            ClimateBand::DeepCold => t <= deep_cold_ceiling,
            ClimateBand::Cold => t < lower_warm,
            ClimateBand::ProductiveBand => t >= lower_warm && t <= upper_warm,
            ClimateBand::Hot => t > upper_warm,
        }
    }
}

/// Signature shape. Threshold matches when a field crosses a value
/// in the configured direction. `All` matches when all of a list
/// of sub-signatures match — used for multi-field signatures like
/// fire (hot + has fuel + has oxidiser). `Any` matches when at least
/// one sub-signature matches — composes hemisphere-asymmetric
/// seasonal templates (north winter OR south winter). `MonthIn`
/// matches when `tick % MONTHS_PER_YEAR` is in the configured
/// month range (inclusive); supports wrap-around (e.g. `MonthIn(11,
/// 1)` = Nov, Dec, Jan). `Hemisphere` matches cells in the named
/// half of the grid (rows < height/2 are northern).
#[derive(Debug, Clone)]
pub enum Signature {
    Above(Field, Real),
    Below(Field, Real),
    AbsAbove(Field, Real),
    All(Vec<Signature>),
    Any(Vec<Signature>),
    MonthIn(u8, u8),
    Hemisphere(Hemisphere),
    /// Cell temperature is in the named climate band, where the
    /// band is computed relative to `ctx.mean_temperature ± gradient`.
    /// Replaces absolute-Kelvin thresholds (250 K, 260 K, 290 K, 310 K)
    /// for templates that name climate-relative phenomena.
    InClimateBand(ClimateBand),
    /// Cell temperature exceeds the planet's combustion ignition
    /// threshold (atmosphere-derived in `sim_core::build_laws`).
    /// Replaces the Earth-fixed 500 K combustion threshold previously
    /// hardcoded in the `fire` template.
    AboveIgnition,
    /// Cell sits in the tidally-locked terminator band — the
    /// boundary longitude between perpetual day and perpetual night
    /// on a tidally-locked planet. Matches when
    /// `ctx.is_tidally_locked` AND the cell's column index is within
    /// 1 of `width/4` or `3·width/4` (the +/-90° from the sub-solar
    /// point). Non-tidally-locked planets never match.
    TidallyLockedTerminator,
}

impl Signature {
    fn matches(&self, state: &PhysicsState, cell: usize, tick: u64, ctx: &PlanetContext) -> bool {
        match self {
            Signature::Above(field, threshold) => field.read(state, cell) > *threshold,
            Signature::Below(field, threshold) => field.read(state, cell) < *threshold,
            Signature::AbsAbove(field, threshold) => field.read(state, cell).abs() > *threshold,
            Signature::All(subs) => subs.iter().all(|s| s.matches(state, cell, tick, ctx)),
            Signature::Any(subs) => subs.iter().any(|s| s.matches(state, cell, tick, ctx)),
            Signature::MonthIn(start, end) => {
                // Per-planet orbital period drives the modulo
                // so seasonal templates fire on the planet's actual
                // year-fraction (8-16 months range) rather than the
                // universal 12-month standard.
                let period = u64::from(ctx.orbital_period_months.max(1));
                let month = u8::try_from(tick % period).unwrap_or(u8::MAX);
                if start <= end {
                    month >= *start && month <= *end
                } else {
                    // Wraps year boundary, e.g. MonthIn(11, 1) = Nov/Dec/Jan.
                    month >= *start || month <= *end
                }
            }
            Signature::Hemisphere(h) => {
                let grid = state.grid();
                let cell_u32 = u32::try_from(cell).unwrap_or(u32::MAX);
                let row = grid.axial_of(CellId(cell_u32)).r;
                // Row indices run [0, height); the equator sits at
                // height/2. Cells in the upper half are northern.
                let half = i32::try_from(grid.height()).unwrap_or(i32::MAX) / 2;
                let in_north = row < half;
                match h {
                    Hemisphere::Northern => in_north,
                    Hemisphere::Southern => !in_north,
                }
            }
            Signature::InClimateBand(band) => ctx.band_match(state.temperature()[cell], *band),
            Signature::AboveIgnition => state.temperature()[cell] > ctx.ignition_threshold,
            Signature::TidallyLockedTerminator => {
                if !ctx.is_tidally_locked {
                    return false;
                }
                let grid = state.grid();
                let cell_u32 = u32::try_from(cell).unwrap_or(u32::MAX);
                let axial = grid.axial_of(CellId(cell_u32));
                let width = i32::try_from(grid.width()).unwrap_or(i32::MAX);
                let q = axial.q.rem_euclid(width.max(1));
                let quarter = width / 4;
                let three_quarter = (3 * width) / 4;
                let near = |target: i32| (q - target).abs() <= 1;
                near(quarter) || near(three_quarter)
            }
        }
    }
}

/// Structural-tag annotations. Each tag declares the kind of
/// signal structure a template produces; a civ's available form
/// vocabulary is the union of `tag.forms()` over every template the
/// civ currently perceives, plus the always-baseline `Constant` and
/// `Linear`. No authored form-unlock table.
///
/// Tags name *signal structures* rather than specific forms so the
/// downstream `Form` mapping can evolve (e.g. iterative-fit forms
/// landing under the tuning umbrella) without rewriting templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FormTag {
    Threshold,
    Periodic,
    DistanceDecay,
    ExponentialChange,
    Logistic,
    Polynomial,
    PowerOrLog,
}

/// Modality-channel selectors, mirroring `sim_species::ModalityKind`.
/// Duplicated here to keep `sim/recognition` free of a `sim_species`
/// dependency (recognition is upstream of species in the run-start
/// order). Keep in sync with the species-side enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChannelKind {
    AcousticAir,
    AcousticWater,
    Seismic,
    VisualLight,
    VisualPolarization,
    Bioluminescent,
    ChemicalPheromone,
    ChemicalTaste,
    Tactile,
    ElectricField,
    MagneticSense,
    InfraredThermal,
    RadioNative,
    Gestural,
    Postural,
}

#[derive(Debug, Clone)]
pub struct RecognitionTemplate {
    pub id: u32,
    pub name: &'static str,
    pub signature: Signature,
    /// Structural tags. Empty `tags` means the template only
    /// participates in the baseline `{Constant, Linear}` form set.
    pub tags: &'static [FormTag],
    /// The modality channels that natively sense this template.
    /// Mirrors `sim_species::template_channels(id)` and is the
    /// authoritative declaration for sensorium gating; species and
    /// civ-tool channel sets union and intersect against this for
    /// perceivability.
    pub channels: &'static [ChannelKind],
}

/// Emergent recognition templates. The static
/// `RecognitionTemplate` uses `&'static str` slices for zero-cost
/// authored templates; species-discovered templates need owned
/// types because their names + channel sets are computed at run
/// time from observation regularities.
///
/// Discovered templates carry the same `Signature` machinery as
/// authored templates and fire through the same scan pipeline,
/// so downstream code (firings, perceivability gates, civ
/// observation accumulators, `relation_prereqs` lookups,
/// the tool tree, post-run report rendering) treats them
/// uniformly via `template_id`. The discovery rule lives in
/// `sim_civ::discovery::propose_discovered_templates`.
///
/// Discovered ids start at `DISCOVERED_TEMPLATE_ID_START` (1000)
/// to leave the 1-999 range to authored templates without
/// collision risk.
#[derive(Debug, Clone)]
pub struct DiscoveredTemplate {
    pub id: u32,
    pub name: String,
    pub signature: Signature,
    pub tags: Vec<FormTag>,
    pub channels: Vec<ChannelKind>,
    /// Tick at which the discovery fired. Used by the post-run
    /// report to slot the event in the species' timeline.
    pub discovered_at_tick: u64,
    /// Civ that proposed the template. The discovery is species-
    /// level (the template enters the species-shared library), but
    /// recording the proposing civ keeps the biographical link.
    pub discovered_by_civ_id: u32,
    /// The confirmed-relation `(static_template_id, channel_index)`
    /// pair the proposal was extrapolated from. Used by tests +
    /// the report; `static_template_id` may itself reference an
    /// earlier discovered template (chained discoveries).
    pub origin_template_id: u32,
}

/// Id space split between authored and discovered templates.
/// Authored templates occupy the dense low range (currently 1..=39
/// in `earth_like_default`); discovered templates start at 1000 so
/// the split is visible at a glance and there's room for ~960 new
/// authored templates before the ranges meet.
pub const DISCOVERED_TEMPLATE_ID_START: u32 = 1000;

/// The library of authored recognition templates. M2 ships a small
/// initial set; the catalog grows as the physics layer surfaces
/// more recognisable signatures.
#[derive(Debug, Clone)]
pub struct RecognitionLibrary {
    pub templates: Vec<RecognitionTemplate>,
}

// `RecognitionLibrary::earth_like_default` lives in `mod templates`.

impl RecognitionLibrary {
    /// Scan the physics state and collect every cell × template
    /// match. Iteration is template-major then cell-major sorted
    /// for deterministic event emission. `tick`
    /// supplies the month-of-year to seasonal templates via
    /// `Signature::MonthIn`; non-seasonal signatures ignore it.
    /// `ctx` carries the per-planet climate calibration so
    /// `Signature::InClimateBand` and `Signature::AboveIgnition`
    /// fire relative to *this planet*, not Earth.
    pub fn scan(&self, state: &PhysicsState, tick: u64, ctx: &PlanetContext) -> Vec<Firing> {
        self.scan_with_discovered(&[], state, tick, ctx)
    }

    /// Scan extension: also fire any species-discovered
    /// templates supplied by the caller. Discovered templates
    /// produce the same `Firing` records as authored ones, so
    /// downstream consumers (per-cell observation accumulators,
    /// civ hypothesizers, tool-prereq lookups) treat them
    /// uniformly. Iteration order is authored-then-discovered so
    /// firing-event order stays stable as the discovered set
    /// grows mid-run.
    pub fn scan_with_discovered(
        &self,
        discovered: &[DiscoveredTemplate],
        state: &PhysicsState,
        tick: u64,
        ctx: &PlanetContext,
    ) -> Vec<Firing> {
        let n = state.grid().n_cells();
        let mut firings = Vec::new();
        for tmpl in &self.templates {
            for cell in 0..n {
                if tmpl.signature.matches(state, cell, tick, ctx) {
                    firings.push(Firing {
                        template_id: tmpl.id,
                        cell: u32::try_from(cell).expect("cell id fits in u32"),
                    });
                }
            }
        }
        for tmpl in discovered {
            for cell in 0..n {
                if tmpl.signature.matches(state, cell, tick, ctx) {
                    firings.push(Firing {
                        template_id: tmpl.id,
                        cell: u32::try_from(cell).expect("cell id fits in u32"),
                    });
                }
            }
        }
        firings
    }
}

#[cfg(test)]
mod tests;
