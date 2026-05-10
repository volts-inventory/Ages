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
        let _ = writeln!(
            s,
            "| Stellar luminosity | {:.0} W/m² |",
            q32_to_f64(p.stellar_luminosity_q32)
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
    } else {
        let _ = writeln!(s, "_(planet event not emitted)_");
    }
    let _ = writeln!(s);

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
