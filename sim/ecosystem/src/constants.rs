//! `sim-ecosystem` calibrated constants.
//!
//! Centralised so the rate / threshold tuning surface is in one place
//! (Sprint 2 Item P2.x → CA2 split). Every value here is a calibrated
//! coefficient referenced by the per-tick step in
//! [`crate::planet::PlanetEcosystem`] or by the sampling path in
//! [`crate::sampling`]. Comments on each item explain the calibration
//! source.

/// Default Lindeman 10:1 assimilation ratio — back-compat fallback
/// used for fixtures that don't pin a habitat. The canonical
/// terrestrial value; per-habitat overrides live in
/// [`crate::invariants::lindeman_assimilation_for_habitat`].
///
/// **Note (P2.5):** the ecosystem step used to *both* assimilate at
/// 10% during predation *and* run a post-step `enforce_lindeman_pyramid`
/// scaling pass that re-clamped each tier to ≤ 0.1× the lower tier.
/// That was double-bookkeeping: a calibrated assimilation efficiency
/// is the *physical* mechanism that produces the pyramid at
/// steady state, so the post-step cap is redundant. The cap is gone;
/// the per-habitat assimilation ratio carries the whole load.
pub const LINDEMAN_RATIO: (i64, i64) = (1, 10);

pub const LINDEMAN_OVERSHOOT_DEBUG_MAX: i64 = 5;

/// Half-saturation default (P2.6 — was K_HALF_SAT; renamed _DEFAULT
/// since per-pair `Interaction::half_saturation` is now the production
/// path). Consumers only reach this when `half_saturation = ZERO`.
pub const K_HALF_SAT_DEFAULT: (i64, i64) = (1, 2);

/// Canonical per-pair half-saturation fractions (P2.6).
pub const HALF_SAT_APEX_PREDATOR: (i64, i64) = (1, 10);
pub const HALF_SAT_SPECIALIST_PREDATOR: (i64, i64) = (3, 10);
pub const HALF_SAT_MUTUALISM: (i64, i64) = (5, 10);
pub const HALF_SAT_HABITAT_MOD: (i64, i64) = (2, 10);

/// Per-tick base growth rate for producers (fraction of carrying
/// capacity). The producer pool drifts toward
/// `producer_capacity` at this fraction per tick when not grazed.
pub const PRODUCER_GROWTH_RATE: (i64, i64) = (2, 100);

/// Per-tick passive mortality for any non-producer species. Without
/// this, predator pools never decay between feedings and oscillations
/// collapse into monotonic ramps.
pub const CONSUMER_DECAY_RATE: (i64, i64) = (1, 100);

/// Betweenness-centrality threshold above which a species is flagged
/// as a keystone. Tuned for the 8-20 species per-planet target where
/// the producer hubs naturally accumulate centrality of order
/// `n_species × n_consumers`. Expressed as a fraction of the maximum
/// possible centrality (n × (n-1)).
pub const KEYSTONE_CENTRALITY_THRESHOLD: (i64, i64) = (15, 100);

/// Syntrophy partner-biomass floor (Sprint 2 Item 9). Mutualism
/// pairs whose smaller partner falls below this absolute biomass
/// drag *both* sides toward extinction at
/// `SYNTROPHY_COLLAPSE_RATE` per tick. The floor is calibrated as a
/// small absolute number rather than as a fraction of capacity so a
/// pair with biomass `(1, 0.01)` reads "the 0.01 side is below the
/// floor → the pair collapses" regardless of the producer pool size.
pub const SYNTROPHY_MIN_PARTNER_BIOMASS: (i64, i64) = (1, 100);

/// Per-tick fractional collapse applied to *both* sides of a
/// Mutualism pair when one partner falls below
/// `SYNTROPHY_MIN_PARTNER_BIOMASS`. 25% per tick is fast enough that
/// the test's "within a few ticks" assertion holds, and slow enough
/// that a transient dip below the floor (e.g. due to a single
/// catastrophic predation event) doesn't trip the cascade on a single
/// tick.
pub const SYNTROPHY_COLLAPSE_RATE: (i64, i64) = (25, 100);

/// P3.1 — `MutualismKind::SeedDisperser` activation threshold. When
/// the disperser side's biomass is at or above this fraction of the
/// planet's producer capacity, the mutualistic flux into the producer
/// is multiplied by [`SEED_DISPERSER_RANGE_BOOST`] (modelling extended
/// effective range from the disperser ferrying propagules into new
/// cells). Below threshold the interaction falls through to the
/// generic mutualism flux. Calibrated as 0.5% of capacity so a small
/// disperser cohort can still trigger the boost, but the trigger
/// requires a real population — not a vanishing trace.
pub const SEED_DISPERSER_BIOMASS_THRESHOLD: (i64, i64) = (5, 1000);
/// P3.1 — `MutualismKind::SeedDisperser` flux multiplier applied to
/// the producer side of the mutualism once the disperser's biomass
/// clears [`SEED_DISPERSER_BIOMASS_THRESHOLD`]. 1.20× matches the
/// "extended effective range" intuition: the producer's growth from
/// the mutualism gets a 20% bump because seeds are reaching new cells.
pub const SEED_DISPERSER_RANGE_BOOST: (i64, i64) = (120, 100);

/// P3.1 — `MutualismKind::Pollinator` per-unit-biomass coupling
/// multiplier. The pollinator-side flux into the producer is scaled
/// by `1 + POLLINATOR_BIOMASS_COUPLING × (pollinator_biomass /
/// producer_capacity)`, so a pollinator cohort at e.g. 1% of capacity
/// boosts the flux by 30% (`POLLINATOR_BIOMASS_COUPLING = 30`). The
/// scaling sits on top of the generic mutualism flux and saturates
/// gracefully — at very high pollinator biomass the multiplier still
/// grows linearly, which is fine because the underlying flux is
/// saturating (Type-II) in the producer side.
pub const POLLINATOR_BIOMASS_COUPLING: i64 = 30;

/// P3.1 — `MutualismKind::Engineer` per-cell tolerance-match boost
/// applied to cohabiting species when an engineer is present and
/// active. Modelled as an additive multiplier on the cohabitor's
/// growth-derived flux into the species; +10% bump matches the
/// "shift per-cell habitat" intuition. Cohabitation is defined as
/// "same `Habitat` tag" since the ecosystem layer doesn't carry
/// per-cell occupancy — the closest approximation we can make at
/// this layer.
pub const ENGINEER_MATCH_BOOST: (i64, i64) = (10, 100);

/// P3.1 — `ParasiteKind::Macro` host fertility multiplier. Macro
/// parasites (worms, fleas) impose a chronic reproductive cost —
/// modelled as a 10% extra deduction on the host's biomass on top of
/// the generic parasitism flux (a hit to the host's growth potential
/// per tick). The 10% figure mirrors the field-ecology rule of thumb
/// for chronic helminth burdens on large herbivores.
pub const MACRO_FERTILITY_MULTIPLIER: (i64, i64) = (10, 100);

/// P3.1 — `ParasiteKind::Micro` crowding-disease scaling. When the
/// host's biomass exceeds [`MICRO_CROWDING_THRESHOLD`] (5% of producer
/// capacity), micro parasites (bacteria, protists) impose an
/// additional -5% biomass hit per tick — modelling the density-
/// dependent epidemic transmission of crowd diseases. Below the
/// threshold the host is sparse enough that transmission rate doesn't
/// add a meaningful extra loss.
pub const MICRO_SURVIVAL_PENALTY: (i64, i64) = (5, 100);
pub const MICRO_CROWDING_THRESHOLD: (i64, i64) = (5, 100);

/// P3.1 — `ParasiteKind::Virus` episodic outbreak cadence and
/// intensity. Every `VIRUS_OUTBREAK_PERIOD` ticks a virus parasite
/// fires a deterministic SplitMix64-driven hit at
/// `VIRUS_OUTBREAK_HOST_LOSS` × host biomass (default -30% — matching
/// the rough field rule for a virgin-soil viral epidemic). Between
/// outbreaks the interaction is inert (no flux, no biomass change).
/// The cadence is a hard period (no jitter) so deterministic replay
/// is byte-stable; the SplitMix step is reserved for tie-breaking
/// when multiple virus parasites coexist on the same host.
pub const VIRUS_OUTBREAK_PERIOD: u64 = 100;
pub const VIRUS_OUTBREAK_HOST_LOSS: (i64, i64) = (30, 100);

/// Per-Chemoautotroph-species growth-demand baseline used by
/// `partition_chemoautotrophs`. A Chemoautotroph wants to add up to
/// this fraction of the producer carrying capacity per tick, scaled
/// by its current biomass / capacity ratio so empty pools fill fast
/// and saturated pools coast. Identical in shape to
/// `PRODUCER_GROWTH_RATE` (which drives Photoautotrophs) but routed
/// through `oxidiser_ladder` so the per-tick growth is also capped
/// by oxidiser availability — a chemolithotroph on a CO2-poor
/// hydrocarbon world can't grow even if biomass demand says it
/// should.
pub const CHEMOAUTOTROPH_GROWTH_RATE: (i64, i64) = (2, 100);

/// Biomass floor below which a species is considered to be
/// collapsing. Expressed as a fraction of the planet's
/// `producer_capacity` so the threshold scales with planet size:
/// `0.001 × capacity`. Sprint 2 Item 6a — paired with
/// `EXTINCTION_CONFIRMATION_TICKS` so a single bad tick can't kill
/// a species, but a sustained collapse does.
pub const EXTINCTION_THRESHOLD_FRAC: (i64, i64) = (1, 1000);

/// Number of consecutive ticks the per-species biomass must sit
/// below `EXTINCTION_THRESHOLD_FRAC × producer_capacity` before the
/// species is flagged extinct. `12` on monthly cadence ≈ one
/// sim-year — long enough that a single seasonal trough doesn't
/// trigger extinction, short enough that an actual collapse converts
/// to an extinction event within the run.
pub const EXTINCTION_CONFIRMATION_TICKS: u64 = 12;

/// Per-tick consumer respiration rate — fraction of consumer biomass
/// returned to atmospheric `CO2` each tick (Sprint 2 Item 6b). 1%/tick.
///
/// Mirror of the carbon side of the biogeochem loop: every consumer
/// (PrimaryConsumer, SecondaryConsumer, ApexConsumer, Detritivore,
/// Saprotroph, Mutualist, Parasite) respires a small fraction of its
/// biomass back to atmospheric `CO2` each tick. Producers don't
/// respire here — they're net carbon sinks (photosynthesis /
/// chemosynthesis) over the daily-averaged tick budget.
pub const RESPIRATION_RATE: (i64, i64) = (1, 100);

/// Per-tick decomposition rate — fraction of all extant species'
/// biomass that decomposers (Detritivore + Saprotroph) liberate to
/// atmospheric `CO2` each tick (Sprint 2 Item 6b). 0.5%/tick.
///
/// This represents the dead-biomass channel: at any given moment a
/// small fraction of every species' standing biomass is dead matter
/// being broken down. The decomposer chain returns that carbon to
/// the atmosphere. Drawn from total biomass (producers included);
/// rate is gated on the presence of at least one Detritivore or
/// Saprotroph.
pub const DECOMPOSITION_RATE: (i64, i64) = (1, 200);
