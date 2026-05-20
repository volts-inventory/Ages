//! Discovery-pipeline channel selectors. `Channel` is the firing-
//! relation x-axis (template-channel pair → `relation_id`);
//! `MeasurementChannel` is the measurement-relation y/x with
//! direct / neighbour-mean / Laplacian / temporal-delta variants.

use sim_arith::Real;
use sim_physics::{PhysicsState, Substance};

/// Physical-channel selectors a candidate relation may use as the
/// independent variable. Mirrors the recognition `Field` enum but
/// names the substance-substance channels separately (richer x
/// vocabulary than the recognition layer needs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Channel {
    Temperature = 0,
    WaterDepth = 1,
    /// Magnitude of cell charge (signed → unsigned for fit
    /// monotonicity); ElectricField perception reads charge
    /// gradients. Distinct from `MagneticField` — birds and other
    /// magnetoreceptors sense the dipole `B`, not the local charge.
    ChargeMagnitude = 2,
    Elevation = 3,
    Fuel = 4,
    Oxidiser = 5,
    Vapour = 6,
    Ice = 7,
    /// Buried fossil-hydrocarbon density. Distinct from `Fuel`
    /// (renewable biofuel) — civs that confirm a relation on this
    /// channel are reading the geological deposit, not the
    /// surface biomass. Stays below 16 to fit the `template_id ×
    /// 16 + channel` relation-id encoding.
    Fossil = 8,
    /// Magnitude of the planetary magnetic field `|B|` at the cell
    /// (`PhysicsState::magnetic_magnitude()`). The channel
    /// MagneticSense / RadioNative species actually read —
    /// previously they were mapped onto `ChargeMagnitude`, which
    /// was observationally wrong (magnetoreceptors don't read
    /// electric charge). ElectricField stays on `ChargeMagnitude`.
    /// Sits at discriminant 9; well under the 16-cap from the
    /// `template_id × 16 + channel` relation-id encoding.
    MagneticField = 9,
}

impl Channel {
    /// All channels available to the discovery pipeline. Used for
    /// the cross-product candidate generation (template × channel)
    /// — see `Hypothesizer::candidates_for`.
    pub const ALL: [Channel; 10] = [
        Channel::Temperature,
        Channel::WaterDepth,
        Channel::ChargeMagnitude,
        Channel::Elevation,
        Channel::Fuel,
        Channel::Oxidiser,
        Channel::Vapour,
        Channel::Ice,
        Channel::Fossil,
        Channel::MagneticField,
    ];
}

/// Stable `relation_id` derived from `(template_id, channel)`. Lets
/// the candidate set grow (when sensorium-extending tools widen
/// the perceivable templates) without renumbering existing
/// confirmed relations. `template_id × 16 + channel_discriminant`
/// keeps the channel namespace below 16 (currently 10 used:
/// 0..=9 with `MagneticField` at 9).
pub fn relation_id_for(template_id: u32, channel: Channel) -> u32 {
    template_id * 16 + (channel as u32)
}

/// Which physics channels each species sensory modality grants
/// access to. Biology-grounded mapping:
///
/// - **VisualLight / VisualPolarization** → `Temperature`
///   (thermal-IR emission visible at long wavelengths +
///   incandescence) and `Elevation` (visible terrain). Vegetation
///   reflectance / fire glow are not in the model; mapping `Fuel`
///   onto vision was a stretch and is dropped.
/// - **InfraredThermal** → `Temperature` only. The whole point of
///   thermal IR is reading the blackbody radiance.
/// - **ChemicalTaste / ChemicalPheromone** → `Vapour` + `Oxidiser`.
///   Fuel is bulk biomass — combustion products read as `Oxidiser`
///   depletion + `Vapour`, not as fuel-density-at-distance.
/// - **AcousticAir / AcousticWater / Seismic** → `WaterDepth` +
///   `Elevation` + `Temperature` (sound-speed gradients reveal
///   density / bathymetry / thermal layering).
/// - **ElectricField** → `ChargeMagnitude`. Electroreceptors read
///   charge gradients — this is the right physics.
/// - **MagneticSense / RadioNative** → `MagneticField`. Magneto-
///   receptors read `|B|`, not charge; the previous mapping onto
///   `ChargeMagnitude` was observationally wrong.
/// - **Tactile** → `Temperature` + `Elevation`. Contact-only sense:
///   it tells the cell's solidity / thermal state but not the bulk
///   substance composition at distance. The earlier 5-channel
///   broad-baseline was over-generous — paired with a universal
///   floor it meant tactile-only species saw nearly everything.
/// - **Bioluminescent / Gestural / Postural** → empty. Output-only
///   or communication modalities; no perception contribution.
///
/// Returned slice may be empty for non-perceptual modalities.
/// Callers union across the species' modality list and pair with
/// `perceivable_channels` to derive the per-civ candidate set fed
/// into `Hypothesizer::candidates_for_with_channels`.
#[must_use]
pub fn channels_for_modality(modality: sim_species::ModalityKind) -> &'static [Channel] {
    use sim_species::ModalityKind as MK;
    match modality {
        MK::VisualLight | MK::VisualPolarization => {
            &[Channel::Temperature, Channel::Elevation]
        }
        MK::InfraredThermal => &[Channel::Temperature],
        MK::ChemicalTaste | MK::ChemicalPheromone => {
            &[Channel::Vapour, Channel::Oxidiser]
        }
        MK::AcousticAir | MK::AcousticWater | MK::Seismic => {
            &[Channel::WaterDepth, Channel::Elevation, Channel::Temperature]
        }
        MK::ElectricField => &[Channel::ChargeMagnitude],
        MK::MagneticSense | MK::RadioNative => &[Channel::MagneticField],
        // Tactile is contact-only: it can tell the cell's
        // thermal state and surface relief but not the bulk
        // substance composition at distance. Earlier the fallback
        // listed 5 channels and was paired with a universal floor;
        // a tactile-only species ended up with 6/9 channels and
        // the "restriction" barely restricted.
        MK::Tactile => &[Channel::Temperature, Channel::Elevation],
        // Pure-output / communication modalities — no perception
        // contribution.
        MK::Bioluminescent | MK::Gestural | MK::Postural => &[],
    }
}

/// Union of channels reachable by *any* of the species' sensory
/// modalities. No universal force-include — Fuel + Fossil were
/// previously pinned on every civ regardless of senses, which
/// made the modality mapping moot for the always-present
/// channels and made the (Fuel via fire glow) Visual overlap
/// invisible. If the union is genuinely empty (e.g. a
/// Bioluminescent-only / Gestural-only seed with no perceptual
/// modalities at all), fall back to a minimum-viable contact
/// set — `Temperature` + `Elevation` — the universally-touchable
/// fields any creature with a body can read.
#[must_use]
pub fn perceivable_channels(
    modalities: &[sim_species::Modality],
) -> Vec<Channel> {
    let kinds: Vec<sim_species::ModalityKind> = modalities.iter().map(|m| m.kind).collect();
    perceivable_channels_from_kinds(&kinds)
}

/// `perceivable_channels` for callers that already hold a slice of
/// `ModalityKind` (the bare enum, no per-modality parameters).
/// Civ founding pipelines thread `species_modality_kinds` here so
/// the per-figure `Hypothesizer` gets a sensor-gated candidate
/// set without needing a full `Vec<Modality>`.
#[must_use]
pub fn perceivable_channels_from_kinds(
    modality_kinds: &[sim_species::ModalityKind],
) -> Vec<Channel> {
    let mut set: std::collections::BTreeSet<Channel> = std::collections::BTreeSet::new();
    for kind in modality_kinds {
        for ch in channels_for_modality(*kind) {
            set.insert(*ch);
        }
    }
    if set.is_empty() {
        // Reachable now that the Fuel/Fossil force-include is
        // gone: every species with only output-only modalities
        // (Bioluminescent/Gestural/Postural) lands here.
        return vec![Channel::Temperature, Channel::Elevation];
    }
    set.into_iter().collect()
}

impl Channel {
    /// Per-channel normalisation scale. Sampled `x` values are
    /// divided by this so the fit-module's `Σϕ(x)ϕ(x)ᵀ` accumulator
    /// stays inside Q32.32 range (±~2.1e9) even with hundreds of
    /// samples on wide-range channels (temperature 200–400 K,
    /// elevation 0–15 000 m). Discovered parameters are in
    /// normalised space; the post-run report reverses the scale
    /// when humanising the relation. Match arms enumerated per
    /// channel for readability.
    #[allow(clippy::match_same_arms)]
    pub fn scale(self) -> Real {
        match self {
            Channel::Temperature => Real::from_int(100),
            Channel::WaterDepth => Real::from_int(100),
            Channel::ChargeMagnitude => Real::from_int(10),
            Channel::Elevation => Real::from_int(1000),
            // Planetary `|B|` sits in low single digits in the
            // current magnetism model (`MagnetosphereParams`
            // earth-like equator ~1 unit; pole-peaked enhancement);
            // unit-scale keeps fits inside Q32.32 range.
            Channel::MagneticField => Real::ONE,
            // Substance densities sit in low single digits;
            // unit-scale leaves them unchanged.
            Channel::Fuel
            | Channel::Oxidiser
            | Channel::Vapour
            | Channel::Ice
            | Channel::Fossil => Real::ONE,
        }
    }

    pub fn read(self, state: &PhysicsState, cell: usize) -> Real {
        let raw = match self {
            Channel::Temperature => state.temperature()[cell],
            Channel::WaterDepth => state.water_depth()[cell],
            Channel::ChargeMagnitude => state.charge()[cell].abs(),
            Channel::Elevation => state.elevation()[cell],
            Channel::Fuel => state.substance(Substance::Fuel.idx())[cell],
            Channel::Oxidiser => state.substance(Substance::Oxidiser.idx())[cell],
            Channel::Vapour => state.substance(Substance::Vapour.idx())[cell],
            Channel::Ice => state.substance(Substance::Ice.idx())[cell],
            Channel::Fossil => state.substance(Substance::Fossil.idx())[cell],
            // Cached per-cell `|B|` from the magnetism kernel —
            // `magnetism::Magnetism::init_field` + `step` refresh
            // it after every state-mutating pass. Magnetoreceptor
            // species now read the actual planetary field strength
            // rather than the misappropriated `ChargeMagnitude`.
            Channel::MagneticField => state.magnetic_field_magnitude(cell),
        };
        raw / self.scale()
    }

    pub fn tag(self) -> &'static str {
        match self {
            Channel::Temperature => "temperature",
            Channel::WaterDepth => "water_depth",
            Channel::ChargeMagnitude => "charge_magnitude",
            Channel::Elevation => "elevation",
            Channel::Fuel => "fuel",
            Channel::Oxidiser => "oxidiser",
            Channel::Vapour => "vapour",
            Channel::Ice => "ice",
            Channel::Fossil => "fossil",
            Channel::MagneticField => "magnetic_field",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MeasurementChannel {
    Direct(Channel),
    NeighbourMean(Channel),
    /// `Σ(neighbour value - cell value)` over the 6 axial
    /// neighbours. The standard discrete Laplacian on the hex
    /// grid (up to a constant factor) — fits against
    /// `TemporalDelta(field)` recover the diffusion coefficient.
    Laplacian(Channel),
    /// `current[cell] - previous[cell]` in fit-space. Reads
    /// `None` when no `prev_state` is available (tick 0 or the
    /// first observation after a snapshot reset). Fitting
    /// `TemporalDelta(field) = α × Laplacian(field)` recovers the
    /// diffusion-law coefficient.
    TemporalDelta(Channel),
}

impl MeasurementChannel {
    /// Read the measurement's value at `cell` in the same fit-space
    /// `Channel::read` uses (raw / `Channel::scale`). Bit-exact and
    /// deterministic. Returns `None` for `TemporalDelta` when no
    /// `prev_state` is available; otherwise always `Some`.
    pub fn read(
        self,
        state: &PhysicsState,
        prev_state: Option<&PhysicsState>,
        cell: usize,
    ) -> Option<Real> {
        match self {
            MeasurementChannel::Direct(ch) => Some(ch.read(state, cell)),
            MeasurementChannel::NeighbourMean(ch) => {
                let grid = state.grid();
                let cell_u32 = u32::try_from(cell).unwrap_or(u32::MAX);
                let axial = grid.axial_of(sim_physics::CellId(cell_u32));
                let mut sum = Real::ZERO;
                for nb in grid.neighbours(axial) {
                    sum = sum + ch.read(state, nb.0 as usize);
                }
                Some(sum / Real::from_int(6))
            }
            MeasurementChannel::Laplacian(ch) => {
                let grid = state.grid();
                let cell_u32 = u32::try_from(cell).unwrap_or(u32::MAX);
                let axial = grid.axial_of(sim_physics::CellId(cell_u32));
                let self_v = ch.read(state, cell);
                let mut sum = Real::ZERO;
                for nb in grid.neighbours(axial) {
                    sum = sum + (ch.read(state, nb.0 as usize) - self_v);
                }
                Some(sum)
            }
            MeasurementChannel::TemporalDelta(ch) => {
                let prev = prev_state?;
                Some(ch.read(state, cell) - ch.read(prev, cell))
            }
        }
    }

    /// The underlying physics channel — used for SI rescaling on
    /// emit so reported coefficients land in real units.
    pub fn channel(self) -> Channel {
        match self {
            MeasurementChannel::Direct(ch)
            | MeasurementChannel::NeighbourMean(ch)
            | MeasurementChannel::Laplacian(ch)
            | MeasurementChannel::TemporalDelta(ch) => ch,
        }
    }

    /// Snake-case tag for the protocol event payload.
    pub fn tag(self) -> String {
        match self {
            MeasurementChannel::Direct(ch) => ch.tag().to_string(),
            MeasurementChannel::NeighbourMean(ch) => format!("neighbour_mean_{}", ch.tag()),
            MeasurementChannel::Laplacian(ch) => format!("laplacian_{}", ch.tag()),
            MeasurementChannel::TemporalDelta(ch) => format!("delta_{}", ch.tag()),
        }
    }

    fn discriminant(self) -> u32 {
        let kind = match self {
            MeasurementChannel::Direct(_) => 0,
            MeasurementChannel::NeighbourMean(_) => 1,
            MeasurementChannel::Laplacian(_) => 2,
            MeasurementChannel::TemporalDelta(_) => 3,
        };
        kind * 16 + (self.channel() as u32)
    }
}

/// Stable id for a measurement relation. Disjoint from firing-
/// relation ids (`template_id × 16 + channel`, max ~500) so the two
/// catalogues coexist in the same `relation_id` namespace.
pub fn measurement_relation_id(y: MeasurementChannel, x: MeasurementChannel) -> u32 {
    1_000_000 + y.discriminant() * 256 + x.discriminant()
}
