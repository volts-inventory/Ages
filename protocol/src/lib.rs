//! Wire protocol for events and snapshots emitted by the sim.
//!
//! Code-first: Rust types here are the source of truth; JSON Schemas
//! are derived via `schemars`. Consumers (post-run report, CLI stream
//! formatter, ad-hoc scripts) parse the NDJSON event log against
//! these types or against the published schemas.
//!
//! Event payload structs are split by domain: `header` (run-level),
//! `world_events` (planet/species/recognition), `civ_events`
//! (founding/collapse/contact/transmission/cosmology/cohesion/
//! catastrophe/tech), `discovery_events` (relations/measurements/
//! refinement/rivals/mythology), and `snapshot`. The `Event` enum
//! lives here so its variant order — the wire-format invariant —
//! stays in one canonical file.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

mod civ_events;
mod discovery_events;
mod header;
mod snapshot;
mod world_events;

/// `i128` `<-> JSON-string serde helpers. `serde`'s internal
/// `Content` buffer (used by `#[serde(tag = "...")]` and
/// `#[serde(untagged)]` enums during the tag-discovery phase) has
/// no I128 variant, so a plain numeric i128 in a tagged-enum
/// variant fails to round-trip with the error "i128 is not
/// supported". Encoding the raw bits as a decimal string side-
/// steps the buffer entirely — the wire format becomes a JSON
/// string of the signed decimal magnitude (e.g. "21474836480000")
/// which is portable across every JSON consumer (jq, Python json,
/// browsers, etc.) and survives the tagged-enum buffer.
///
/// Applied via `#[serde(with = "pop_bits_serde")]` on every
/// `i128` field that lives inside `#[serde(tag = "kind")] Event`.
/// `pop_bits_vec_serde` is the matching helper for `Vec<i128>`.
pub mod pop_bits_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &i128, s: S) -> Result<S::Ok, S::Error> {
        v.to_string().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<i128, D::Error> {
        let s = String::deserialize(d)?;
        s.parse::<i128>().map_err(serde::de::Error::custom)
    }
}

/// `Vec<i128>` <-> JSON-array-of-strings serde helper. See
/// [`pop_bits_serde`] for why string encoding is necessary inside
/// tagged enums.
pub mod pop_bits_vec_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &Vec<i128>, s: S) -> Result<S::Ok, S::Error> {
        let strs: Vec<String> = v.iter().map(i128::to_string).collect();
        strs.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<i128>, D::Error> {
        let strs: Vec<String> = Vec::deserialize(d)?;
        strs.iter()
            .map(|s| s.parse::<i128>().map_err(serde::de::Error::custom))
            .collect()
    }
}

pub use civ_events::*;
pub use discovery_events::*;
pub use header::*;
pub use snapshot::*;
pub use world_events::*;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    RunStart(RunHeader),
    RunMetadata(RunMetadata),
    Tick(TickEvent),
    Recognition(RecognitionFiring),
    Planet(PlanetDerived),
    PlanetMap(PlanetMap),
    /// Per-cell producer-life (vegetation) snapshot. Emitted once at
    /// run start after `PlanetMap`, then on a yearly cadence. Lets the
    /// live viewport tint land by actual producer biomass instead of
    /// elevation, so a baked or barren surface stops reading as green.
    CellBiomass(CellBiomass),
    Species(SpeciesDerived),
    FigureBorn(FigureBorn),
    TechUnlocked(TechUnlocked),
    CivFounded(CivFounded),
    CivTerritoryChanged(CivTerritoryChanged),
    CivCollapsed(CivCollapsed),
    KnowledgeTransmitted(KnowledgeTransmitted),
    CosmologyShifted(CosmologyShifted),
    /// A civ's three-axis religion / customs vector drifted
    /// at least 0.20 (L2 distance) from the last emitted snapshot.
    /// Fast-divergent layer separate from `CosmologyShifted`.
    ReligionShifted(ReligionShifted),
    /// A civ's life expectancy at birth changed by at least 2
    /// years since the last emission. Reflects the cumulative
    /// effect of tech (mortality reduction + lifespan extension),
    /// drift (cognition, sociality, lifespan), and species biology.
    CivLifeExpectancyChanged(CivLifeExpectancyChanged),
    CatastropheFired(CatastropheFired),
    CivContact(CivContact),
    ConflictResolved(ConflictResolved),
    /// Belligerence threshold crossed for a pair with
    /// existing territorial overlap and prior contact. Brackets
    /// the existing `ConflictResolved` skirmish stream.
    WarDeclared(WarDeclared),
    /// Belligerence dropped below the end threshold, or
    /// the loser surrendered all disputed cells, or overlap
    /// emptied without flips. Closes a war opened by a matching
    /// `WarDeclared`.
    PeaceConcluded(PeaceConcluded),
    KnowledgeDiffused(KnowledgeDiffused),
    RelationConfirmed(RelationConfirmed),
    MeasurementConfirmed(MeasurementConfirmed),
    RefinementProposed(RefinementProposed),
    RefinementConfirmed(RefinementConfirmed),
    RefinementRejected(RefinementRejected),
    RelationFalsified(RelationFalsified),
    RelationRevalidated(RelationRevalidated),
    RelationLapsed(RelationLapsed),
    Snapshot(Snapshot),
    /// Per-tick snapshot of the species' nomadic population.
    /// Cells with > floor population that are *not* claimed by any
    /// civ. The viewport renders these as `0` glyphs (nomads); a
    /// civ's BFS expansion absorbs nomads into its cohort.
    SpeciesNomadsChanged(SpeciesNomadsChanged),
    /// A civ's confirmed `ThresholdStep` law produced an
    /// emergent recognition template that joins the species
    /// canon. The template fires species-wide on subsequent
    /// ticks and is inheritable across civ boundaries.
    TemplateDiscovered(TemplateDiscovered),
    /// A civ's confirmed-relation cluster on a single
    /// channel produced an emergent dynamic tool. Joins the
    /// species `dynamic_tool_registry` + the civ's
    /// `unlocked_dynamic_tools`. Effects fold into the civ-level
    /// aggregators alongside static `ToolKind` tools.
    ToolDiscovered(ToolDiscovered),
    /// Per-seed cosmology pole-position bias declaration.
    /// Emitted once at run start, immediately after `Species`.
    /// Records the species' starting position on the five
    /// cultural axes (Empirical / Communitarian / Reformist /
    /// Mystical / Hierarchical) — derived from species traits +
    /// planet. Civs of this species inherit this
    /// vector as their initial cosmology rather than starting at
    /// neutral.
    SpeciesCosmologyBias(SpeciesCosmologyBias),
    /// Open lever-signature archetype classification, emitted once at
    /// run start from the world+species prior. The realized archetype
    /// refines as civs confirm relations and unlock tools.
    ArchetypeDerived(ArchetypeDerived),
    /// Archetype-specific civilizational endpoint at transcendence —
    /// different levers reach different fates.
    ArchetypeEndpoint(ArchetypeEndpoint),
    /// Per-civ species trait drift snapshot. Emitted at civ
    /// founding (refound, breakaway, or post-collapse emergent) when
    /// the inherited drift exceeds a half-step threshold on at
    /// least one of the four channels. Records the deltas relative
    /// to the species' baseline so consumers can show "this civ's
    /// effective cognition is +0.06 above species norm."
    ///
    /// All four scalars are Q32.32 raw bits; consumers convert via
    /// `i64 as f64 / 2^32`. Lifespan is in years (signed); cognition,
    /// sociality, and communication-fidelity are unit-range.
    SpeciesDrift(SpeciesDrift),
    /// A civ's internal cohesion crossed a meaningful
    /// (≥ 0.05 absolute) threshold since the last emission.
    /// Cohesion is a `[0, 1]` scalar tracking the polity's
    /// ability to hold itself together — drifts based on civ size,
    /// food security, dogmatism, and literacy. When it stays below
    /// `CIVIL_WAR_COHESION_FLOOR` (0.10) for
    /// `CIVIL_WAR_STREAK_TICKS` (75 years) the civ collapses with
    /// reason `civil_war`.
    CohesionShifted(CohesionShifted),
    /// M8 — civ's economic surplus accumulator shifted by at
    /// least `SURPLUS_EMIT_DELTA_FLOOR` pop-equivalents since the
    /// last emission. Buffer of stored productive output;
    /// modulates food-crisis collapse + war strength + catastrophe
    /// resilience.
    CivSurplusChanged(CivSurplusChanged),
    /// P0.5 — civ ecological resilience drifted by at least
    /// `RESILIENCE_EMIT_DELTA_FLOOR` (currently 0.05) since the
    /// last emission. Tracks `producer_biomass /
    /// initial_producer_biomass`, clamped to `[0, 2]`. Lets
    /// consumers timeline ecosystem-civ coupling — a planet whose
    /// producers crash via cascading extinctions now visibly
    /// degrades every civ's resilience over the same tick window.
    CivResilienceTick(CivResilienceTick),
    /// M8 — trade route opened between two peaceful civs. Per-tick
    /// surplus flow runs between the pair until war / collapse /
    /// hierarchy-drift closes it.
    TradeRouteEstablished(TradeRouteEstablished),
    /// M8 — trade route closed. See `TradeRouteClosed::reason` for
    /// the trigger.
    TradeRouteClosed(TradeRouteClosed),
    /// Mutual alliance formed between two civs. Both satisfied
    /// the cumulative criteria (cosmology + religion proximity,
    /// prior peaceful contact). Allied pairs have
    /// `conflict::resolve` short-circuit, so they can share
    /// borders without war.
    AllianceFormed(AllianceFormed),
    /// Alliance between two civs dissolved. Reason indicates
    /// which dissolution rule fired (drift, war misalignment, or
    /// trust erosion).
    AllianceDissolved(AllianceDissolved),
    /// Sprint 2 Item 6a — the ecosystem step flagged a species
    /// extinct after its biomass / population pool stayed below
    /// `EXTINCTION_THRESHOLD` for
    /// `EXTINCTION_CONFIRMATION_TICKS` consecutive ticks. The
    /// species record stays in the per-planet registry for
    /// history / replay determinism but is skipped by subsequent
    /// ecosystem ticks.
    SpeciesExtinct(SpeciesExtinct),
    /// Sprint 3 Item 11a — a horizontal-gene-transfer trial
    /// succeeded between two co-located `Lifecycle::Microbial`
    /// species. The recipient species' `trait_swapped` axis was
    /// nudged a small fraction toward the donor's value. Emitted
    /// in `(donor_id, recipient_id)`-sorted order per tick.
    HorizontalGeneTransfer(HgtEvent),
    /// Sprint 3 Item 11 — the ecosystem speciation step generated
    /// a new daughter species from a parent under one of five
    /// triggers (allopatric / sympatric / polyploid / founder
    /// effect / post-extinction radiation). The daughter inherits
    /// parent traits with correlated allometric drift; the parent
    /// stays extant. Speciation and extinction together bound
    /// per-planet registry growth.
    SpeciationOccurred(SpeciationEvent),
    /// A relation almost crossed the inter-civ
    /// transmission gate but didn't quite — the comprehension
    /// score landed in the mythologization band so the relation's
    /// content didn't transfer as confirmed knowledge but its
    /// *meaning* perturbed the receiving civ's cosmology along one
    /// of the five axes. Real-history analogue: a society that
    /// lost the original physics of a phenomenon retains the
    /// taboo, ritual, or sacred reverence around it.
    RelationMythologized(RelationMythologized),
    /// An alternative fit was proposed as a competing
    /// hypothesis for an already-confirmed `relation_id`. The
    /// primary remains in place; the rival sits in the
    /// `Hypothesizer::rivals` list and may displace the primary
    /// later via `displace_primary_with_best_rival`. Real-history
    /// analogue: phlogiston vs oxygen, geocentric vs heliocentric,
    /// miasma vs germ — multiple theories can coexist before one
    /// displaces the other.
    RivalHypothesisProposed(RivalHypothesisProposed),
    /// A rival hypothesis displaced the primary fit for a
    /// `relation_id` because its confidence was higher. The
    /// previous primary returns to the rivals list (so future
    /// swaps can flip back).
    PrimaryHypothesisDisplaced(PrimaryHypothesisDisplaced),
    RunEnd {
        tick: u64,
        reason: String,
    },
}
