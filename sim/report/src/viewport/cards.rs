//! Formatted info-cards that ride the top of the viewport: the
//! planet card (world stats — type, climate, orbital mechanics) and
//! the species card (cognition, senses, biology). Both are pure
//! formatters that read snapshot state captured by `apply_state`
//! and return a multi-line `String` (or `None` while the
//! underlying event hasn't arrived yet).

use super::emitter::ViewportEmitter;
use crate::labels::{
    atmosphere_descriptor, cog_tier, comm_label, format_atmospheric_composition, friendly_badge,
    host_species_status, planet_archetype, short_manip, short_modality, sociality_label,
    substrate_biochem,
};
use std::fmt::Write as FmtWrite;
use std::io::Write;

impl<W: Write> ViewportEmitter<W> {
    /// Format the planet card for the top of the viewport. Two
    /// short lines of compact stats, each ≤ 32 chars so the card
    /// fits on portrait phone terminals (iPhone Termius narrowest
    /// column is ~30; this leaves 2 chars margin). Static for the
    /// run since `Planet` is emitted once.
    pub(super) fn planet_card(&self) -> Option<String> {
        use crate::q32::q32_to_f64;
        let p = self.planet.as_ref()?;
        let mut s = String::new();
        // Card layout groups thematically-related fields per
        // line. Planet name lives in the section divider (rendered
        // by `render()`); species + bio info lives in a dedicated
        // species section (see `species_card()`). The planet card
        // covers only the *world* — type, climate, orbital
        // mechanics.
        let mean_t_k = q32_to_f64(p.mean_temperature_q32);
        // Substrate freeze/boil come from the captured
        // `RunMetadata` event (sourced upstream from
        // `sim_physics::chemistry::substrate_properties`).
        // Apply the per-seed perturbation
        // (`p.substrate_perturbation_q32`) so the displayed values
        // match what `Chemistry::for_planet_with_perturbation`
        // actually wired into the run's physics. Without this the
        // card showed water freezing at 273.15 K every seed even
        // though seed-42's effective freeze point might be 271.7 K.
        let perturb = q32_to_f64(p.substrate_perturbation_q32);
        let (freeze_k, boil_k) = self.metadata.as_ref().map_or((0.0, 0.0), |m| {
            let nominal_freeze = m
                .substrate_freeze_k
                .get(&p.metabolic_substrate)
                .copied()
                .unwrap_or(0.0);
            let nominal_boil = m
                .substrate_boil_k
                .get(&p.metabolic_substrate)
                .copied()
                .unwrap_or(0.0);
            (
                nominal_freeze * (1.0 + perturb),
                nominal_boil * (1.0 + perturb),
            )
        });
        let badge = host_species_status(
            &p.metabolic_substrate,
            &p.atmosphere,
            mean_t_k,
            freeze_k,
            boil_k,
        );
        let badge_friendly = friendly_badge(badge);
        // Compute actual surface ocean fraction from the per-cell
        // water-depth grid; substrate alone over-labels every aqueous-
        // biology planet as "ocean world" even when 0 % of its
        // surface holds liquid water (e.g. seed 42).
        let ocean_frac = self.planet_map.as_ref().map_or(0.0, |pm| {
            let total = pm.water_depth_q32.len();
            if total == 0 {
                0.0
            } else {
                let wet = pm.water_depth_q32.iter().filter(|&&d| d > 0).count();
                wet as f64 / total as f64
            }
        });
        let terrain_peak_m = q32_to_f64(p.terrain_peak_q32);
        let ptype = planet_archetype(
            p.metabolic_substrate.as_str(),
            mean_t_k,
            freeze_k,
            boil_k,
            terrain_peak_m,
            ocean_frac,
        );
        // Line 1: archetype noun · friendly badge — leads with the
        // surface-aware archetype (`ocean world` only when there
        // really is one) and follows with a one-word habitability
        // descriptor (e.g. `desert world · scorching`).
        let _ = writeln!(s, "{ptype} · {badge_friendly}");
        // Line 2 (climate): atmosphere · temperature ·
        // magnetosphere — the three "what's the air / sky like"
        // fields. `none` magnetosphere collapses to `no`
        // (reads better than "none mag").
        let mean_t_display = self.cfg.temperature_unit.from_kelvin(mean_t_k);
        let temp_suffix = self.cfg.temperature_unit.suffix();
        let mag_label = if p.magnetosphere == "none" {
            "no"
        } else {
            p.magnetosphere.as_str()
        };
        let atm_desc = atmosphere_descriptor(p.atmosphere.as_str());
        let _ = writeln!(
            s,
            "{atm_desc} · {mean_t_display:.0}{temp_suffix} · {mag_label} mag",
        );
        // Atmospheric composition — top three channels by
        // mass fraction, e.g. `78%N₂ 21%O₂ 1%Ar`. Skipped on
        // vacuum (sum ≈ 0). Older event logs default all
        // composition channels to 0 and fall through to vacuum.
        if let Some(line) = format_atmospheric_composition(p) {
            let _ = writeln!(s, "{line}");
        }
        // Line 3 (orbital): day · year · tilt · moons —
        // the rotation/orbit/satellite fields a reader would
        // associate with "what does the sky cycle look like".
        let _ = writeln!(
            s,
            "{:.0}h · {}mo · {:.0}° · {} moon{}",
            q32_to_f64(p.day_length_hours_q32),
            p.orbital_period_months,
            q32_to_f64(p.axial_tilt_deg_q32),
            p.moon_count,
            if p.moon_count == 1 { "" } else { "s" },
        );
        Some(s)
    }

    /// Species card body. Returns `None` until both `Planet`
    /// and `Species` events have arrived (the biochem axis needs
    /// the planet's substrate). Three lines:
    ///
    /// 1. *Cognition* — full-word topology + tier phrase
    ///    (`centralized medium cognition`).
    /// 2. *Senses + manipulation* — primary modality and primary
    ///    manipulation mode, prefixed with `sense:` / `manip:`
    ///    so the reader doesn't have to know the order convention.
    /// 3. *Biology* — lifespan years + sociality tier + comm tier
    ///    + substrate-implied biochemistry.
    ///
    /// The species name is *not* repeated here — `render()` writes
    /// it as the section divider label (`---- Cyranites ----`).
    pub(super) fn species_card(&self) -> Option<String> {
        use crate::q32::q32_to_f64;
        let p = self.planet.as_ref()?;
        let sp = self.species.as_ref()?;
        let mut s = String::new();
        // Line 1: cognition phrase. `{topology} {tier} cognition`
        // — a noun phrase that reads naturally. Tier bucket comes
        // from the shared `labels::cog_tier` so the boundaries
        // match every other consumer.
        let cog = q32_to_f64(sp.cognition_q32);
        let cog_tier_word = cog_tier(cog);
        let topo_full = match sp.cognition_topology.as_str() {
            "centralized" => "centralized",
            "distributed-redundant" => "distributed",
            "collective" => "collective",
            "acentric" => "acentric",
            _ => "unknown",
        };
        let _ = writeln!(s, "{topo_full} {cog_tier_word} cognition");
        // Line 2: senses + manipulation, labeled.
        let primary_modality = sp.modalities.first().map_or("?", String::as_str);
        let primary_manip = sp.manipulation_modes.first().map_or("?", String::as_str);
        let _ = writeln!(
            s,
            "sense: {} · manip: {}",
            short_modality(primary_modality),
            short_manip(primary_manip),
        );
        // Line 3: biology — lifespan, sociality, comm, biochem.
        let lifespan_years = q32_to_f64(sp.lifespan_years_q32) as i64;
        let soc_word = sociality_label(q32_to_f64(sp.sociality_q32));
        let comm_word = comm_label(q32_to_f64(sp.communication_fidelity_q32));
        let biochem = substrate_biochem(p.metabolic_substrate.as_str());
        let _ = writeln!(
            s,
            "{lifespan_years}y · {soc_word} · {comm_word} · {biochem}",
        );
        Some(s)
    }
}
