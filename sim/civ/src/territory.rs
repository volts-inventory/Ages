//! Per-cell cohort distribution + territory claim
//! reshaping + inter-cell migration. The aggregate `cohort` stays
//! in sync with the sum of `region_cohorts` (per bracket) so
//! collapse + capacity readers see a consistent mid-tick view.
//! Migration follows the family-unit policy: only fertile adults
//! and their proportional dependents (infants + juveniles) move
//! cell-to-cell; elders are too rooted/senescent to migrate and
//! stay put when their cell sheds population.

use crate::{catastrophe, Civ};
use sim_arith::{Pop, Real};
use sim_population::Cohort;
use std::collections::{BTreeMap, BTreeSet};

impl Civ {
    /// + : claim a set of cells.
    ///
    /// ** update:** prior behaviour was to wipe all
    /// `region_cohorts` and re-seed every claimed cell uniformly
    /// (`cohort.count / n_cells`). That made every cell of a civ
    /// have identical population at all times.
    ///
    /// New behaviour:
    /// - Cells in the new set AND in the old set keep their
    ///   existing cohort intact (heterogeneous distribution
    ///   survives expansion).
    /// - Cells in the new set but NOT the old set seed from the
    ///   civ's centroid: 10% of the centroid cohort transfers to
    ///   the new cell, capped at 100 individuals so a single
    ///   expansion doesn't drain the capital. Models "the capital
    ///   sends colonists to the frontier".
    /// - Cells in the old set but NOT the new (shed during
    ///   contraction) drop out of `region_cohorts`. Their pop
    ///   becomes lost in this pass; migration will route
    ///   them to interior cells.
    ///
    /// First-claim case (founding): no prior cohorts, all cells
    /// are "new". Falls back to uniform seeding from
    /// `self.cohort.count` so a freshly-constructed civ doesn't
    /// silently lose its founding population.
    pub fn claim_cells(&mut self, cells: &BTreeSet<u32>) {
        let n = cells.len();
        let n_u32 = u32::try_from(n).unwrap_or(u32::MAX);
        if n_u32 > self.peak_claimed_cells {
            self.peak_claimed_cells = n_u32;
        }

        // First-claim case: no prior cohorts. Concentrate the
        // founding band in the centroid (the "homeland") and seed
        // outlying cells thinly. Earlier behaviour spread `count /
        // n` uniformly across every claimed cell, which under the
        // per-cell-capacity model loaded each cell well above its
        // real `cell_capacity` on fuel-poor seeds — logistic
        // dynamics then squeezed pop back to per-cell ceiling and
        // the civ's aggregate dropped sharply in the first ten
        // ticks. The historical "civilization radiates from a
        // homeland" pattern (Çatalhöyük → valley → frontier) maps
        // naturally onto centroid-heavy founding: the capital
        // starts dense, frontier cells start sparse, and migration
        // smooths out heterogeneity over the first sim-decades.
        //
        // 70% to centroid, 30% spread across other cells. If the
        // centroid happens not to be in the claim set (e.g.
        // mid-founding relocation edge case), fall back to uniform.
        if self.region_cohorts.is_empty() {
            self.claimed_cells.clone_from(cells);
            if cells.is_empty() {
                return;
            }
            let centroid = self.territory_centroid;
            if cells.contains(&centroid) && cells.len() > 1 {
                let centroid_share = Real::percent(70);
                let mut centroid_cohort = Cohort::with_civ(Pop::ZERO, self.id);
                centroid_cohort.merge_in(&self.cohort);
                centroid_cohort.scale_in_place(centroid_share);
                let mut others_pool = Cohort::with_civ(Pop::ZERO, self.id);
                others_pool.merge_in(&self.cohort);
                others_pool.scale_in_place(Real::ONE - centroid_share);
                let other_n_count = i64::try_from(cells.len() - 1).unwrap_or(1);
                let other_n = Real::from_int(other_n_count.max(1));
                let per_other_factor = Real::ONE / other_n;
                for &cell in cells {
                    if cell == centroid {
                        let mut c = Cohort::empty_with_civ(self.id);
                        c.merge_in(&centroid_cohort);
                        self.region_cohorts.insert(cell, c);
                    } else {
                        let mut c = Cohort::empty_with_civ(self.id);
                        let mut share = Cohort::empty();
                        share.merge_in(&others_pool);
                        share.scale_in_place(per_other_factor);
                        c.merge_in(&share);
                        self.region_cohorts.insert(cell, c);
                    }
                }
            } else {
                let n_real = Real::from_int(i64::try_from(n).unwrap_or(i64::MAX).max(1));
                let per_cell_factor = Real::ONE / n_real;
                for &cell in cells {
                    let mut c = Cohort::empty_with_civ(self.id);
                    let mut share = Cohort::empty();
                    share.merge_in(&self.cohort);
                    share.scale_in_place(per_cell_factor);
                    c.merge_in(&share);
                    self.region_cohorts.insert(cell, c);
                }
            }
            return;
        }

        // path: preserve existing cohorts; seed new cells
        // from centroid; redistribute shed-cell pop to retained
        // cells so contraction doesn't leak population.
        let old_cells = self.claimed_cells.clone();
        self.claimed_cells.clone_from(cells);

        // Gather shed-cell cohorts as a refugee pool, preserving
        // bracket structure. Earlier path summed only scalar count
        // and re-deposited as fertile; under the 4-bracket model
        // we keep the age structure of the people leaving so
        // refugees arriving in a retained cell carry their infants
        // and elders with them.
        let mut refugee_pool = Cohort::empty_with_civ(self.id);
        for cell in old_cells.difference(cells) {
            if let Some(c) = self.region_cohorts.remove(cell) {
                refugee_pool.merge_in(&c);
            }
        }

        // Redistribute refugees uniformly across retained cells —
        // each bracket scaled by `1 / n_retained`.
        let retained: Vec<u32> = old_cells.intersection(cells).copied().collect();
        if !retained.is_empty() && refugee_pool.total() > Pop::ZERO {
            let share_factor =
                Real::ONE / Real::from_int(i64::try_from(retained.len()).unwrap_or(1).max(1));
            for cell in &retained {
                if let Some(c) = self.region_cohorts.get_mut(cell) {
                    let mut share = Cohort::empty();
                    share.merge_in(&refugee_pool);
                    share.scale_in_place(share_factor);
                    c.merge_in(&share);
                }
            }
        }

        // Seed new cells (in new, not in old) from centroid via
        // `split_off_fraction` so the seed carries every bracket
        // proportionally, not just fertile adults. Cap the per-cell
        // fraction so a single expansion can't drain the capital.
        let new_cells: Vec<u32> = cells.difference(&old_cells).copied().collect();
        if !new_cells.is_empty() {
            let centroid = self.territory_centroid;
            let n_new = i64::try_from(new_cells.len()).unwrap_or(0).max(1);
            // 10% of centroid per new cell, capped at 80% total so
            // we never drain the capital below 20%.
            let per_cell_fraction = Real::percent(10);
            let total_fraction = (per_cell_fraction * Real::from_int(n_new)).min(Real::percent(80));
            let actual_per_cell = total_fraction / Real::from_int(n_new);
            // Slice the centroid in one operation.
            let mut centroid_seed = self
                .region_cohorts
                .get_mut(&centroid)
                .map_or_else(Cohort::empty, |c| c.split_off_fraction(total_fraction));
            // Distribute the seed across the new cells. Each cell
            // gets `actual_per_cell / total_fraction` fraction of
            // the centroid_seed pool.
            let per_seed_share = if total_fraction > Real::ZERO {
                actual_per_cell / total_fraction
            } else {
                Real::ZERO
            };
            for cell in new_cells {
                let mut share_pool = Cohort::empty();
                share_pool.merge_in(&centroid_seed);
                let mut moved = share_pool.split_off_fraction(per_seed_share);
                centroid_seed.scale_in_place(Real::ONE - per_seed_share);
                moved.civ_membership = Some(self.id);
                self.region_cohorts.insert(cell, moved);
            }
        }

        // keep cohort.count consistent with the redistributed
        // region_cohorts sum so check_collapse + carrying_capacity
        // see the right aggregate immediately after expansion or
        // contraction (the next per-cell step also rederives this,
        // but tests + intermediate readers shouldn't see a stale
        // mid-tick aggregate).
        self.resync_aggregate_from_regions();
    }

    /// Re-derive every aggregate-cohort bracket from the per-cell
    /// `region_cohorts` sums. Called after any path that mutates
    /// `region_cohorts` outside the per-cell step (territory
    /// reshape, prune, expansion, war-casualty bracket adjustment).
    pub fn resync_aggregate_from_regions(&mut self) {
        self.cohort.infant = self
            .region_cohorts
            .values()
            .map(|c| c.infant)
            .fold(Pop::ZERO, |a, b| a + b);
        self.cohort.juvenile = self
            .region_cohorts
            .values()
            .map(|c| c.juvenile)
            .fold(Pop::ZERO, |a, b| a + b);
        self.cohort.fertile = self
            .region_cohorts
            .values()
            .map(|c| c.fertile)
            .fold(Pop::ZERO, |a, b| a + b);
        self.cohort.elder = self
            .region_cohorts
            .values()
            .map(|c| c.elder)
            .fold(Pop::ZERO, |a, b| a + b);
    }

    /// Reduce the given cell's region cohort by `fraction`
    /// (clamped to `[0, 1]`), scaling every bracket equally so the
    /// age structure survives. Returns the population lost so
    /// catastrophe events can carry the magnitude. The aggregate
    /// cohort is re-synced after the loss.
    pub fn drop_cell_pop(&mut self, cell: u32, fraction: Real) -> Pop {
        let frac = fraction.clamp01();
        let mut lost = Pop::ZERO;
        if let Some(c) = self.region_cohorts.get_mut(&cell) {
            let before = c.total();
            c.scale_in_place(Real::ONE - frac);
            lost = before - c.total();
        }
        // Mirror the loss in the aggregate so consumers stay in sync.
        self.resync_aggregate_from_regions();
        lost
    }

    /// inter-cell migration. Each tick, after the per-cell
    /// pop step, redistribute pop from high-pressure cells
    /// (count above 85% of `cell_capacity`) toward adjacent
    /// claimed cells with headroom (count below `cell_capacity`).
    /// Models pre-emptive migration: people leave cities before
    /// food security drops to crisis levels.
    ///
    /// Uses two-phase update so cell-iteration order doesn't bias
    /// the result: pass 1 computes per-source-cell deltas, pass 2
    /// applies them atomically. Determinism preserved.
    pub fn migrate_inter_cell(
        &mut self,
        state: &sim_physics::PhysicsState,
        tick: u64,
        planet: &sim_world::Planet,
        grid_width: u32,
        grid_height: u32,
    ) {
        if self.region_cohorts.is_empty() {
            return;
        }
        // Tech-augmented threshold: a high-tech civ tolerates a
        // denser core before pushing migrants outward — frontier
        // expansion is a resource-poor response. See
        // `tech_augmented_migration_threshold` for the formula.
        let pressure_threshold = crate::demographics::tech_augmented_migration_threshold(
            self.migration_pressure_threshold,
            self.tool_capacity_multiplier(),
        );
        // Base 5%-per-tick rate scaled by tools that accelerate
        // intra-civ population redistribution (transport, navigation,
        // logistics coordination). `tool_migration_speed_bonus` is
        // capped at +1.00 so the rate can at most double from base.
        let migration_rate = Real::percent(5) * (Real::ONE + self.tool_migration_speed_bonus());

        let caps: BTreeMap<u32, Pop> = self
            .region_cohorts
            .keys()
            .copied()
            .map(|cell| (cell, self.cell_capacity(state, cell, tick, planet)))
            .collect();

        // Two-phase: pass 1 computes per-source decisions
        // (target neighbours + per-neighbour fertile-to-move),
        // pass 2 applies them via `migrate_family_to` so each
        // moved adult drags their proportional dependents (infants
        // + juveniles) but elders stay rooted. Determinism is
        // preserved by `BTreeMap` iteration order.
        let mut moves: Vec<(u32, u32, Pop)> = Vec::new();
        for (&cell, cohort) in &self.region_cohorts {
            let cap = caps.get(&cell).copied().unwrap_or(Pop::ZERO);
            if cap <= Pop::ZERO {
                continue;
            }
            let pressure_count = cap * pressure_threshold;
            if cohort.total() <= pressure_count {
                continue;
            }
            // Only fertile + dependents migrate (option 2.b);
            // elders stay. Cap "movable adults" at the source's
            // fertile bracket so we never try to send more adults
            // than the source has.
            let overflow = cohort.total() - pressure_count;
            let movable_fertile = overflow.min(cohort.fertile);
            if movable_fertile <= Pop::ZERO {
                continue;
            }
            // Compute per-neighbour headroom over claimed
            // neighbours. Headroom is per-cell capacity minus the
            // neighbour's *total* current pop (all brackets count
            // toward filling capacity).
            let nbrs = catastrophe::hex_neighbors(cell, grid_width, grid_height);
            let mut headroom_total = Pop::ZERO;
            let mut nbr_headrooms: Vec<(u32, Pop)> = Vec::with_capacity(6);
            for nbr in nbrs {
                if !self.claimed_cells.contains(&nbr) {
                    continue;
                }
                let n_cap = caps.get(&nbr).copied().unwrap_or(Pop::ZERO);
                let n_count = self
                    .region_cohorts
                    .get(&nbr)
                    .map_or(Pop::ZERO, sim_population::Cohort::total);
                let h = n_cap - n_count;
                if h > Pop::ZERO {
                    headroom_total = headroom_total + h;
                    nbr_headrooms.push((nbr, h));
                }
            }
            if headroom_total <= Pop::ZERO || nbr_headrooms.is_empty() {
                continue;
            }
            // Total fertile move: 5% of fertile-overflow, capped at
            // total headroom. Note: the cap is in *people* (total),
            // so a fertile-only move still respects whole-cohort
            // capacity since dependents come along on top.
            let want = movable_fertile * migration_rate;
            let total_move_f = if want > headroom_total {
                headroom_total
            } else {
                want
            };
            // Distribute proportionally to neighbour headroom.
            for (nbr, h) in nbr_headrooms {
                let share = total_move_f * (h / headroom_total);
                if share > Pop::ZERO {
                    moves.push((cell, nbr, share));
                }
            }
        }
        // Pass 2: apply via family-aware migration. We need to
        // borrow two cohorts at once, so resolve via remove + insert.
        for (src_cell, dst_cell, fertile_amount) in moves {
            if src_cell == dst_cell {
                continue;
            }
            let Some(mut src) = self.region_cohorts.remove(&src_cell) else {
                continue;
            };
            let Some(mut dst) = self.region_cohorts.remove(&dst_cell) else {
                // Source was removed by an earlier move that
                // happened to drain it; restore and skip.
                self.region_cohorts.insert(src_cell, src);
                continue;
            };
            src.migrate_family_to(&mut dst, fertile_amount);
            self.region_cohorts.insert(src_cell, src);
            self.region_cohorts.insert(dst_cell, dst);
        }
    }

    /// Per-cell capacity-driven territorial expansion. Replaces the
    /// older `target_cell_count(pop) → BFS-from-centroid` model
    /// (which used a flat `PEOPLE_PER_CELL = 5` and produced uniform
    /// round territories regardless of where the fertile cells
    /// actually were).
    ///
    /// Iterates each high-pressure claimed cell (count ≥
    /// `migration_pressure_threshold × cell_capacity`); when an
    /// unclaimed habitable neighbour exists with positive
    /// `cell_capacity`, the civ claims its highest-cap neighbour
    /// and seeds it with a slice of the source cell's overflow.
    ///
    /// `cells_claimed_by_others` is the union of *other* civs'
    /// `claimed_cells` — the new claim never trespasses on an
    /// already-occupied cell.
    ///
    /// Per-tick expansion budget: `1 +
    /// floor(10 × tool_expansion_rate_bonus())`. A foot civ claims
    /// at most one new cell per tick; navigation/transport tools
    /// raise the ceiling so industrial-era civs spread faster.
    /// Returns the new cells claimed this tick (in claim order).
    pub fn expand_via_overflow(
        &mut self,
        state: &sim_physics::PhysicsState,
        tick: u64,
        planet: &sim_world::Planet,
        grid_width: u32,
        grid_height: u32,
        cells_claimed_by_others: &BTreeSet<u32>,
    ) -> Vec<u32> {
        let mut newly_claimed: Vec<u32> = Vec::new();
        if self.region_cohorts.is_empty() {
            return newly_claimed;
        }
        // Substrate-derived claim cadence: on Aqueous (metabolism=1)
        // we attempt expansion every tick; on slow substrates the
        // cadence stretches so claim activity tracks the planet's
        // biological time. Tick 0 never claims (consistent with the
        // pre-existing `tick > 0` invariants elsewhere).
        let metabolism = planet.metabolic_substrate.metabolism();
        let cadence = crate::demographics::streak_ticks_for_metabolism(1, metabolism);
        if cadence > 1 && !tick.is_multiple_of(cadence) {
            return newly_claimed;
        }
        // Per-tick claim ceiling: 1 cell as a baseline, plus
        // `floor(10 × tool_expansion_rate_bonus)` extras. A vanilla
        // civ claims ≤ 1 new cell per tick; HeavyTransport (+0.20
        // expansion) gives ≤ 3, OrbitalReach (+0.30) ≤ 4, etc. Q32.32
        // raw → integer floor via `bits >> 32`; non-negative inputs
        // (`tool_expansion_rate_bonus` is always ≥ 0).
        let scaled = self.tool_expansion_rate_bonus() * Real::from_int(10);
        let scaled_floor = u64::try_from((scaled.raw().to_bits() >> 32).max(0)).unwrap_or(0);
        let budget: u64 = 1u64.saturating_add(scaled_floor);
        // Same tech-augmented threshold as `migrate_intra_civ` —
        // high-tech civs spill over later (denser cores, less
        // frontier-grab pressure per tick).
        let pressure_threshold = crate::demographics::tech_augmented_migration_threshold(
            self.migration_pressure_threshold,
            self.tool_capacity_multiplier(),
        );
        let seed_fraction = Real::percent(20);

        let caps: BTreeMap<u32, Pop> = self
            .region_cohorts
            .keys()
            .copied()
            .map(|cell| (cell, self.cell_capacity(state, cell, tick, planet)))
            .collect();

        // Walk source cells in deterministic order; stop when the
        // budget is exhausted. `BTreeMap` iteration is sorted by
        // key, so cell-id order keeps determinism.
        let source_order: Vec<u32> = self.region_cohorts.keys().copied().collect();
        let mut spent: u64 = 0;
        for cell in source_order {
            if spent >= budget {
                break;
            }
            let cap = caps.get(&cell).copied().unwrap_or(Pop::ZERO);
            if cap <= Pop::ZERO {
                continue;
            }
            let count = self
                .region_cohorts
                .get(&cell)
                .map_or(Pop::ZERO, sim_population::Cohort::total);
            let fertile_count = self
                .region_cohorts
                .get(&cell)
                .map_or(Pop::ZERO, |c| c.fertile);
            let pressure_count = cap * pressure_threshold;
            if count <= pressure_count {
                continue;
            }
            // Pick the highest-cap unclaimed habitable neighbour.
            let nbrs = catastrophe::hex_neighbors(cell, grid_width, grid_height);
            let mut best: Option<(u32, Pop)> = None;
            for nbr in nbrs {
                if self.claimed_cells.contains(&nbr) {
                    continue;
                }
                if cells_claimed_by_others.contains(&nbr) {
                    continue;
                }
                let nbr_cap = self.cell_capacity(state, nbr, tick, planet);
                if nbr_cap <= Pop::ZERO {
                    continue;
                }
                if best.is_none_or(|(_, b)| nbr_cap > b) {
                    best = Some((nbr, nbr_cap));
                }
            }
            let Some((target_cell, _)) = best else {
                continue;
            };
            // Move 20% of source overflow (in fertile-equivalent
            // people, since only fertile + dependents found new
            // cells; elders stay) as the founding seed — enough
            // that the new frontier cell isn't instantly pruned,
            // but small enough that the source cell remains dense.
            let overflow = count - pressure_count;
            let seed_fertile = (overflow * seed_fraction).min(fertile_count);
            if seed_fertile <= Pop::ZERO {
                continue;
            }
            // Drain the source via family-aware migration into a
            // fresh cohort representing the founding band.
            let mut new_cohort = Cohort::empty_with_civ(self.id);
            if let Some(c) = self.region_cohorts.get_mut(&cell) {
                c.migrate_family_to(&mut new_cohort, seed_fertile);
            }
            // Claim + seed the target.
            self.claimed_cells.insert(target_cell);
            self.region_cohorts.insert(target_cell, new_cohort);
            let n = u32::try_from(self.claimed_cells.len()).unwrap_or(u32::MAX);
            if n > self.peak_claimed_cells {
                self.peak_claimed_cells = n;
            }
            newly_claimed.push(target_cell);
            spent = spent.saturating_add(1);
        }
        // Resync aggregate.
        if !newly_claimed.is_empty() {
            self.resync_aggregate_from_regions();
        }
        newly_claimed
    }

    /// Drop claimed cells whose region cohort has fallen below a
    /// trivial floor. Replaces the aggregate `target_cell_count`
    /// shrink path. The civ's centroid is never pruned (a civ with
    /// a single near-empty centroid is the collapse path's
    /// responsibility).
    /// Returns the cells removed this tick.
    pub fn prune_empty_cells(&mut self) -> Vec<u32> {
        let floor = Pop::from_ratio(1, 10); // 0.1 person across all brackets
        let mut removed: Vec<u32> = Vec::new();
        let candidates: Vec<u32> = self
            .region_cohorts
            .iter()
            .filter(|(&c, cohort)| c != self.territory_centroid && cohort.total() < floor)
            .map(|(&c, _)| c)
            .collect();
        for cell in candidates {
            self.region_cohorts.remove(&cell);
            self.claimed_cells.remove(&cell);
            removed.push(cell);
        }
        if !removed.is_empty() {
            self.resync_aggregate_from_regions();
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_physics::{HexGrid, PhysicsState, Substance};
    use sim_world::sample_planet;

    fn well_fed_state(width: u32, height: u32) -> PhysicsState {
        let mut s = PhysicsState::new(HexGrid::new(width, height));
        for v in s.substance_mut(Substance::Fuel.idx()) {
            *v = Real::from_int(10);
        }
        s
    }

    #[test]
    fn expand_claims_neighbour_when_centroid_overflows() {
        // Civ with one centroid cell at saturation should claim
        // exactly one neighbour per tick (default budget = 1).
        // Cell cap at fuel=10 × per_unit=50,000 ≈ 500k per cell, so
        // we set fertile to 1M to push well above the migration
        // pressure threshold (85% of cap by default).
        let state = well_fed_state(8, 8);
        let planet = sample_planet(1);
        let mut civ = Civ::new(1, 0, Pop::from_int(1_000_000));
        civ.territory_centroid = 27; // interior cell
        let init: BTreeSet<u32> = std::iter::once(27u32).collect();
        civ.claim_cells(&init);
        // Force pressure: bump the centroid cohort high above cap.
        if let Some(c) = civ.region_cohorts.get_mut(&27) {
            c.fertile = Pop::from_int(1_000_000);
        }
        let gained = civ.expand_via_overflow(&state, 0, &planet, 8, 8, &BTreeSet::new());
        assert_eq!(gained.len(), 1, "default budget is one cell per tick");
        assert!(civ.claimed_cells.contains(&gained[0]));
    }

    #[test]
    fn expand_does_not_trespass_on_other_civs() {
        let state = well_fed_state(8, 8);
        let planet = sample_planet(1);
        let mut civ = Civ::new(1, 0, Pop::from_int(1_000_000));
        civ.territory_centroid = 27;
        let init: BTreeSet<u32> = std::iter::once(27u32).collect();
        civ.claim_cells(&init);
        if let Some(c) = civ.region_cohorts.get_mut(&27) {
            c.fertile = Pop::from_int(1_000_000);
        }
        // Block every neighbour of cell 27 by claiming each via a
        // hypothetical second civ.
        let nbrs = catastrophe::hex_neighbors(27, 8, 8);
        let others: BTreeSet<u32> = nbrs.iter().copied().collect();
        let gained = civ.expand_via_overflow(&state, 0, &planet, 8, 8, &others);
        assert!(gained.is_empty(), "no neighbour available, no claim");
        assert_eq!(civ.claimed_cells.len(), 1);
    }

    #[test]
    fn expand_skips_low_pressure_civs() {
        let state = well_fed_state(8, 8);
        let planet = sample_planet(1);
        let mut civ = Civ::new(1, 0, Pop::from_int(50));
        civ.territory_centroid = 27;
        let init: BTreeSet<u32> = std::iter::once(27u32).collect();
        civ.claim_cells(&init);
        // Cohort at 50 — well below `cell_capacity` for a fuel-10
        // cell (cap ≈ fuel × per_unit ≈ 10 × 50 = 500). No
        // pressure → no expansion.
        let gained = civ.expand_via_overflow(&state, 0, &planet, 8, 8, &BTreeSet::new());
        assert!(gained.is_empty());
    }

    #[test]
    fn prune_drops_empty_cells_keeps_centroid() {
        let mut civ = Civ::new(1, 0, Pop::from_int(50));
        let cells: BTreeSet<u32> = [0u32, 1, 2, 3].iter().copied().collect();
        civ.claim_cells(&cells);
        civ.territory_centroid = 0;
        // Drain cells 1+2 to ~0; leave centroid + cell 3 healthy.
        if let Some(c) = civ.region_cohorts.get_mut(&1) {
            *c = Cohort::empty_with_civ(civ.id);
        }
        if let Some(c) = civ.region_cohorts.get_mut(&2) {
            *c = Cohort::empty_with_civ(civ.id);
        }
        // Force centroid to zero too — should stay claimed despite
        // the floor check.
        if let Some(c) = civ.region_cohorts.get_mut(&0) {
            *c = Cohort::empty_with_civ(civ.id);
        }
        let removed = civ.prune_empty_cells();
        assert!(removed.contains(&1));
        assert!(removed.contains(&2));
        assert!(!removed.contains(&0), "centroid never pruned");
        assert!(civ.claimed_cells.contains(&0));
        assert!(civ.claimed_cells.contains(&3));
        assert!(!civ.claimed_cells.contains(&1));
    }
}
