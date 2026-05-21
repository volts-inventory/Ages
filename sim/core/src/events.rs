//! Event-payload conversion helpers. Pure functions mapping
//! sim-side data structures (`ConfirmedRelation`, `Planet`,
//! `Species`, `NamedFigure`, `Civ`) into the `protocol::*Derived`
//! / `*Event` payload structs the NDJSON consumer reads. Q32.32
//! `Real` scalars are emitted as raw bits so the event log stays
//! bit-exact deterministic across platforms.

use protocol::{FigureBorn, PlanetDerived, RelationConfirmed, SpeciesDerived};
use sim_civ::discovery::ConfirmedRelation;
use sim_recognition::RecognitionLibrary;
use sim_species::Species;
use sim_world::{Atmosphere, BiosphereClass, Composition, Crust, Magnetosphere, Planet};

/// Map a `ConfirmedRelation` to its protocol event payload. Real-
/// valued fields encoded as `Q32.32` raw bits for bit-exact event
/// determinism across platforms. Parameters are rescaled out of the
/// hypothesizer's normalised fit-space (`x_norm = x_real / channel.scale()`)
/// into real-unit space so external consumers receive SI-consistent
/// coefficients.
pub(crate) fn relation_to_event(
    tick: u64,
    r: &ConfirmedRelation,
    figure_id: u32,
    recognition: &RecognitionLibrary,
) -> RelationConfirmed {
    let real_params = r.params_in_real_units();
    let template_name = recognition
        .templates
        .iter()
        .find(|t| t.id == r.template_id)
        .map_or_else(
            || format!("template_{}", r.template_id),
            |t| t.name.to_string(),
        );
    RelationConfirmed {
        tick,
        relation_id: r.relation_id,
        figure_id,
        template_id: r.template_id,
        template_name,
        channel: r.channel.tag().to_string(),
        form: r.form.tag().to_string(),
        params_q32: real_params.iter().map(|p| p.raw().to_bits()).collect(),
        residual_q32: r.residual.raw().to_bits(),
        confidence_q32: r.confidence.raw().to_bits(),
        n_samples: u32::try_from(r.n_samples).unwrap_or(u32::MAX),
    }
}

/// Map the sampled `Planet` to its protocol event payload. Real
/// scalars are emitted as `Q32.32` raw bits; enums are stringified
/// in `snake_case` so consumers don't need a private mapping.
pub(crate) fn planet_to_event(p: &Planet) -> PlanetDerived {
    PlanetDerived {
        seed: p.seed,
        name: p.name.clone(),
        gravity_q32: p.gravity.raw().to_bits(),
        composition: match p.composition {
            Composition::Rocky => "rocky".into(),
            Composition::OceanWorld => "ocean_world".into(),
            Composition::SubSurfaceOcean => "sub_surface_ocean".into(),
            Composition::GaseousShell => "gaseous_shell".into(),
        },
        mean_temperature_q32: p.mean_temperature.raw().to_bits(),
        temperature_gradient_q32: p.temperature_gradient.raw().to_bits(),
        terrain_peak_q32: p.terrain_peak.raw().to_bits(),
        sea_level_q32: p.sea_level.raw().to_bits(),
        atmosphere: match p.atmosphere {
            Atmosphere::None => "none".into(),
            Atmosphere::Thin => "thin".into(),
            Atmosphere::Oxidising => "oxidising".into(),
            Atmosphere::Reducing => "reducing".into(),
            Atmosphere::Hazy => "hazy".into(),
        },
        surface_pressure_q32: p.surface_pressure.raw().to_bits(),
        biosphere: match p.biosphere {
            BiosphereClass::None => "none".into(),
            BiosphereClass::Sparse => "sparse".into(),
            BiosphereClass::Lush => "lush".into(),
            BiosphereClass::HyperBiodiverse => "hyper_biodiverse".into(),
        },
        magnetosphere: match p.magnetosphere {
            Magnetosphere::None => "none".into(),
            Magnetosphere::Weak => "weak".into(),
            Magnetosphere::Strong => "strong".into(),
        },
        crust: match p.crust {
            Crust::Basaltic => "basaltic".into(),
            Crust::Hydrocarbon => "hydrocarbon".into(),
            Crust::Piezoelectric => "piezoelectric".into(),
            Crust::Ferrous => "ferrous".into(),
            Crust::RareEarth => "rare_earth".into(),
        },
        stellar_luminosity_q32: p.stellar_luminosity.raw().to_bits(),
        moon_count: p.moon_count,
        axial_tilt_deg_q32: p.axial_tilt_deg.raw().to_bits(),
        day_length_hours_q32: p.day_length_hours.raw().to_bits(),
        orbital_period_months: p.orbital_period_months,
        metabolic_substrate: p.metabolic_substrate.tag().to_string(),
        substrate_perturbation_q32: p.substrate_perturbation.raw().to_bits(),
        atmospheric_n2_q32: p.atmospheric_composition.n2.raw().to_bits(),
        atmospheric_o2_q32: p.atmospheric_composition.o2.raw().to_bits(),
        atmospheric_co2_q32: p.atmospheric_composition.co2.raw().to_bits(),
        atmospheric_ch4_q32: p.atmospheric_composition.ch4.raw().to_bits(),
        atmospheric_nh3_q32: p.atmospheric_composition.nh3.raw().to_bits(),
        atmospheric_h2o_q32: p.atmospheric_composition.h2o.raw().to_bits(),
        atmospheric_h2_q32: p.atmospheric_composition.h2.raw().to_bits(),
        atmospheric_ar_q32: p.atmospheric_composition.ar.raw().to_bits(),
        atmospheric_other_q32: p.atmospheric_composition.other.raw().to_bits(),
        biosphere_density_q32: p.biosphere_density.raw().to_bits(),
        crustal_silicate_q32: p.crustal_composition.silicate.raw().to_bits(),
        crustal_hydrocarbon_q32: p.crustal_composition.hydrocarbon.raw().to_bits(),
        crustal_piezoelectric_q32: p.crustal_composition.piezoelectric.raw().to_bits(),
        crustal_ferrous_q32: p.crustal_composition.ferrous.raw().to_bits(),
        crustal_rare_earth_q32: p.crustal_composition.rare_earth.raw().to_bits(),
        crustal_ice_q32: p.crustal_composition.ice.raw().to_bits(),
        crustal_other_q32: p.crustal_composition.other.raw().to_bits(),
    }
}

/// Build a `FigureBorn` event from a `NamedFigure` + civ id.
/// Personality scalars (charisma, curiosity, doubt,
/// communicativeness) are emitted as `Q32.32` raw bits for
/// bit-exact determinism; consumers like the post-run report
/// recover them via `q32_to_f64`.
pub(crate) fn figure_born_event(civ_id: u32, fig: &sim_civ::figures::NamedFigure) -> FigureBorn {
    FigureBorn {
        tick: fig.born_tick,
        civ_id,
        figure_id: fig.id,
        name: fig.name.clone(),
        charisma_q32: fig.charisma.raw().to_bits(),
        curiosity_q32: fig.curiosity.raw().to_bits(),
        doubt_q32: fig.doubt.raw().to_bits(),
        communicativeness_q32: fig.communicativeness.raw().to_bits(),
        cell_assignment: fig.cell_assignment,
    }
}

/// Snapshot a civ's claimed-cells set as a sorted Vec for the
/// `CivFounded` event payload. Sorted for byte-identical
/// determinism across runs.
pub(crate) fn claimed_cells_for_event(civ: &sim_civ::Civ) -> Vec<u32> {
    let mut cells: Vec<u32> = civ.claimed_cells.iter().copied().collect();
    cells.sort_unstable();
    cells
}

/// Map a derived `Species` to its protocol event payload. Real
/// scalars are emitted as `Q32.32` raw bits so the event log stays
/// bit-exact deterministic across platforms.
pub(crate) fn species_to_event(s: &Species) -> SpeciesDerived {
    SpeciesDerived {
        seed: s.seed,
        name: s.name.clone(),
        cognition_q32: s.cognition.raw().to_bits(),
        sociality_q32: s.sociality.raw().to_bits(),
        communication_fidelity_q32: s.communication_fidelity.raw().to_bits(),
        lifespan_years_q32: s.lifespan_years.raw().to_bits(),
        t0_loss_q32: s.t0_loss.raw().to_bits(),
        modalities: s
            .modalities
            .iter()
            .map(|m| format!("{:?}", m.kind))
            .collect(),
        manipulation_modes: s
            .manipulation_modes
            .iter()
            .map(|m| format!("{:?}", m.kind))
            .collect(),
        perceivable_template_ids: s.perceivable_templates.iter().copied().collect(),
        cognition_topology: match s.cognition_topology {
            sim_species::CognitionTopology::Centralized => "centralized".into(),
            sim_species::CognitionTopology::DistributedRedundant => "distributed-redundant".into(),
            sim_species::CognitionTopology::Collective => "collective".into(),
            sim_species::CognitionTopology::Acentric => "acentric".into(),
        },
    }
}
