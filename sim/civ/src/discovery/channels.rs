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
    /// monotonicity); EM field activity.
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
}

impl Channel {
    /// All channels available to the discovery pipeline. Used for
    /// the cross-product candidate generation (template × channel)
    /// — see `Hypothesizer::candidates_for`.
    pub const ALL: [Channel; 9] = [
        Channel::Temperature,
        Channel::WaterDepth,
        Channel::ChargeMagnitude,
        Channel::Elevation,
        Channel::Fuel,
        Channel::Oxidiser,
        Channel::Vapour,
        Channel::Ice,
        Channel::Fossil,
    ];
}

/// Stable `relation_id` derived from `(template_id, channel)`. Lets
/// the candidate set grow (when sensorium-extending tools widen
/// the perceivable templates) without renumbering existing
/// confirmed relations. `template_id × 16 + channel_discriminant`
/// keeps the channel namespace below 16 (currently 8 used).
pub fn relation_id_for(template_id: u32, channel: Channel) -> u32 {
    template_id * 16 + (channel as u32)
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
