//! Planet card + ASCII grid map + latitude climate strip.
//! `terrain_symbol` is shared with `frame.rs` (which renders the
//! same hex grid for the live viewport) so it stays
//! `pub(crate)` and re-exports through `render::mod.rs`.

use crate::digest::Digest;
use crate::q32::q32_to_f64;
use std::fmt::Write;

pub(crate) fn render_planet(s: &mut String, d: &Digest) {
    let _ = writeln!(s, "## Planet");
    let _ = writeln!(s);
    if let Some(p) = &d.planet {
        let _ = writeln!(s, "| Property | Value |");
        let _ = writeln!(s, "|---|---|");
        let _ = writeln!(s, "| Composition | `{}` |", p.composition);
        let _ = writeln!(
            s,
            "| Surface gravity | {:.2} m/s² |",
            q32_to_f64(p.gravity_q32)
        );
        let _ = writeln!(
            s,
            "| Mean surface temp | {:.0} K |",
            q32_to_f64(p.mean_temperature_q32)
        );
        let _ = writeln!(
            s,
            "| Equator-to-pole gradient | {:.0} K |",
            q32_to_f64(p.temperature_gradient_q32)
        );
        let _ = writeln!(
            s,
            "| Terrain peak | {:.0} m |",
            q32_to_f64(p.terrain_peak_q32)
        );
        let _ = writeln!(s, "| Sea level | {:.0} m |", q32_to_f64(p.sea_level_q32));
        let _ = writeln!(s, "| Atmosphere | `{}` |", p.atmosphere);
        let _ = writeln!(
            s,
            "| Surface pressure | {:.0} Pa |",
            q32_to_f64(p.surface_pressure_q32)
        );
        let _ = writeln!(s, "| Biosphere | `{}` |", p.biosphere);
        let _ = writeln!(s, "| Magnetosphere | `{}` |", p.magnetosphere);
        let _ = writeln!(s, "| Crust | `{}` |", p.crust);
        // Bolometric stellar flux at planet — wire emits it as the
        // incident-flux scalar (W/m²). Derive a spectral-class label
        // by comparing against the solar constant (~1361 W/m²); the
        // mapping is rough but reads as "F-type star" / "M-type
        // star" / etc., which is what a planet header wants.
        let lum_wm2 = q32_to_f64(p.stellar_luminosity_q32);
        let _ = writeln!(
            s,
            "| Stellar flux at planet | {lum_wm2:.0} W/m² (~{} class) |",
            stellar_class_label(lum_wm2)
        );
        // Habitable-zone edges from the incident-flux model: the
        // inner / outer flux limits of the conservative HZ are
        // roughly `1.10` / `0.36` of the solar constant. Translate
        // these into orbital-distance ratios via `d = sqrt(L_star /
        // F_limit)` for a sun-like baseline. Reported in solar-flux
        // units so the value reads alongside the flux row.
        let _ = writeln!(
            s,
            "| HZ flux band | {:.2}–{:.2} solar (planet sits at {:.2} solar) |",
            HZ_FLUX_OUTER_SOLAR,
            HZ_FLUX_INNER_SOLAR,
            lum_wm2 / SOLAR_CONSTANT_W_M2,
        );
        let _ = writeln!(s, "| Moons | {} |", p.moon_count);
        let _ = writeln!(
            s,
            "| Axial tilt | {:.1}° |",
            q32_to_f64(p.axial_tilt_deg_q32)
        );
        let _ = writeln!(
            s,
            "| Day length | {:.0} hours |",
            q32_to_f64(p.day_length_hours_q32)
        );
        let _ = writeln!(s, "| Orbital period | {} months |", p.orbital_period_months);
        let _ = writeln!(s, "| Metabolic substrate | `{}` |", p.metabolic_substrate);
        // Day-length / orbital-period heuristic for tidal-locking
        // state: a sidereal day equal to the orbital period reads
        // as synchronous rotation. The wire schema doesn't carry a
        // dedicated locking-state field yet; this heuristic stays
        // in the report layer so the rule lives next to its display.
        let _ = writeln!(s, "| Rotation state | {} |", rotation_state_label(p));
    } else {
        let _ = writeln!(s, "_(planet event not emitted)_");
    }
    let _ = writeln!(s);

    // Ecosystem / planet-dynamics aggregate card. Surfaces the
    // run-end species census, speciation / HGT counts, catastrophe
    // histogram, and the mean civ ecological resilience.
    render_ecosystem_summary(s, d);

    // ASCII map of the spatial grid. The vision mentions "a spatial
    // grid" but the report had no spatial view. Per-cell elevation +
    // water_depth come from the PlanetMap event; symbols pick the
    // most-distinctive feature per cell (peak / land / sea / deep
    // water).
    if let Some(pm) = &d.planet_map {
        if pm.grid_width > 0 && pm.grid_height > 0 {
            render_ascii_map(s, pm, d.planet.as_ref());
        }
    }
}

/// Per-cell terrain symbol decision. Reads elevation +
/// `water_depth` plus the cell's neighbours to distinguish
/// coastal from inland land. Symbols (Unicode block characters;
/// render in any monospace markdown viewer):
///
/// - `▲` peak area (elevation > 0.7 × `terrain_peak`)
/// - `△` mountain (elevation > 0.4 × `terrain_peak`)
/// - `░` coastal land (within 1 cell of water)
/// - `▒` inland land
/// - `~` shallow water (`water_depth` ≤ 100m)
/// - `≈` deep water
/// - `≡` gaseous shell (`terrain_peak == 0`; no rocky surface)
/// - `·` featureless surface (low-relief rocky / sub-surface
///   ocean ice / oceanic basin without liquid water)
#[allow(clippy::many_single_char_names)]
pub(crate) fn terrain_symbol(
    pm: &protocol::PlanetMap,
    r: usize,
    q: usize,
    terrain_peak: f64,
) -> char {
    let w = pm.grid_width as usize;
    let i = r * w + q;
    let elev = pm.elevation_q32.get(i).copied().map_or(0.0, q32_to_f64);
    let depth = pm.water_depth_q32.get(i).copied().map_or(0.0, q32_to_f64);
    if depth > 100.0 {
        return '\u{2248}'; // deep water
    }
    if depth > 0.0 {
        return '~';
    }
    // Hill/peak thresholds relative to the (sea_level, peak)
    // range, not raw `terrain_peak`. The original cutoffs were
    // 0.4 / 0.7 of `terrain_peak`, but `sea_level` is typically
    // already ~0.4 of `terrain_peak` (Rocky planets sample
    // sea_level in [1000, 4000] m and terrain_peak above that),
    // so any cell barely above sea_level immediately read as a
    // hill. The result: virtually every land cell rendered △ or
    // ▲ and the `▒` inland glyph almost never appeared.
    //
    // Recasting the thresholds against the *land range*
    // `(sea_level, terrain_peak)` gives ▒ to the lower 60 % of
    // land, △ to the next 30 %, ▲ to the top 10 %. A typical
    // continent now reads as a flat plain (`▒`) ringed by coast
    // (`░`) with hills and a few peaks at the centre — visually
    // matching how Earth maps render at this zoom. The numeric
    // bands carry no behaviour outside of glyph picking, so this
    // is purely a display fix.
    if terrain_peak > 0.0 && elev > 0.0 {
        // PlanetMap doesn't carry sea_level directly, so we
        // approximate it as 30 % of terrain_peak (matches the
        // typical `sea_level / terrain_peak` ratio for
        // Rocky / OceanWorld samples — the renderer is
        // display-only so a ~few-percent miss is fine).
        let approx_sea = 0.3 * terrain_peak;
        let land_range = terrain_peak - approx_sea;
        if land_range > 0.0 {
            let land_frac = (elev - approx_sea) / land_range;
            if land_frac > 0.85 {
                return '\u{25B2}'; // ▲ peak (top 15 %)
            }
            if land_frac > 0.55 {
                return '\u{25B3}'; // △ hill (next 30 %)
            }
            // Fall through to coast/inland branch below for
            // the lower 55 % of land.
        }
    }
    if elev <= 0.0 {
        // Replace the bare ` ` blank fallback with two
        // distinct glyphs so an "empty" map (no surface water,
        // no positive elevation) at least conveys *what kind*
        // of empty:
        //   - `terrain_peak == 0` → gaseous shell (gas-giant
        //     composition; no rocky surface). Render `≡` to
        //     suggest cloud bands.
        //   - otherwise → low-relief rocky / sub-surface ocean
        //     ice / oceanic-basin floor without liquid water.
        //     Render `·` to read as "featureless" rather than
        //     "literally a blank cell".
        // Civ markers still overlay both glyphs.
        if terrain_peak == 0.0 {
            return '\u{2261}'; // ≡ gas band
        }
        return '\u{00B7}'; // · featureless
    }
    // Land: distinguish coastal from inland by checking the four
    // axial neighbours for water. Only the cardinal neighbours are
    // checked (not the diagonal six-hex set) — sufficient for the
    // coarse 8x6 grid the dev profile uses, and visually crisp.
    let h = pm.grid_height as usize;
    let neighbour_is_water = |nr: i64, nq: i64| -> bool {
        if nr < 0 || nq < 0 || nr >= h as i64 || nq >= w as i64 {
            return false;
        }
        let ni = (nr as usize) * w + (nq as usize);
        pm.water_depth_q32
            .get(ni)
            .copied()
            .is_some_and(|d| q32_to_f64(d) > 0.0)
    };
    let coastal = neighbour_is_water(r as i64 - 1, q as i64)
        || neighbour_is_water(r as i64 + 1, q as i64)
        || neighbour_is_water(r as i64, q as i64 - 1)
        || neighbour_is_water(r as i64, q as i64 + 1);
    if coastal {
        '\u{2591}' // ░ coastal
    } else {
        '\u{2592}' // ▒ inland
    }
}

/// Render the planet's grid as an ASCII map with row + column
/// axes and the terrain-symbol palette from `terrain_symbol`. The
/// hex grid is laid out with alternating-row indentation so a
/// monospace renderer reads it as a hex tessellation rather than
/// a square grid.
fn render_ascii_map(
    s: &mut String,
    pm: &protocol::PlanetMap,
    planet: Option<&protocol::PlanetDerived>,
) {
    let terrain_peak = planet.map_or(0.0, |p| q32_to_f64(p.terrain_peak_q32));
    let w = pm.grid_width as usize;
    let h = pm.grid_height as usize;
    let _ = writeln!(s, "```text");
    let _ = writeln!(
        s,
        "Planet map ({}x{} hex). ▲ peak  △ mtn  ▒ inland  ░ coast  ~ shallow  ≈ deep",
        pm.grid_width, pm.grid_height
    );
    let _ = writeln!(s);
    // Column header. Two-character columns plus the row-prefix
    // padding (3 chars) so the digits land above their cells.
    let mut header = String::from("    ");
    for q in 0..w {
        let _ = write!(header, "{q:>2}");
    }
    let _ = writeln!(s, "{header}");
    for r in 0..h {
        let mut line = format!("{r:>2}  ");
        // Hex offset: odd rows indent by 1.
        if r % 2 == 1 {
            line.push(' ');
        }
        for q in 0..w {
            line.push(terrain_symbol(pm, r, q, terrain_peak));
            line.push(' ');
        }
        let _ = writeln!(s, "{line}");
    }
    let _ = writeln!(s, "```");
    let _ = writeln!(s);

    // Climate strip: latitude temperature gradient. The sim's
    // per-cell temperature varies by row (see init_planet); render
    // it as a horizontal bar of cell-row indices coloured cold→hot.
    if let Some(p) = planet {
        render_climate_strip(s, p, h);
    }
}

/// Single-row temperature bar showing the latitude gradient from
/// pole to equator. Reads `mean_temperature` + `temperature_gradient`
/// from the planet event and reproduces the per-row temperature
/// formula from `sim_world::init_planet`.
fn render_climate_strip(s: &mut String, p: &protocol::PlanetDerived, h: usize) {
    if h == 0 {
        return;
    }
    let mean_t = q32_to_f64(p.mean_temperature_q32);
    let grad = q32_to_f64(p.temperature_gradient_q32);
    let half_grad = grad / 2.0;
    let half_height = (h as f64 / 2.0 + 1.0).max(1.0);
    // Recompute per-row temperature as init_planet does:
    // t = mean + half_grad - (gradient * pole_dist) / half_height
    let mut row_temps: Vec<f64> = Vec::with_capacity(h);
    for r in 0..h {
        let pole_dist = ((r as i64) - (h as i64 / 2)).unsigned_abs() as f64;
        let t = mean_t + half_grad - (grad * pole_dist) / half_height;
        row_temps.push(t);
    }
    let min_t = row_temps.iter().copied().fold(f64::INFINITY, f64::min);
    let max_t = row_temps.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let _ = writeln!(s, "```text");
    let _ = writeln!(
        s,
        "Latitude climate strip: per-row mean surface temperature (K)."
    );
    let _ = writeln!(s);
    // 5-bucket discretisation: cold ░ ▒ ▓ █ hot. Buckets per row.
    for (r, t) in row_temps.iter().enumerate() {
        let bucket = if max_t - min_t < 1e-9 {
            2
        } else {
            let norm = (*t - min_t) / (max_t - min_t);
            (norm * 4.99).floor() as usize
        };
        let glyph = match bucket {
            0 => '\u{2591}',
            1 => '\u{2592}',
            2 => '\u{2593}',
            _ => '\u{2588}',
        };
        let _ = writeln!(
            s,
            "  row {r:>2}  {glyph} {glyph} {glyph} {glyph} {glyph} {glyph} {glyph} {glyph}  ~{t:.0} K"
        );
    }
    let _ = writeln!(s, "```");
    let _ = writeln!(s);
}

/// Earth-equivalent solar flux (W/m²). The wire's
/// `PlanetDerived::stellar_luminosity_q32` is the incident flux at
/// the planet, so we normalise against the solar constant to read
/// the value in "solar units" and infer a rough spectral class.
const SOLAR_CONSTANT_W_M2: f64 = 1361.0;

/// Inner edge of the conservative habitable zone, expressed as
/// the maximum incident flux relative to the solar constant.
/// Above this the inner-HZ greenhouse runaway sets in.
const HZ_FLUX_INNER_SOLAR: f64 = 1.10;

/// Outer edge of the conservative habitable zone, expressed as
/// the minimum incident flux relative to the solar constant.
/// Below this CO₂ ices out / runaway snowball sets in.
const HZ_FLUX_OUTER_SOLAR: f64 = 0.36;

/// Rough Morgan-Keenan-style spectral class label from the incident
/// flux. Coarse bands — the report layer's job is "give the reader
/// a hint about the host star" not "model stellar physics."
fn stellar_class_label(flux_wm2: f64) -> &'static str {
    let ratio = flux_wm2 / SOLAR_CONSTANT_W_M2;
    if ratio >= 5.0 {
        "F"
    } else if ratio >= 1.5 {
        "G-warm"
    } else if ratio >= 0.7 {
        "G"
    } else if ratio >= 0.3 {
        "K"
    } else {
        "M"
    }
}

/// Heuristic tidal-locking / rotation-state label. A planet whose
/// sidereal day length (in hours) is nearly equal to its orbital
/// period (translated to hours via 30 days/month × 24 h/day) reads
/// as synchronous rotation. The wire schema doesn't yet carry the
/// dedicated locking-state field; this heuristic gives the report
/// a useful proxy until it does.
fn rotation_state_label(p: &protocol::PlanetDerived) -> &'static str {
    let day_hours = q32_to_f64(p.day_length_hours_q32).max(0.0);
    // 30 days/month × 24 h/day is the conventional Earth-equivalent
    // for the calendar month the sim uses. Sufficient for the
    // "is the day length within ±5% of one orbit?" check.
    let orbit_hours = f64::from(p.orbital_period_months) * 30.0 * 24.0;
    if day_hours == 0.0 || orbit_hours == 0.0 {
        return "unknown";
    }
    let ratio = day_hours / orbit_hours;
    if (0.95..=1.05).contains(&ratio) {
        "synchronous (tidally locked)"
    } else if day_hours > orbit_hours {
        "slow rotator"
    } else {
        "free rotation"
    }
}

/// Ecosystem / planet-dynamics aggregate card. Rendered immediately
/// after the planet table so the reader sees the run-end species
/// census alongside the planet's static properties. Quiet when no
/// matching events were emitted.
fn render_ecosystem_summary(s: &mut String, d: &Digest) {
    let e = &d.ecosystem;
    let _ = writeln!(s, "## Ecosystem & dynamics");
    let _ = writeln!(s);
    if e.known_species_ids.is_empty()
        && e.catastrophes_by_kind.is_empty()
        && e.mean_resilience_q32.is_none()
    {
        let _ = writeln!(s, "_No ecosystem-level events emitted in this run._");
        let _ = writeln!(s);
        return;
    }
    let _ = writeln!(s, "| Aggregate | Value |");
    let _ = writeln!(s, "|---|---|");
    let _ = writeln!(
        s,
        "| Species count (extant / extinct) | {} / {} |",
        e.extant_species_count(),
        e.extinct_species_count(),
    );
    let _ = writeln!(s, "| Speciation events | {} |", e.speciation_count);
    let _ = writeln!(s, "| Horizontal gene transfer events | {} |", e.hgt_count);
    if let Some(mean) = e.mean_resilience_q32 {
        let _ = writeln!(
            s,
            "| Mean ecological resilience | {:.3} (1.0 = baseline) |",
            q32_to_f64(mean),
        );
    } else {
        let _ = writeln!(s, "| Mean ecological resilience | _(not emitted)_ |");
    }
    if e.catastrophes_by_kind.is_empty() {
        let _ = writeln!(s, "| Catastrophes | none |");
    } else {
        let summary = e
            .catastrophes_by_kind
            .iter()
            .map(|(k, v)| format!("{k}×{v}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(s, "| Catastrophes by kind | {summary} |");
    }
    // Magnetic reversal: the protocol doesn't yet emit a dedicated
    // event for these. Surface a "_(not emitted)_" placeholder so
    // the field is visible — when the protocol grows the event
    // shape later, this row starts reporting real counts.
    let _ = writeln!(
        s,
        "| Magnetic reversal events | {} |",
        if e.magnetic_reversal_events == 0 {
            "_(not emitted)_".to_string()
        } else {
            format!("{}", e.magnetic_reversal_events)
        }
    );
    // Hadley / tidal heating / subsurface ocean temp are all
    // currently internal to sim_physics and not on the wire.
    // Render explicit placeholders so future protocol growth has a
    // place to land without restructuring the table.
    let _ = writeln!(
        s,
        "| Hadley cell count | {} |",
        e.hadley_cell_count
            .map_or_else(|| "_(not emitted)_".to_string(), |c| c.to_string())
    );
    let _ = writeln!(
        s,
        "| Mean Hadley jet velocity | {} |",
        e.mean_hadley_jet_q32.map_or_else(
            || "_(not emitted)_".to_string(),
            |j| format!("{:.2} m/s", q32_to_f64(j))
        )
    );
    let _ = writeln!(
        s,
        "| Total tidal heating budget | {} |",
        e.total_tidal_heating_tw_q32.map_or_else(
            || "_(not emitted)_".to_string(),
            |tw| format!("{:.2} TW", q32_to_f64(tw))
        )
    );
    let _ = writeln!(
        s,
        "| Mean subsurface ocean temp | {} |",
        e.mean_subsurface_temp_k_q32.map_or_else(
            || "_(not emitted)_".to_string(),
            |k| format!("{:.0} K", q32_to_f64(k))
        )
    );
    let _ = writeln!(s);
}
