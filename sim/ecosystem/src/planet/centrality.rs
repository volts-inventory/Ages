//! Keystone-betweenness + interaction-graph centrality. Split out of
//! `planet.rs` in CB4.
//!
//! Houses [`PlanetEcosystem::keystone_species`] and
//! [`PlanetEcosystem::betweenness_centrality`]: Brandes' algorithm on
//! the unweighted, undirected interaction graph, with the
//! `KEYSTONE_CENTRALITY_THRESHOLD` normalised cutoff.

use sim_arith::Real;
use sim_species::SpeciesId;
use std::collections::{BTreeMap, BTreeSet};

use crate::constants::KEYSTONE_CENTRALITY_THRESHOLD;

use super::PlanetEcosystem;

impl PlanetEcosystem {
    /// Compute betweenness centrality over the interaction graph
    /// (treated undirected for keystone detection) and return any
    /// species whose normalised centrality exceeds the configured
    /// threshold.
    #[must_use]
    pub fn keystone_species(&self) -> BTreeSet<SpeciesId> {
        let centralities = self.betweenness_centrality();
        let n = self.species.len();
        if n < 3 {
            return BTreeSet::new();
        }
        // Maximum centrality for an undirected graph is
        // (n-1)(n-2)/2. Normalise then compare against threshold.
        let max_c = Real::from_int(((n - 1) * (n - 2) / 2) as i64);
        let threshold = Real::from(KEYSTONE_CENTRALITY_THRESHOLD);
        let mut out = BTreeSet::new();
        for (id, c) in centralities {
            if max_c > Real::ZERO {
                let normed = c / max_c;
                if normed >= threshold {
                    out.insert(id);
                }
            }
        }
        out
    }

    /// Compute betweenness centrality for every species via
    /// Brandes' algorithm on the unweighted, undirected interaction
    /// graph. Returns a `BTreeMap` so iteration order is stable.
    #[must_use]
    pub fn betweenness_centrality(&self) -> BTreeMap<SpeciesId, Real> {
        let mut adjacency: BTreeMap<SpeciesId, BTreeSet<SpeciesId>> = BTreeMap::new();
        for id in self.species.keys() {
            adjacency.insert(*id, BTreeSet::new());
        }
        for (a, b) in self.interactions.pairs.keys() {
            if !self.species.contains_key(a) || !self.species.contains_key(b) {
                continue;
            }
            adjacency.entry(*a).or_default().insert(*b);
            adjacency.entry(*b).or_default().insert(*a);
        }

        let ids: Vec<SpeciesId> = self.species.keys().copied().collect();
        let mut centrality: BTreeMap<SpeciesId, Real> =
            ids.iter().map(|id| (*id, Real::ZERO)).collect();

        // Brandes: for each source, do BFS, then back-accumulate.
        for s in &ids {
            // Predecessors of v on shortest paths from s.
            let mut preds: BTreeMap<SpeciesId, Vec<SpeciesId>> =
                ids.iter().map(|id| (*id, Vec::new())).collect();
            // sigma[v] = number of shortest paths from s to v.
            let mut sigma: BTreeMap<SpeciesId, i64> =
                ids.iter().map(|id| (*id, 0)).collect();
            sigma.insert(*s, 1);
            // dist[v] = shortest-path length s..v (negative = unset).
            let mut dist: BTreeMap<SpeciesId, i64> =
                ids.iter().map(|id| (*id, -1)).collect();
            dist.insert(*s, 0);

            let mut queue: std::collections::VecDeque<SpeciesId> =
                std::collections::VecDeque::new();
            queue.push_back(*s);
            let mut stack: Vec<SpeciesId> = Vec::new();

            while let Some(v) = queue.pop_front() {
                stack.push(v);
                let v_dist = *dist.get(&v).unwrap_or(&-1);
                if let Some(neighbours) = adjacency.get(&v) {
                    for w in neighbours {
                        let w_dist = *dist.get(w).unwrap_or(&-1);
                        if w_dist < 0 {
                            dist.insert(*w, v_dist + 1);
                            queue.push_back(*w);
                        }
                        if *dist.get(w).unwrap_or(&-1) == v_dist + 1 {
                            let new_sigma =
                                *sigma.get(w).unwrap_or(&0) + *sigma.get(&v).unwrap_or(&0);
                            sigma.insert(*w, new_sigma);
                            preds.entry(*w).or_default().push(v);
                        }
                    }
                }
            }

            // Back-accumulate dependencies.
            let mut delta: BTreeMap<SpeciesId, Real> =
                ids.iter().map(|id| (*id, Real::ZERO)).collect();
            while let Some(w) = stack.pop() {
                let sigma_w = *sigma.get(&w).unwrap_or(&1);
                let delta_w = *delta.get(&w).unwrap_or(&Real::ZERO);
                if let Some(pred_list) = preds.get(&w) {
                    for v in pred_list {
                        let sigma_v = *sigma.get(v).unwrap_or(&0);
                        if sigma_w > 0 {
                            let contribution = Real::from_ratio(sigma_v, sigma_w)
                                * (Real::ONE + delta_w);
                            let cur = *delta.get(v).unwrap_or(&Real::ZERO);
                            delta.insert(*v, cur + contribution);
                        }
                    }
                }
                if w != *s {
                    let cur = *centrality.get(&w).unwrap_or(&Real::ZERO);
                    centrality.insert(w, cur + delta_w);
                }
            }
        }

        // Undirected — divide by 2.
        for v in centrality.values_mut() {
            *v = *v / Real::from_int(2);
        }
        centrality
    }
}
