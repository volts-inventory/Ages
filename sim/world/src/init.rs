//! `init_planet` — derives per-cell physics state from a
//! sampled `Planet`. Big and self-contained: terrain (multi-
//! peak, Poisson-disc spread), water column derivation,
//! biosphere fuel deposition, atmosphere mass distribution,
//! magnetosphere-driven charge baseline, archetype-specific
//! gas-shell / sub-surface-ocean column overrides. Pulled out of
//! `lib.rs` so the type definitions sit at the top of the crate
//! without 400 lines of cell-painting in between.

use crate::{Atmosphere, BiosphereClass, Composition, Crust, Magnetosphere, Planet};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use sim_arith::Real;
use sim_physics::{Crust as PhysicsCrust, PhysicsState, Substance};

/// Map the worldgen `Crust` archetype (developmental bias —
/// fossil-fuel / piezoelectric / rare-earth biases) onto the
/// physics-side petrological `Crust` (P3.7), which keys
/// reflectivity off surface mineralogy. The two enums live in
/// different crates because they answer different questions:
/// worldgen wants "what biases does this crust impart on the
/// civ's tech tree?"; physics wants "what's the bare-cell base
/// albedo?". Variants without a dedicated reflectivity hint fall
/// through to [`PhysicsCrust::Default`] (0.20 bare-rock baseline).
#[must_use]
pub fn physics_crust_for(crust: Crust) -> PhysicsCrust {
    match crust {
        // Earth-like basalt-dominated crust → dark mafic albedo.
        Crust::Basaltic => PhysicsCrust::Basaltic,
        // Titan-style tholin / dark organic surface.
        Crust::Hydrocarbon => PhysicsCrust::Hydrocarbon,
        // No dedicated reflectivity hint for the three
        // "developmental-bias" archetypes — they're petrologically
        // unconstrained, so default to the bare-rock baseline.
        Crust::Piezoelectric | Crust::Ferrous | Crust::RareEarth => PhysicsCrust::Default,
    }
}

/// EM discharge threshold (lightning ceiling) per magnetosphere
/// class. Planet-metadata imprints derive from this — every per-cell charge
/// baseline must sit below it so the planet doesn't immediately
/// self-zap on the first EM tick. Shared with `sim_core::build_laws`
/// so the two sources of truth can't drift.
pub fn discharge_threshold_for(magnetosphere: Magnetosphere) -> Real {
    match magnetosphere {
        Magnetosphere::None => Real::from_int(20),
        Magnetosphere::Weak => Real::from_int(40),
        Magnetosphere::Strong => Real::from_int(80),
    }
}

/// Initialise physics state from a sampled planet. Deterministic:
/// same Planet + same grid → identical cell-level state every time.
#[allow(clippy::too_many_lines)]
pub fn init_planet(state: &mut PhysicsState, planet: &Planet) {
    // P3.7: pipe the planet's petrological crust class into
    // `PhysicsState` so the per-cell albedo loop sees a darker
    // base on basalt worlds and a lighter one on
    // hydrocarbon-tholin / icy / sedimentary worlds. Set early
    // so any law that reads albedo during init sees the right
    // baseline.
    state.set_planet_crust(physics_crust_for(planet.crust));

    let grid = state.grid().clone();
    let centre_q = planet
        .terrain_centre_q
        .rem_euclid(i32::try_from(grid.width()).expect("width fits in i32"));
    let centre_r = planet
        .terrain_centre_r
        .rem_euclid(i32::try_from(grid.height()).expect("height fits in i32"));
    let half_height_real = Real::from_int(i64::from(grid.height() / 2 + 1));

    // Atmospheric oxidiser density depends on atmosphere kind.
    let oxidiser_density = match planet.atmosphere {
        Atmosphere::None => Real::ZERO,
        Atmosphere::Thin => Real::from_ratio(1, 10),
        Atmosphere::Oxidising => Real::from_int(2),
        Atmosphere::Reducing => Real::percent(1),
        Atmosphere::Hazy => Real::from_ratio(1, 5),
    };
    // Two separate combustible channels:
    //   - `Substance::Fuel`   — biosphere-derived, renewable.
    //     Regrows toward `biofuel_ceiling` via the photosynthesis-
    //     equivalent `BiofuelRegrowth` reaction.
    //   - `Substance::Fossil` — buried hydrocarbons, non-renewable.
    //     Worldgen-only; combusts at a higher ignition threshold
    //     and never recovers. A `Piezoelectric` rocky planet with
    //     `Sparse` biosphere has very little of either, making "no
    //     combustion path" worlds still possible.
    let bio_fuel = match planet.biosphere {
        BiosphereClass::None => Real::ZERO,
        BiosphereClass::Sparse => Real::from_ratio(1, 5),
        BiosphereClass::Lush => Real::from_int(1),
        BiosphereClass::HyperBiodiverse => Real::from_int(3),
    };
    let fossil_density = match planet.crust {
        // Hydrocarbon crust deposits enough fossil that the
        // hydrocarbon-seep recognition template (Above(Fossil, 0))
        // fires; non-hydrocarbon crusts deposit none.
        Crust::Hydrocarbon => Real::from_int(4),
        Crust::Basaltic | Crust::Ferrous | Crust::RareEarth | Crust::Piezoelectric => Real::ZERO,
    };

    // Imprinting: planet metadata into per-cell physics state.
    // Without this, crust/atmosphere/magnetosphere are flavour-only
    // and the recognition layer can't observe them.
    //
    // Crust-derived initial charge on dry land cells. Stays below
    // the lightning_buildup threshold (40) so it doesn't trigger
    // continuous discharge events. Gives the recognition layer a
    // physical signature for each crust archetype.
    let crust_charge_baseline = match planet.crust {
        // Basaltic: neutral baseline.
        Crust::Basaltic => Real::ZERO,
        // Hydrocarbon: low baseline (oil deposits weakly conductive).
        Crust::Hydrocarbon => Real::from_int(2),
        // Piezoelectric: mechanical stress generates measurable charge
        // — fires `piezoelectric_pulse` when combined with fuel + dry.
        Crust::Piezoelectric => Real::from_int(12),
        // Ferrous: iron + rare-earth, magnetic-mineral baseline —
        // fires `magnetic_lodestone` on dry cells.
        Crust::Ferrous => Real::from_int(15),
        // RareEarth: trace baseline; templates fire only at extreme
        // cold (`superconductor_resonance`).
        Crust::RareEarth => Real::from_int(6),
    };
    // Atmosphere-derived initial vapour density. Drives
    // `reducing_storm` + `hazy_obscuration` + `pressure_storm`.
    let atmosphere_vapour_baseline = match planet.atmosphere {
        Atmosphere::None => Real::ZERO,
        Atmosphere::Thin => Real::percent(5),
        Atmosphere::Oxidising => Real::from_ratio(2, 10),
        Atmosphere::Reducing => Real::from_int(1),
        Atmosphere::Hazy => Real::from_int(2),
    };
    // Composition-derived initial ice fraction for cold rocky/
    // ocean worlds. SubSurfaceOcean handles its ice shell inline
    // by routing the water column to Ice on init (otherwise
    // chemistry's first-tick freeze releases enough latent heat
    // to re-melt it).
    let composition_ice_baseline = match planet.composition {
        Composition::SubSurfaceOcean | Composition::GaseousShell => Real::ZERO,
        Composition::Rocky | Composition::OceanWorld => {
            if planet.mean_temperature < Real::from_int(263) {
                Real::from_ratio(3, 10)
            } else {
                Real::ZERO
            }
        }
    };
    // Magnetosphere-derived planet-wide charge baseline. Adds to
    // the crust contribution so the field-and-resonance archetypes
    // get reliable EM activity.
    let magnetosphere_charge_baseline = match planet.magnetosphere {
        Magnetosphere::None => Real::ZERO,
        Magnetosphere::Weak => Real::from_int(1),
        Magnetosphere::Strong => Real::from_int(3),
    };
    // GaseousShell-specific imprint magnitudes. There is no
    // "surface" — every cell is a deep-atmosphere column whose
    // base reaches metallic-hydrogen depth. We imprint vapour,
    // temp, and charge planet-wide so EM diffusion (which
    // equalises any localised band back toward the planet
    // average) doesn't wipe the signal. The charge column tracks
    // the magnetosphere class so it sits just below the planet's
    // EM discharge threshold (`discharge_threshold_for`) —
    // otherwise the imprint self-zaps each tick. Always above the
    // metallic_hydrogen_signal template's |charge| > 14 firing
    // gate so gaseous-shell worlds reliably surface that
    // signature; pinned by the regression test
    // `imprints_satisfy_discharge_and_template_invariants`.
    let is_gaseous = matches!(planet.composition, Composition::GaseousShell);
    let gas_vapour_column = Real::from_int(5);
    let gas_temp_floor = Real::from_int(700);
    let gas_charge_column = match planet.magnetosphere {
        Magnetosphere::None => Real::from_int(15),
        Magnetosphere::Weak => Real::from_int(35),
        Magnetosphere::Strong => Real::from_int(70),
    };

    // Multi-peak elevation sampler. Replaces an earlier
    // single-cone falloff (one giant pyramid centred on
    // `terrain_centre_q/r`) with a deterministic 3–5 peak
    // composition. Each peak contributes a *piecewise* cone:
    // steep near the summit (drops fast through the upland
    // interior) and gentle around the coast band (≤ 50 m/cell so
    // the renderer's 100 m shallow→deep threshold leaves a `~`
    // band between coastal `░` and deep `≈`). Per-cell
    // contributions are summed (smooth-max), then capped at
    // `terrain_peak` so the renderer's relative-elevation glyph
    // ramp (`▲ > 0.7·peak`, `△ > 0.4·peak`) still fires off the
    // primary summit.
    //
    // Earlier, the linear cone had a fixed `terrain_peak/max_dim`
    // slope (≈ 150–400 m/cell) that jumped past the 100 m
    // shallow→deep threshold in one step — coastlines went
    // straight from `░` to `≈` with no `~` band. The piecewise
    // cone fixes that without sacrificing the visible peak
    // (steep summit interior keeps the high-relief feel).
    //
    // Height budget. The primary peak (anchored at
    // `terrain_centre_q/r` so legacy callers — imprint test,
    // catastrophe synthetic planets — still find a mountain
    // there) gets 80 % of `terrain_peak`; the remaining 20 % is
    // shared equally across 2–4 secondary peaks. Across 3–5 peaks
    // total, the cell-level sum stays bounded by `terrain_peak`
    // even at the primary summit (the cap is rarely reached, but
    // exists to enforce the upper bound).
    //
    // Determinism. Peak centres + count are sampled from a
    // ChaCha20 stream seeded with `planet.seed XOR TERRAIN_SALT`.
    // The salt is distinct from the planet-name pool (uses
    // raw `seed`) and the species-name pool
    // (`0xFEED_FACE_BAAD_F00D`) so the three streams stay
    // independent. GaseousShell + `terrain_peak == 0`
    // short-circuit to a zero-length peak vec → uniform-zero
    // elevation everywhere (no rocky surface).
    let terrain_peak_salt: u64 = 0xA17E_BEEF_C0DE_0147;
    // Slopes in metres per axial-distance cell. The "shallow"
    // slope is held below the renderer's 100 m shallow→deep
    // threshold so a peak's coastal flank always shows `~`. The
    // "steep" slope drops the summit interior fast enough that
    // even a 7000 m primary on a 32×20 grid taper through to the
    // shallow band within a handful of cells.
    let steep_slope = Real::from_int(200);
    // Shallow slope 350 m/cell. With a smaller value each peak's
    // shallow zone reached zero ~14 cells out, so 6-8 peaks at
    // ~5-cell spacing combined into one continuous landmass even
    // under max-of-cones. 350 m/cell shrinks the per-peak land
    // radius to ~6 cells; combined with max-of-cones each peak
    // becomes a discrete island. The shallow-water `~` band gets
    // narrower (≈ 0.3 cells per cone) but the population-wide
    // sweep test still catches at least one shallow-band cell.
    let shallow_slope = Real::from_int(350);
    // Cells of the primary cone whose elevation lies within the
    // gentle band (`buffer` metres above sea_level → coast).
    // Setting the buffer to 200 m means each peak's flank spends
    // a full 4 cells (`buffer / shallow_slope`) above sea_level
    // and another 2 cells (`100 / shallow_slope`) inside the
    // `~` shallow-water band before crossing into deep water.
    let multi_peak_buffer = Real::from_int(200);

    let peaks: Vec<(i32, i32, Real)> = if planet.terrain_peak == Real::ZERO {
        Vec::new()
    } else {
        let mut peak_rng = ChaCha20Rng::seed_from_u64(planet.seed ^ terrain_peak_salt);
        // 4..=7 secondaries → 5..=8 peaks total.
        // With max-of-cones combination and the steeper
        // shallow slope, each peak is its own discrete mountain;
        // more peaks = more separate landmasses spread across the
        // map (Earth has ~7 continents on a roughly 360° × 180°
        // sphere — scaled to a 36×30 grid we land in the same
        // ballpark with this count).
        let n_secondary: u32 = peak_rng.gen_range(4..=7);
        let w = i32::try_from(grid.width()).expect("width fits in i32");
        let h = i32::try_from(grid.height()).expect("height fits in i32");
        // Primary: anchored at the planet's `terrain_centre_q/r`,
        // 80 % of `terrain_peak`.
        let primary_height = (planet.terrain_peak * Real::from_int(8)) / Real::from_int(10);
        let mut peaks: Vec<(i32, i32, Real)> = Vec::with_capacity(1 + n_secondary as usize);
        peaks.push((centre_q, centre_r, primary_height));
        // Secondaries are substantial peaks in their own
        // right (50–80 % of `terrain_peak`), not the tiny 5 % bumps
        // they were under an earlier sum-of-cones model. With max-of-cones
        // every peak's height shows directly, and a secondary
        // smaller than the primary's flank-at-its-position would
        // be invisible — so each secondary needs to be tall enough
        // to clear the primary's far-flank elevation. The
        // `peak_rng.gen_range(50..=80)` band varies them so the
        // mountains aren't uniform-height clones.
        // Minimum-distance rejection sampling. With a single
        // uniform draw per secondary's (q, r) — fine on
        // average — but with 3–5 peaks on a 36×30 grid the secondaries
        // routinely landed within a few cells of the primary or each
        // other, manifesting as one big mountain blob instead of
        // separate ranges. Re-rolls (up to `max_attempts`)
        // until the candidate sits at least `min_dist` cells from
        // every already-accepted peak. Distance metric matches the
        // piecewise-cone falloff (`|dq| + |dr|`) so "spread out
        // visually" and "spread out numerically" line up.
        //
        // `min_dist` is `max(3, max(w, h) / (num_peaks * 2))`. On a
        // 36×30 grid with 4 peaks total that's 36/8 = 4 cells; with
        // 5 peaks it's 36/10 = 3 cells (clamped at the floor); with
        // 3 peaks it's 36/6 = 6 cells. Roughly half of an
        // equal-partition gap so peaks scatter without forcing a
        // grid-pattern feel. The 3-cell floor keeps small grids
        // (8×6 used in `init_planet_is_deterministic`) from
        // collapsing min_dist to 0 and trivially accepting every
        // candidate.
        //
        // Falls back to the last attempted candidate if no valid
        // position is found in `max_attempts` tries — rare in
        // practice (≤5 peaks on a 36×30 grid leaves plenty of room),
        // but the bound keeps determinism: same seed always draws
        // the same RNG sequence, so the fallback hits the same cell.
        let num_peaks = 1 + n_secondary;
        let max_dim = if w > h { w } else { h };
        // min_dist set to `max_dim / (num_peaks + 1)`. An earlier
        // spacing of `max_dim / (num_peaks * 2)` was half of an
        // equal-partition gap, which let peaks cluster in one
        // quadrant; combined with the wide cones, this produced
        // one giant continent-blob. The current value spaces them
        // at a full equal-partition gap so 5–8 peaks scatter across
        // the whole map. On a 36×30 grid with 6 peaks that's 36/7
        // ≈ 5 cells (was 36/12 = 3); with 8 peaks 36/9 = 4.
        // Combined with the narrower cones (≈ 13-cell radius)
        // this gives discrete mountainous islands separated by
        // ocean. The 3-cell floor still applies for the small
        // dev grid (8×6) so determinism tests stay valid.
        let min_dist_raw = max_dim / i32::try_from(num_peaks + 1).expect("num_peaks fits");
        let min_dist = if min_dist_raw > 3 { min_dist_raw } else { 3 };
        let max_attempts: u32 = 200;
        for _ in 0..n_secondary {
            let mut chosen: Option<(i32, i32)> = None;
            let mut last: (i32, i32) = (0, 0);
            for _ in 0..max_attempts {
                let q = peak_rng.gen_range(0..w);
                let r = peak_rng.gen_range(0..h);
                last = (q, r);
                let ok = peaks
                    .iter()
                    .all(|&(pq, pr, _)| (q - pq).abs() + (r - pr).abs() >= min_dist);
                if ok {
                    chosen = Some((q, r));
                    break;
                }
            }
            let (q, r) = chosen.unwrap_or(last);
            // Each secondary's height is 50–80 % of
            // `terrain_peak`, drawn from the same salted RNG so
            // determinism is preserved. Different heights make
            // the mountains visually varied (some big, some
            // small) without losing visibility under max-of-
            // cones — every secondary still clears the primary's
            // far-flank elevation at min_dist away.
            let height_pct: i64 = peak_rng.gen_range(50..=80);
            let height = (planet.terrain_peak * Real::from_int(height_pct)) / Real::from_int(100);
            peaks.push((q, r, height));
        }
        peaks
    };
    // Pivot height — the elevation at which each peak's cone
    // switches from steep to shallow slope. Above sea_level by
    // `multi_peak_buffer` so the gentle band straddles the coast.
    let pivot_elev = planet.sea_level + multi_peak_buffer;

    for (cid, axial) in grid.cells() {
        let i = cid.0 as usize;

        // Elevation = max of per-peak piecewise cones (was
        // sum). An earlier sum-of-cones approach made secondary
        // peaks rise visibly above the primary's flank, but with
        // a wide cone footprint that produced one giant
        // continent-blob covering most of the map — every cell
        // got contributions from every peak. Switching to max
        // (combined with the narrower shallow slope and
        // higher peak count) gives each peak its own discrete
        // mountain with ocean between, so the planet reads as
        // archipelago / scattered-continents instead of one
        // mega-continent.
        //
        // Trade-off: a small secondary peak placed *inside*
        // a tall primary's cone will be invisible (the primary's
        // shallow taper at that distance is taller than the
        // secondary's whole height). The minimum-distance
        // sampler keeps secondaries far enough from each other
        // that this rarely matters in practice, but it's
        // documented here so future tuners know the behaviour.
        //
        // GaseousShell + `terrain_peak == 0` keep uniform-zero
        // elevation (peaks vec empty).
        let elev = if peaks.is_empty() {
            Real::ZERO
        } else {
            let mut max_elev = Real::ZERO;
            for &(pq, pr, peak_height) in &peaks {
                let dq = (axial.q - pq).abs();
                let dr = (axial.r - pr).abs();
                let dist = Real::from_int(i64::from(dq + dr));
                let contribution = if peak_height <= pivot_elev {
                    // Short peak — entirely shallow-slope cone.
                    let drop = dist * shallow_slope;
                    if drop >= peak_height {
                        Real::ZERO
                    } else {
                        peak_height - drop
                    }
                } else {
                    // Tall peak: steep summit interior, then a
                    // gentle coastal band.
                    let steep_drop_total = peak_height - pivot_elev;
                    // Cells of steep zone (round down via integer
                    // div-equivalent: we work with Real so the
                    // boundary at the exact transition is fine).
                    let dist_steep = steep_drop_total / steep_slope;
                    if dist <= dist_steep {
                        peak_height - dist * steep_slope
                    } else {
                        let dist_into_shallow = dist - dist_steep;
                        let shallow_drop = dist_into_shallow * shallow_slope;
                        if shallow_drop >= pivot_elev {
                            Real::ZERO
                        } else {
                            pivot_elev - shallow_drop
                        }
                    }
                };
                if contribution > max_elev {
                    max_elev = contribution;
                }
            }
            if max_elev > planet.terrain_peak {
                planet.terrain_peak
            } else {
                max_elev
            }
        };
        state.elevation_mut()[i] = elev;

        // Water depth: cells with elevation < sea_level are flooded
        // to sea level.
        let depth = if elev < planet.sea_level {
            planet.sea_level - elev
        } else {
            Real::ZERO
        };
        state.water_depth_mut()[i] = depth;

        // Mirror surface water into the chemistry Water substance.
        // SubSurfaceOcean worlds put the whole column into Ice (the
        // ice shell at the surface) instead of Water — otherwise
        // chemistry's first-tick freeze cycle releases enough latent
        // heat to spike the cell well above freezing and re-melt
        // everything, destroying the ice signature.
        if matches!(planet.composition, Composition::SubSurfaceOcean) {
            state.substance_mut(Substance::Water.idx())[i] = Real::ZERO;
            state.substance_mut(Substance::Ice.idx())[i] = depth;
        } else {
            state.substance_mut(Substance::Water.idx())[i] = depth;
        }

        // Atmospheric oxidiser available in cells with elevation
        // above sea level (i.e., land + air column). Aquatic /
        // sub-surface cells get a fraction.
        let oxid = if elev > planet.sea_level {
            oxidiser_density
        } else {
            oxidiser_density / Real::from_int(10)
        };
        state.substance_mut(Substance::Oxidiser.idx())[i] = oxid;

        // Biosphere fuel — only on land cells with non-zero
        // biosphere. Renewable: the `biofuel_ceiling` field stores
        // this same value as the regrowth target, so combustion-
        // depleted cells relax back toward the worldgen baseline.
        let bio = if elev > planet.sea_level {
            bio_fuel
        } else {
            Real::ZERO
        };
        state.substance_mut(Substance::Fuel.idx())[i] = bio;
        state.biofuel_ceiling_mut()[i] = bio;
        // Fossil hydrocarbons — buried, non-renewable. Same land
        // mask as biofuel; gas-giant-shell handling below zeroes it.
        let fossil = if elev > planet.sea_level {
            fossil_density
        } else {
            Real::ZERO
        };
        state.substance_mut(Substance::Fossil.idx())[i] = fossil;

        // Planet-metadata imprints — apply per-cell.
        // Crust charge: only on dry land cells (so water cells
        // don't shadow lithosphere signal).
        let on_land = elev > planet.sea_level;
        let charge = if on_land {
            crust_charge_baseline
        } else {
            Real::ZERO
        } + magnetosphere_charge_baseline;
        state.charge_mut()[i] = charge;
        // Atmospheric vapour: planet-wide above sea level; sea
        // surface gets a smaller fraction; sub-surface zero.
        let vapour = if on_land {
            atmosphere_vapour_baseline
        } else if elev == planet.sea_level || depth < Real::from_int(10) {
            atmosphere_vapour_baseline / Real::from_int(2)
        } else {
            Real::ZERO
        };
        state.substance_mut(Substance::Vapour.idx())[i] = vapour;
        // Composition ice: cold Rocky/OceanWorld gets partial cover.
        // (SubSurfaceOcean handled inline above.)
        if composition_ice_baseline > Real::ZERO {
            state.substance_mut(Substance::Ice.idx())[i] = composition_ice_baseline;
        }

        // Temperature: latitude-driven gradient around mean.
        let pole_dist =
            (axial.r - i32::try_from(grid.height() / 2).expect("height fits in i32")).abs();
        let pole_dist_real = Real::from_int(i64::from(pole_dist));
        let half_grad = planet.temperature_gradient / Real::from_int(2);
        let mut t = planet.mean_temperature + half_grad
            - (planet.temperature_gradient * pole_dist_real) / half_height_real;

        // GaseousShell imprint — every cell is a deep atmospheric
        // column reaching metallic-hydrogen depth. Vapour fills
        // the column; temperature floor + charge floor are
        // applied planet-wide so EM diffusion can't equalise the
        // signal away. The latitude-driven `t` term still
        // contributes a sub-K gradient on top of the floor.
        if is_gaseous {
            state.substance_mut(Substance::Vapour.idx())[i] = gas_vapour_column;
            state.substance_mut(Substance::Water.idx())[i] = Real::ZERO;
            state.substance_mut(Substance::Fuel.idx())[i] = Real::ZERO;
            state.substance_mut(Substance::Fossil.idx())[i] = Real::ZERO;
            state.biofuel_ceiling_mut()[i] = Real::ZERO;
            t = gas_temp_floor + (t - planet.mean_temperature);
            state.charge_mut()[i] = gas_charge_column;
        }
        state.temperature_mut()[i] = t;
    }
}
