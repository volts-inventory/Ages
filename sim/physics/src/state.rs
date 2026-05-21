//! Per-cell physics state.
//!
//! Storage is array-of-arrays (one Vec per field) rather than
//! array-of-structs, for cache friendliness and so each integrator
//! can iterate just the fields it needs without walking unrelated
//! ones.
//!
//! M1a fields: temperature, pressure, fluid velocity (q, r
//! components in axial coordinates). Mechanics doesn't add a field
//! — it acts as gravitational forcing on fluid velocity each
//! sub-step. M1b adds charge and per-substance density vectors.

use crate::grid::{CellId, HexGrid};
use crate::magnetism::DipoleState;
use sim_arith::Real;

/// A view of a single cell's M1a state. Returned by lookups; not the
/// canonical storage.
#[derive(Debug, Clone, Copy)]
pub struct Cell {
    pub elevation: Real,
    pub water_depth: Real,
    pub temperature: Real,
    pub pressure: Real,
    pub fluid_v_q: Real,
    pub fluid_v_r: Real,
}

/// Number of named-substance density tracks held in
/// `PhysicsState::substances`. The integer indices align with the
/// `Substance` enum in `crate::chemistry`. Grows as new substances
/// are authored — keep this in sync with the enum.
pub const N_SUBSTANCES: usize = 9;

#[derive(Debug, Clone)]
pub struct PhysicsState {
    grid: HexGrid,
    /// Terrain height. Set at run start from planet seed; not
    /// updated by M1a physics (tectonics is permanently deferred).
    elevation: Vec<Real>,
    /// Water column depth above the terrain. Mutated by fluid law.
    water_depth: Vec<Real>,
    temperature: Vec<Real>,
    pressure: Vec<Real>,
    fluid_v_q: Vec<Real>,
    fluid_v_r: Vec<Real>,
    /// Per-substance density at each cell. Outer index is the
    /// substance id (a `crate::chemistry::Substance` cast to usize);
    /// inner index is the cell index. Mutated by chemistry law.
    substances: Vec<Vec<Real>>,
    /// Electric charge per cell. Signed; mutated by EM law (M1b).
    charge: Vec<Real>,
    /// Planetary magnetic field as a vector — `(B_q, B_r)`
    /// per cell in axial coordinates. Previously this was a scalar
    /// magnitude with no direction; promoting to a vector lets the
    /// dipole pattern (axis-aligned, equatorially horizontal) carry
    /// through to any future Lorentz-force coupling on charged
    /// particle motion. Initialised at planet init by
    /// `Magnetism::init_field`; modulated each macro-step by
    /// `Magnetism::integrate` for diurnal variation.
    magnetic_b_q: Vec<Real>,
    magnetic_b_r: Vec<Real>,
    /// Vertical-axis (out-of-plane) component of the
    /// magnetic vector field. Real planetary dipoles have a
    /// vertical component that peaks at the magnetic *poles*
    /// (where the horizontal components vanish) and is zero at
    /// the equator. Previously the Lorentz law approximated
    /// this with `|B|` (the horizontal magnitude), which was
    /// wrong at the poles where `|B_horizontal|` ≈ 0 but `B_z` is
    /// largest. With `B_z` as a real field, the Lorentz law reads the true
    /// vertical component for the cross-product physics.
    magnetic_b_z: Vec<Real>,
    /// Cached per-cell magnetic-field magnitude
    /// (`sqrt(B_q² + B_r²)`). Updated by `Magnetism::init_field`
    /// and `Magnetism::integrate` whenever they touch the vector
    /// components. Cached because the recognition scan reads
    /// per-cell magnitude on every tick × every template; the
    /// uncached version added ~50 % to the long-test runtime.
    /// Maintenance contract: any law that writes
    /// `magnetic_b_q` / `magnetic_b_r` must also refresh
    /// `magnetic_magnitude`.
    magnetic_magnitude: Vec<Real>,
    /// Macro-step counter used by laws that need a planet-wide
    /// clock (tides, seasonal insolation, diurnal cycles). Advanced
    /// once per macro-step by `orchestration::integrate_civ_step`.
    /// Stored as u64 so a million-year run (~1.2×10⁸ macro-steps)
    /// fits with margin to spare.
    macro_step: u64,
    /// Per-cell upper-atmosphere temperature in K. Sits one
    /// layer above the surface `temperature` field; coupled to it
    /// by vertical convection (`VerticalConvection` law). Previously
    /// the simulation had only one temperature field per cell —
    /// no vertical structure, no lapse rate, no high-altitude
    /// cloud / vapour layering. This field introduces a single upper
    /// layer as the minimum-viable vertical resolution; future
    /// refinements can grow this into a multi-level stack.
    upper_temperature: Vec<Real>,
    /// Per-cell biofuel carrying-capacity ceiling. Set at planet
    /// init from the cell's biosphere contribution (land + non-zero
    /// biosphere class → positive ceiling; sea + lifeless worlds →
    /// zero). Read by the `BiofuelRegrowth` reaction as the upper
    /// bound a regrowing cell relaxes toward; never mutated by
    /// physics laws. Stored per-cell rather than recomputed because
    /// future tuning may want spatial variation (terrain, microclimate
    /// modifiers) that's awkward to derive from scratch each tick.
    biofuel_ceiling: Vec<Real>,
    /// Per-cell fraction of cell surface covered by snow.
    /// Authored by the `IceAlbedo` law each macro-step from
    /// temperature + precipitation. Drives the highest-albedo
    /// channel (`0.85 × snow_fraction`). On land + on top of
    /// sea ice. Range `[0, 1]`.
    snow_fraction: Vec<Real>,
    /// Per-cell fraction of cell surface covered by sea ice
    /// without snow on top. Gray albedo (`0.55`), not white —
    /// younger / refrozen sea ice is darker than glacial / snow-
    /// capped ice. Authored by the `IceAlbedo` law from surface
    /// temperature alone. Range `[0, 1]`.
    sea_ice_fraction: Vec<Real>,
    /// Per-cell fraction of cell covered by cloud. Authored by
    /// the `Clouds` law (Sprint 5 Item 23) from vapour
    /// supersaturation + vertical motion proxies; read by the
    /// `effective_albedo_for` helper and by `Radiation` for the
    /// per-cell greenhouse contribution. Range `[0, 1]`.
    cloud_fraction: Vec<Real>,
    /// Per-cell cloud type discriminant (Sprint 5 Item 23). One
    /// byte per cell: `0` = stratus (low-altitude, high-albedo,
    /// low-greenhouse), `1` = cirrus (high-altitude, low-albedo,
    /// high-greenhouse). Authored by the `Clouds` law based on
    /// surface elevation + vertical motion strength; read by
    /// `effective_albedo_for` (cloud type modulates the cloud
    /// channel's albedo peak) and `Radiation` (cloud type
    /// modulates the per-cell greenhouse contribution). Stored as
    /// `u8` rather than `Vec<CloudType>` so the per-cell footprint
    /// stays one byte and the slice can be passed to SIMD-friendly
    /// loops without enum boxing.
    cloud_type: Vec<u8>,
    /// Tectonic plate id owning each cell (Sprint 4 Item 12). Set
    /// at worldgen via `Tectonics::sample_plates_for_seed`; immutable
    /// per-cell in this PR (future Items 12a subduction + 12e
    /// slab-pull will mutate it as plate boundaries migrate). Index
    /// into `Tectonics::plates`. Length is either zero (no plates
    /// sampled — `Tectonics::integrate` no-ops) or `grid.n_cells()`.
    plate_id: Vec<u32>,
    /// Per-cell crust thickness in km-equivalent (Sprint 4 Item 12).
    /// Initialised to the owning plate's default (~7 km oceanic,
    /// ~35 km continental); per-cell rather than per-plate so future
    /// Item 12c isostasy + Item 12a subduction can grow / shrink
    /// individual cells. Length is either zero or `grid.n_cells()`
    /// (same contract as `plate_id`).
    crust_thickness: Vec<Real>,
    /// Aggregate mass of crust consumed by subduction over the run
    /// (Sprint 4 Item 12a). Accumulates as oceanic-side cells at
    /// convergent boundaries decay toward zero thickness; future
    /// volcanism (Item 12d) draws from this pool to outgas CO2 and
    /// rebuild crust at arc / hotspot positions. Aggregate (not per-
    /// cell) because subducted material decouples from its origin
    /// cell once it enters the mantle — it surfaces wherever the
    /// volcanism law decides, not where the oceanic plate sank.
    subducted_mass: Real,
    /// Per-cell crust age in ticks since formation (Sprint 4 Item 12b).
    /// Incremented every tick by `Tectonics::integrate` except for
    /// cells at divergent (ridge) boundaries where fresh crust spawns
    /// and the age is reset to zero. Drives oceanic ridge-cooling
    /// depth via the `depth ∝ sqrt(age)` half-space-cooling law:
    /// older sea floor sits lower than younger sea floor near a
    /// mid-ocean ridge. Length is either zero (no plates installed)
    /// or `grid.n_cells()` (same contract as `plate_id`).
    crust_age: Vec<u64>,
    /// Per-cell isostatic baseline elevation in scaled units
    /// (Sprint 4 Item 12c). Represents the "geological base" surface
    /// — the elevation contribution that is *not* due to current
    /// isostatic lift from the crust column. The invariant
    /// `elevation = h_base + (ρ_mantle/ρ_crust - 1) × crust_thickness`
    /// is maintained at the end of every `apply_isostasy` pass.
    /// Lazily baked on the first `apply_isostasy` call after
    /// `set_tectonics_fields` — empty until then. Tectonic uplift
    /// + erosion write to `elevation` directly; `apply_isostasy`
    /// absorbs those changes into `h_base` and re-derives elevation
    /// from the current crust thickness so a thickness change
    /// (subduction-driven thinning, convergent thickening) lifts /
    /// drops the surface without disturbing the geological signal.
    h_base: Vec<Real>,
    /// Per-cell snapshot of `crust_thickness` at the last
    /// `apply_isostasy` pass (Sprint 4 Item 12c). Used to detect
    /// thickness changes between calls so external mutations to
    /// `elevation` (from convergent uplift / erosion) can be
    /// separated from isostasy-induced changes. Empty until the
    /// first `apply_isostasy` pass bakes it.
    last_thickness: Vec<Real>,
    /// Geomagnetic dipole state machine (Sprint 5 Item 20). A
    /// Markov chain over `{Normal, Reversing, Reversed}`: from a
    /// stable polarity the law trials a rare reversal event each
    /// tick; once started, the reversal completes after a fixed
    /// window (`reversal_duration_ticks`) and flips the stable
    /// polarity. The state itself doesn't affect the per-cell
    /// `(B_q, B_r, B_z)` vector field directly — it scales the
    /// `dipole_strength` envelope below, which the `Magnetism`
    /// law could read on a future refactor to weaken the per-
    /// cell vectors during the reversal window.
    dipole_state: DipoleState,
    /// Envelope multiplier on the per-cell magnetic field
    /// strength (Sprint 5 Item 20). 1.0 = full strength (stable
    /// polarity); during a reversal this decays linearly to a
    /// floor (~0.1) at the midpoint of the reversal window and
    /// ramps back up as the new polarity locks in. Read by
    /// `cosmic_ray_ground_flux()` for the inverse-coupling
    /// accessor; a future refactor can also scale the per-cell
    /// vectors in `Magnetism::integrate` by this value to make
    /// the weakening visible to recognition templates.
    dipole_strength: Real,
    /// Tick at which the *current* reversal began (Sprint 5 Item
    /// 20). `None` when the dipole sits in a stable state
    /// (`Normal` or `Reversed`). Set on the trial-success
    /// transition; cleared on the reversal-complete transition.
    reversal_start_tick: Option<u64>,
    /// Tick at which the previous reversal *completed* (Sprint
    /// 5 Item 20). Used by diagnostics and any future law that
    /// wants to gate on "time since last polarity flip" (e.g.
    /// post-reversal cosmic-ray-driven mutation pulses). `0`
    /// before the first reversal of a run.
    last_reversal_tick: u64,
}

impl PhysicsState {
    /// Build a fresh state with the given grid; all fields zero. Real
    /// runs initialise from planet seed before the first integration;
    /// tests construct directly.
    pub fn new(grid: HexGrid) -> Self {
        let n = grid.n_cells();
        Self {
            elevation: vec![Real::ZERO; n],
            water_depth: vec![Real::ZERO; n],
            temperature: vec![Real::ZERO; n],
            pressure: vec![Real::ZERO; n],
            fluid_v_q: vec![Real::ZERO; n],
            fluid_v_r: vec![Real::ZERO; n],
            substances: vec![vec![Real::ZERO; n]; N_SUBSTANCES],
            charge: vec![Real::ZERO; n],
            magnetic_b_q: vec![Real::ZERO; n],
            magnetic_b_r: vec![Real::ZERO; n],
            magnetic_b_z: vec![Real::ZERO; n],
            magnetic_magnitude: vec![Real::ZERO; n],
            macro_step: 0,
            upper_temperature: vec![Real::ZERO; n],
            biofuel_ceiling: vec![Real::ZERO; n],
            snow_fraction: vec![Real::ZERO; n],
            sea_ice_fraction: vec![Real::ZERO; n],
            cloud_fraction: vec![Real::ZERO; n],
            // Default `0` = stratus. Cells without active vertical
            // motion default to low-altitude stratus; the `Clouds`
            // law re-classifies as cirrus when elevation or
            // updraft strength crosses the thresholds.
            cloud_type: vec![0u8; n],
            // Empty by default — populated by
            // `set_tectonics_fields` after worldgen samples plates.
            // The `Tectonics::integrate` law gates on the empty case
            // so tests + bare-state callers don't have to fabricate
            // a plate roster they don't need.
            plate_id: Vec::new(),
            crust_thickness: Vec::new(),
            subducted_mass: Real::ZERO,
            // Per-cell crust age in ticks. Empty by default — sized
            // by `set_tectonics_fields` alongside `plate_id` /
            // `crust_thickness` so the three tectonics arrays share
            // a single source of truth for their length.
            crust_age: Vec::new(),
            // Sprint 4 Item 12c: empty until first `apply_isostasy`
            // pass lazily bakes them. Lazy baking lets tests set
            // elevation + thickness after `set_tectonics_fields`
            // without having to manually rebake — the first pass
            // captures whatever state exists at that moment as the
            // isostatic baseline.
            h_base: Vec::new(),
            last_thickness: Vec::new(),
            // Sprint 5 Item 20: fresh planet starts on Normal
            // polarity at full dipole strength with no in-flight
            // reversal. `last_reversal_tick` stays 0 until the
            // first reversal completes.
            dipole_state: DipoleState::Normal,
            dipole_strength: Real::ONE,
            reversal_start_tick: None,
            last_reversal_tick: 0,
            grid,
        }
    }

    /// Current macro-step counter. Laws that need a planet-wide
    /// clock (tides, seasonal forcing) read this. Advanced exclusively
    /// by `orchestration::integrate_civ_step`; never written by laws.
    #[must_use]
    pub fn macro_step(&self) -> u64 {
        self.macro_step
    }

    /// Bump the macro-step counter. Only the orchestrator should
    /// call this — once per macro-step, after the law sequence runs.
    pub fn advance_macro_step(&mut self) {
        self.macro_step = self.macro_step.saturating_add(1);
    }

    pub fn grid(&self) -> &HexGrid {
        &self.grid
    }

    pub fn cell(&self, id: CellId) -> Cell {
        let i = id.0 as usize;
        Cell {
            elevation: self.elevation[i],
            water_depth: self.water_depth[i],
            temperature: self.temperature[i],
            pressure: self.pressure[i],
            fluid_v_q: self.fluid_v_q[i],
            fluid_v_r: self.fluid_v_r[i],
        }
    }

    pub fn elevation(&self) -> &[Real] {
        &self.elevation
    }

    pub fn elevation_mut(&mut self) -> &mut [Real] {
        &mut self.elevation
    }

    pub fn water_depth(&self) -> &[Real] {
        &self.water_depth
    }

    pub fn water_depth_mut(&mut self) -> &mut [Real] {
        &mut self.water_depth
    }

    pub fn temperature(&self) -> &[Real] {
        &self.temperature
    }

    pub fn temperature_mut(&mut self) -> &mut [Real] {
        &mut self.temperature
    }

    pub fn pressure(&self) -> &[Real] {
        &self.pressure
    }

    pub fn pressure_mut(&mut self) -> &mut [Real] {
        &mut self.pressure
    }

    pub fn fluid_velocity(&self) -> (&[Real], &[Real]) {
        (&self.fluid_v_q, &self.fluid_v_r)
    }

    pub fn fluid_velocity_mut(&mut self) -> (&mut [Real], &mut [Real]) {
        (&mut self.fluid_v_q, &mut self.fluid_v_r)
    }

    /// Density of the substance at index `id` for every cell.
    pub fn substance(&self, id: usize) -> &[Real] {
        &self.substances[id]
    }

    pub fn substance_mut(&mut self, id: usize) -> &mut [Real] {
        &mut self.substances[id]
    }

    pub fn charge(&self) -> &[Real] {
        &self.charge
    }

    pub fn charge_mut(&mut self) -> &mut [Real] {
        &mut self.charge
    }

    /// Vector magnetic field — `(B_q, B_r)` slices in axial
    /// coordinates. Previously this returned a scalar magnitude; the
    /// vector form lets directional couplings (Lorentz force,
    /// magnetic compass templates) read the orientation as well.
    /// `magnetic_field_magnitude(cell)` derives the scalar magnitude
    /// for callers that only want it.
    #[must_use]
    pub fn magnetic_field(&self) -> (&[Real], &[Real]) {
        (&self.magnetic_b_q, &self.magnetic_b_r)
    }

    pub fn magnetic_field_mut(&mut self) -> (&mut [Real], &mut [Real]) {
        (&mut self.magnetic_b_q, &mut self.magnetic_b_r)
    }

    /// Vertical (out-of-plane) magnetic-field component.
    /// Read by the Lorentz coupling for the true 3D
    /// `F = q · v × B` cross-product instead of the previous
    /// `|B|` proxy.
    #[must_use]
    pub fn magnetic_field_z(&self) -> &[Real] {
        &self.magnetic_b_z
    }

    pub fn magnetic_field_z_mut(&mut self) -> &mut [Real] {
        &mut self.magnetic_b_z
    }

    /// Scalar magnitude of the magnetic field at a cell —
    /// `sqrt(B_q² + B_r²)`. Returns the cached
    /// `magnetic_magnitude[cell]` rather than recomputing the
    /// sqrt each call. The cache is refreshed by
    /// `Magnetism::init_field` / `Magnetism::integrate`; tests
    /// that mutate the vector components directly should call
    /// `refresh_magnetic_magnitude` (test-only helper) before
    /// reading.
    #[must_use]
    pub fn magnetic_field_magnitude(&self, cell: usize) -> Real {
        self.magnetic_magnitude[cell]
    }

    /// Full magnitude slice (caller-side bulk reads). Same
    /// caching contract as `magnetic_field_magnitude`.
    #[must_use]
    pub fn magnetic_magnitude(&self) -> &[Real] {
        &self.magnetic_magnitude
    }

    /// Write-side accessor for the cached magnitude. Only
    /// `Magnetism` (or test code mirroring its updates) should
    /// touch this; recognition reads via `magnetic_magnitude()`.
    pub fn magnetic_magnitude_mut(&mut self) -> &mut [Real] {
        &mut self.magnetic_magnitude
    }

    /// Per-cell upper-atmosphere temperature in K. Previously
    /// the simulation was vertically homogeneous; this slice gives
    /// vertical-convection laws somewhere to put the lapse-rate
    /// signal. Initialised to zero and seeded at planet init or
    /// by the first `VerticalConvection::integrate` pass.
    #[must_use]
    pub fn upper_temperature(&self) -> &[Real] {
        &self.upper_temperature
    }

    pub fn upper_temperature_mut(&mut self) -> &mut [Real] {
        &mut self.upper_temperature
    }

    /// Per-cell biofuel ceiling (regrowth target). Set once at
    /// planet init from biosphere class + land mask; constant
    /// across a run.
    #[must_use]
    pub fn biofuel_ceiling(&self) -> &[Real] {
        &self.biofuel_ceiling
    }

    pub fn biofuel_ceiling_mut(&mut self) -> &mut [Real] {
        &mut self.biofuel_ceiling
    }

    /// Per-cell snow-cover fraction (`[0, 1]`). Authored by the
    /// `IceAlbedo` law; read by the per-cell effective-albedo
    /// helper that modulates `Radiation`'s per-row T_eq table.
    #[must_use]
    pub fn snow_fraction(&self) -> &[Real] {
        &self.snow_fraction
    }

    pub fn snow_fraction_mut(&mut self) -> &mut [Real] {
        &mut self.snow_fraction
    }

    /// Per-cell sea-ice cover fraction, gray-channel (no snow on
    /// top). `[0, 1]`. Authored by the `IceAlbedo` law from
    /// surface temperature alone; read by the effective-albedo
    /// helper.
    #[must_use]
    pub fn sea_ice_fraction(&self) -> &[Real] {
        &self.sea_ice_fraction
    }

    pub fn sea_ice_fraction_mut(&mut self) -> &mut [Real] {
        &mut self.sea_ice_fraction
    }

    /// Per-cell cloud-cover fraction (`[0, 1]`). Authored by the
    /// `Clouds` law (Sprint 5 Item 23) from vapour supersaturation
    /// against [`crate::hydrology::saturation_vapour_cap`] plus a
    /// vertical-motion proxy (surface-vs-upper temperature gap).
    /// Read by `effective_albedo_for` (modulated by per-cell
    /// `cloud_type` — cirrus contributes ~0.2, stratus ~0.5) and
    /// by `Radiation` for the per-cell greenhouse contribution.
    #[must_use]
    pub fn cloud_fraction(&self) -> &[Real] {
        &self.cloud_fraction
    }

    pub fn cloud_fraction_mut(&mut self) -> &mut [Real] {
        &mut self.cloud_fraction
    }

    /// Per-cell cloud-type byte (Sprint 5 Item 23). `0` = stratus,
    /// `1` = cirrus. Authored by the `Clouds` law from surface
    /// elevation + vertical-motion strength; read by the
    /// effective-albedo helper and by `Radiation` for the per-cell
    /// greenhouse contribution. See [`crate::clouds::CloudType`]
    /// for the typed wrapper around individual entries.
    #[must_use]
    pub fn cloud_type(&self) -> &[u8] {
        &self.cloud_type
    }

    pub fn cloud_type_mut(&mut self) -> &mut [u8] {
        &mut self.cloud_type
    }

    /// Per-cell tectonic plate id (Sprint 4 Item 12). Empty slice
    /// when no plates have been sampled — callers that depend on
    /// the field being populated must check `len() ==
    /// grid().n_cells()` before indexing. `Tectonics::integrate`
    /// already does this and no-ops on the bare case.
    #[must_use]
    pub fn plate_id(&self) -> &[u32] {
        &self.plate_id
    }

    pub fn plate_id_mut(&mut self) -> &mut [u32] {
        &mut self.plate_id
    }

    /// Per-cell crust thickness in km-equivalent (Sprint 4 Item 12).
    /// Empty slice when no plates have been sampled; otherwise
    /// `grid().n_cells()` long.
    #[must_use]
    pub fn crust_thickness(&self) -> &[Real] {
        &self.crust_thickness
    }

    pub fn crust_thickness_mut(&mut self) -> &mut [Real] {
        &mut self.crust_thickness
    }

    /// Per-cell crust age in ticks since formation (Sprint 4 Item
    /// 12b). Empty slice when no plates have been sampled; otherwise
    /// `grid().n_cells()` long. Bumped each tick by
    /// `Tectonics::integrate` except at divergent boundaries, where
    /// the age is reset to zero — fresh crust spawning at a ridge.
    /// Read by the ocean-floor depth modulator to drive the
    /// half-space cooling `depth ∝ sqrt(age)` law.
    #[must_use]
    pub fn crust_age(&self) -> &[u64] {
        &self.crust_age
    }

    pub fn crust_age_mut(&mut self) -> &mut [u64] {
        &mut self.crust_age
    }

    /// Install per-cell tectonics fields (Sprint 4 Item 12). The two
    /// vectors must already be sized to `grid().n_cells()`; this
    /// method just installs them. Worldgen calls this once after
    /// `Tectonics::sample_plates_for_seed`. Existing tests construct
    /// the fields by hand for deterministic plate layouts.
    ///
    /// `plate_id` and `crust_thickness` are paired in this single
    /// setter so callers can't install one without the other —
    /// previously a partial install would have left
    /// `Tectonics::integrate` reading a zero-length field on one
    /// side and a populated field on the other, with no clean
    /// semantics for that mismatch.
    ///
    /// The companion `crust_age` field (Sprint 4 Item 12b) is sized
    /// to `grid().n_cells()` and zeroed here too: a planet starts
    /// with all-zero ages and accrues age as ticks pass, except at
    /// ridge cells where the age is reset to zero each tick.
    pub fn set_tectonics_fields(&mut self, plate_id: Vec<u32>, crust_thickness: Vec<Real>) {
        let n = self.grid.n_cells();
        assert_eq!(
            plate_id.len(),
            n,
            "plate_id length {} must match grid n_cells {n}",
            plate_id.len()
        );
        assert_eq!(
            crust_thickness.len(),
            n,
            "crust_thickness length {} must match grid n_cells {n}",
            crust_thickness.len()
        );
        self.plate_id = plate_id;
        self.crust_thickness = crust_thickness;
        // Initialise per-cell crust age to zero. `Tectonics::integrate`
        // advances this each tick.
        self.crust_age = vec![0u64; n];
        // Sprint 4 Item 12c: invalidate isostasy bookkeeping so the
        // next `apply_isostasy` pass re-bakes against the freshly
        // installed thickness + whatever elevation is current at
        // that moment. Without the clear, a state that gets its
        // tectonics fields reinstalled (e.g. a future worldgen
        // rerun) would carry stale `h_base` / `last_thickness` from
        // the previous run and corrupt the surface elevation.
        self.h_base.clear();
        self.last_thickness.clear();
    }

    /// Per-cell isostatic baseline elevation (Sprint 4 Item 12c).
    /// Empty until `apply_isostasy` runs once after tectonics fields
    /// are installed. Read-only public accessor for diagnostics and
    /// tests; the canonical write path is `apply_isostasy` itself.
    #[must_use]
    pub fn h_base(&self) -> &[Real] {
        &self.h_base
    }

    /// Mutable accessor for `h_base` — required by the isostasy law
    /// which is in a sibling module (so a private field wouldn't be
    /// reachable). Modules outside `physics` should never call this;
    /// going through `apply_isostasy` keeps the
    /// `elevation = h_base + factor × thickness` invariant intact.
    pub fn h_base_mut(&mut self) -> &mut Vec<Real> {
        &mut self.h_base
    }

    /// Per-cell snapshot of `crust_thickness` at the last
    /// `apply_isostasy` pass (Sprint 4 Item 12c). Same semantics as
    /// `h_base`: empty until first pass, then sized to
    /// `grid().n_cells()`. Exposed for diagnostics + tests.
    #[must_use]
    pub fn last_thickness(&self) -> &[Real] {
        &self.last_thickness
    }

    /// Mutable accessor for `last_thickness` — same isolation
    /// rationale as `h_base_mut`.
    pub fn last_thickness_mut(&mut self) -> &mut Vec<Real> {
        &mut self.last_thickness
    }

    /// Aggregate mass of subducted crust (Sprint 4 Item 12a). In
    /// km-equivalent thickness units, summed across every cell that
    /// has lost mass at a convergent oceanic boundary. Future
    /// volcanism (Item 12d) drains this pool when it outgasses; for
    /// now `Tectonics::integrate` is a pure source.
    #[must_use]
    pub fn subducted_mass(&self) -> Real {
        self.subducted_mass
    }

    /// Mutable accessor for the subducted-mass pool. Used by
    /// `Tectonics::integrate` to deposit consumed oceanic mass and
    /// by Item 12d volcanism to drain it once that law lands.
    pub fn subducted_mass_mut(&mut self) -> &mut Real {
        &mut self.subducted_mass
    }

    /// Current geomagnetic dipole state (Sprint 5 Item 20). One of
    /// `Normal`, `Reversing`, `Reversed`. Mutated by
    /// `MagneticReversal::integrate`.
    #[must_use]
    pub fn dipole_state(&self) -> DipoleState {
        self.dipole_state
    }

    /// Mutable accessor for the dipole state. Only the
    /// `MagneticReversal` law and tests forcing a known polarity
    /// should touch this.
    pub fn dipole_state_mut(&mut self) -> &mut DipoleState {
        &mut self.dipole_state
    }

    /// Per-planet envelope multiplier on the magnetic field
    /// strength (Sprint 5 Item 20). 1.0 in stable states; decays
    /// to a floor (~0.1) at the midpoint of a reversal window and
    /// ramps back up. Read by `cosmic_ray_ground_flux()`.
    #[must_use]
    pub fn dipole_strength(&self) -> Real {
        self.dipole_strength
    }

    /// Mutable accessor for the dipole-strength envelope. Same
    /// isolation rationale as `dipole_state_mut` — only the
    /// reversal law writes through this path.
    pub fn dipole_strength_mut(&mut self) -> &mut Real {
        &mut self.dipole_strength
    }

    /// Tick the *current* reversal began at, or `None` when the
    /// dipole sits in a stable polarity (Sprint 5 Item 20).
    #[must_use]
    pub fn reversal_start_tick(&self) -> Option<u64> {
        self.reversal_start_tick
    }

    /// Mutable accessor for `reversal_start_tick`. Written by the
    /// reversal law on the trial-success edge and cleared on the
    /// reversal-complete edge.
    pub fn reversal_start_tick_mut(&mut self) -> &mut Option<u64> {
        &mut self.reversal_start_tick
    }

    /// Tick at which the *previous* reversal completed (Sprint 5
    /// Item 20). `0` before the first reversal of a run. Diagnostics
    /// and tests can use the gap between successive values to
    /// estimate the realised inter-reversal period.
    #[must_use]
    pub fn last_reversal_tick(&self) -> u64 {
        self.last_reversal_tick
    }

    /// Mutable accessor for `last_reversal_tick`. Written by the
    /// reversal law when a reversal window closes.
    pub fn last_reversal_tick_mut(&mut self) -> &mut u64 {
        &mut self.last_reversal_tick
    }

    /// Cosmic-ray ground flux multiplier (Sprint 5 Item 20).
    /// Inverse-coupled to `dipole_strength`: a strong dipole
    /// shields the surface (low flux), a weak / vanishing
    /// dipole during a reversal lets cosmic rays through
    /// (high flux). Functional form: `1 / (strength + 0.1)`.
    /// The `+ 0.1` floor keeps the multiplier finite when the
    /// dipole drops to zero mid-reversal. At `strength = 1` the
    /// multiplier is `1 / 1.1 ≈ 0.91`; at `strength = 0.1` it is
    /// `1 / 0.2 = 5.0` — a ~5× surface-flux amplification during
    /// the deepest part of a reversal window.
    ///
    /// Read by Item 11 (species drift) for mutation-rate
    /// coupling and by Item 17 (atmospheric escape) for ion-
    /// channel modulation; pure accessor with no side effects.
    #[must_use]
    pub fn cosmic_ray_ground_flux(&self) -> Real {
        Real::ONE / (self.dipole_strength + Real::from_ratio(1, 10))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_state_zeroed() {
        let g = HexGrid::new(3, 3);
        let s = PhysicsState::new(g);
        for t in s.temperature() {
            assert_eq!(*t, Real::ZERO);
        }
    }
}
