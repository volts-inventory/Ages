//! Interactive `--config` planet builder. ASCII GM persona: each
//! prompt frames the choice in-character (one sentence of setup),
//! then lists numbered options. Option 0 on every prompt keeps the
//! seed-driven value so the user can skip anything they don't care
//! about. After each pick we surface short implications/warnings so
//! the user sees the consequences before moving on.
//!
//! Map geography (terrain elevation, water depth, sea level,
//! terrain peak, coastline) always comes from `--seed`. Only
//! planet-level scalars are user-overridable here.

use anyhow::{Context, Result};
use sim_world::{
    Atmosphere, BiosphereClass, Crust, Magnetosphere, MetabolicSubstrate, PlanetOverrides,
    SpectralType,
};
use std::io::{BufRead, Write};

const RULE: &str = "════════════════════════════════════════════════════════════";

/// Run the interactive prompt against the given stdin/stdout. Returns
/// the assembled `PlanetOverrides`. Errors propagate I/O failures
/// (broken pipe, EOF on stdin).
pub fn run_interactive<R: BufRead, W: Write>(stdin: &mut R, mut stdout: W) -> Result<PlanetOverrides> {
    let mut o = PlanetOverrides::default();

    writeln!(stdout, "{RULE}")?;
    writeln!(stdout, " THE COSMIC LOOM")?;
    writeln!(stdout, "{RULE}")?;
    writeln!(stdout)?;
    writeln!(stdout, " You stand before a half-spun world. Map and coastlines")?;
    writeln!(stdout, " are already woven by the seed; the bones of geography")?;
    writeln!(stdout, " stay where chance laid them. But the climate, the air,")?;
    writeln!(stdout, " the company in the sky — those are yours to set.")?;
    writeln!(stdout)?;
    writeln!(stdout, " For every question, option 0 keeps the seed's answer.")?;
    writeln!(stdout, " Pick what matters; let the rest drift.")?;
    writeln!(stdout)?;

    o.substrate = prompt_substrate(stdin, &mut stdout)?;
    o.atmosphere = prompt_atmosphere(stdin, &mut stdout, o.substrate)?;
    o.mean_temperature_k = prompt_mean_temperature(stdin, &mut stdout, o.substrate)?;
    o.gravity_g_x100 = prompt_gravity(stdin, &mut stdout)?;
    o.spectral_type = prompt_spectral_type(stdin, &mut stdout)?;
    o.axial_tilt_deg = prompt_axial_tilt(stdin, &mut stdout)?;
    o.day_length_hours = prompt_day_length(stdin, &mut stdout)?;
    o.orbital_period_months = prompt_year_length(stdin, &mut stdout)?;
    o.moon_count = prompt_moon_count(stdin, &mut stdout)?;
    o.magnetosphere = prompt_magnetosphere(stdin, &mut stdout)?;
    o.crust = prompt_crust(stdin, &mut stdout)?;
    o.biosphere = prompt_biosphere(stdin, &mut stdout)?;

    writeln!(stdout)?;
    writeln!(stdout, "{RULE}")?;
    writeln!(stdout, " The loom hums. Your world condenses out of probability…")?;
    writeln!(stdout, "{RULE}")?;
    writeln!(stdout)?;
    stdout.flush()?;
    Ok(o)
}

fn prompt_substrate<R: BufRead, W: Write>(
    stdin: &mut R,
    stdout: &mut W,
) -> Result<Option<MetabolicSubstrate>> {
    writeln!(stdout, "── SUBSTRATE ──")?;
    writeln!(stdout)?;
    writeln!(
        stdout,
        " The very chemistry of life. What solvent will the biosphere"
    )?;
    writeln!(stdout, " run on?")?;
    writeln!(stdout)?;
    writeln!(stdout, "  0) random (let the seed pick)")?;
    writeln!(stdout, "  1) aqueous     — liquid water, the cosmic default")?;
    writeln!(stdout, "  2) ammoniacal  — cold ammonia-based biochemistry")?;
    writeln!(stdout, "  3) hydrocarbon — Titan-style methane/ethane biology")?;
    writeln!(stdout, "  4) silicate    — silicon biochem on molten rock")?;
    writeln!(stdout)?;
    let pick = read_choice(stdin, stdout, 0..=4)?;
    let result = match pick {
        0 => None,
        1 => Some(MetabolicSubstrate::Aqueous),
        2 => Some(MetabolicSubstrate::Ammoniacal),
        3 => Some(MetabolicSubstrate::Hydrocarbon),
        4 => Some(MetabolicSubstrate::Silicate),
        _ => unreachable!(),
    };
    if let Some(s) = result {
        let hint = match s {
            MetabolicSubstrate::Aqueous => "  → liquid solvent wants ~273–373 K on the surface.",
            MetabolicSubstrate::Ammoniacal => "  → ammonia stays liquid roughly 195–240 K.",
            MetabolicSubstrate::Hydrocarbon => "  → methane is liquid in a thin band, ~91–112 K.",
            MetabolicSubstrate::Silicate => "  → silicate biology needs molten rock, ~1700–3500 K.",
        };
        writeln!(stdout, "{hint}")?;
    }
    writeln!(stdout)?;
    Ok(result)
}

fn prompt_atmosphere<R: BufRead, W: Write>(
    stdin: &mut R,
    stdout: &mut W,
    sub: Option<MetabolicSubstrate>,
) -> Result<Option<Atmosphere>> {
    writeln!(stdout, "── ATMOSPHERE ──")?;
    writeln!(stdout)?;
    writeln!(stdout, " The sky overhead. What gases blanket the surface?")?;
    writeln!(stdout)?;
    writeln!(stdout, "  0) random")?;
    writeln!(stdout, "  1) none      — vacuum (silicate biology only)")?;
    writeln!(stdout, "  2) thin      — Mars-like, ~10–30 kPa")?;
    writeln!(stdout, "  3) oxidising — Earth-like, O₂-rich")?;
    writeln!(stdout, "  4) reducing  — CO₂/CH₄ dominant, no free O₂")?;
    writeln!(stdout, "  5) hazy      — Titan-style nitrogen + methane haze")?;
    writeln!(stdout)?;
    let pick = read_choice(stdin, stdout, 0..=5)?;
    let atm = match pick {
        0 => None,
        1 => Some(Atmosphere::None),
        2 => Some(Atmosphere::Thin),
        3 => Some(Atmosphere::Oxidising),
        4 => Some(Atmosphere::Reducing),
        5 => Some(Atmosphere::Hazy),
        _ => unreachable!(),
    };
    // Guided-realism note: vacuum on non-silicate biology is incoherent.
    if matches!(atm, Some(Atmosphere::None))
        && matches!(
            sub,
            Some(
                MetabolicSubstrate::Aqueous
                    | MetabolicSubstrate::Ammoniacal
                    | MetabolicSubstrate::Hydrocarbon
            )
        )
    {
        writeln!(
            stdout,
            "  ⚠ vacuum atmosphere on a non-silicate biosphere will starve the biology. proceeding anyway."
        )?;
    }
    writeln!(stdout)?;
    Ok(atm)
}

fn prompt_mean_temperature<R: BufRead, W: Write>(
    stdin: &mut R,
    stdout: &mut W,
    sub: Option<MetabolicSubstrate>,
) -> Result<Option<i64>> {
    writeln!(stdout, "── MEAN SURFACE TEMPERATURE ──")?;
    writeln!(stdout)?;
    writeln!(stdout, " The thermometer on a typical noon. Kelvin.")?;
    writeln!(stdout)?;
    writeln!(stdout, "  0) random")?;
    writeln!(stdout, "  1) Pluto-like    (40 K)")?;
    writeln!(stdout, "  2) Titan-like    (94 K)")?;
    writeln!(stdout, "  3) Mars-like     (210 K)")?;
    writeln!(stdout, "  4) Earth-like    (288 K)")?;
    writeln!(stdout, "  5) warm tropic   (320 K)")?;
    writeln!(stdout, "  6) sweltering    (370 K)")?;
    writeln!(stdout, "  7) Venus-like    (735 K)")?;
    writeln!(stdout, "  8) molten        (2500 K)")?;
    writeln!(stdout, "  9) custom — enter exact Kelvin")?;
    writeln!(stdout)?;
    let pick = read_choice(stdin, stdout, 0..=9)?;
    let t = match pick {
        0 => None,
        1 => Some(40),
        2 => Some(94),
        3 => Some(210),
        4 => Some(288),
        5 => Some(320),
        6 => Some(370),
        7 => Some(735),
        8 => Some(2500),
        9 => Some(read_integer(stdin, stdout, "Temperature in K", 1, 5000)?),
        _ => unreachable!(),
    };
    // Coherence warnings with substrate.
    if let (Some(t_k), Some(s)) = (t, sub) {
        let (lo, hi) = match s {
            MetabolicSubstrate::Aqueous => (273, 373),
            MetabolicSubstrate::Ammoniacal => (195, 240),
            MetabolicSubstrate::Hydrocarbon => (91, 112),
            MetabolicSubstrate::Silicate => (1700, 3500),
        };
        if t_k < lo || t_k > hi {
            writeln!(
                stdout,
                "  ⚠ {t_k} K is outside the {s:?} liquid window [{lo}, {hi}]. life will struggle. proceeding."
            )?;
        }
    }
    writeln!(stdout)?;
    Ok(t)
}

fn prompt_gravity<R: BufRead, W: Write>(stdin: &mut R, stdout: &mut W) -> Result<Option<i64>> {
    writeln!(stdout, "── SURFACE GRAVITY ──")?;
    writeln!(stdout)?;
    writeln!(stdout, " How heavy does a fallen feather feel?")?;
    writeln!(stdout)?;
    writeln!(stdout, "  0) random")?;
    writeln!(stdout, "  1) Moon-like     (0.17 g)")?;
    writeln!(stdout, "  2) Mars-like     (0.38 g)")?;
    writeln!(stdout, "  3) Earth-like    (1.00 g)")?;
    writeln!(stdout, "  4) super-Earth   (1.50 g)")?;
    writeln!(stdout, "  5) heavy         (2.50 g)")?;
    writeln!(stdout, "  6) crushing      (5.00 g)")?;
    writeln!(stdout, "  7) custom — enter exact g × 100 (e.g. 75 = 0.75 g)")?;
    writeln!(stdout)?;
    let pick = read_choice(stdin, stdout, 0..=7)?;
    let g_x100 = match pick {
        0 => None,
        1 => Some(17),
        2 => Some(38),
        3 => Some(100),
        4 => Some(150),
        5 => Some(250),
        6 => Some(500),
        7 => Some(read_integer(stdin, stdout, "Gravity × 100 (in g)", 1, 2000)?),
        _ => unreachable!(),
    };
    if let Some(g) = g_x100 {
        if g > 300 {
            writeln!(
                stdout,
                "  ⚠ above 3 g most macroscopic life would be flattened. proceeding."
            )?;
        }
    }
    writeln!(stdout)?;
    Ok(g_x100)
}

fn prompt_spectral_type<R: BufRead, W: Write>(
    stdin: &mut R,
    stdout: &mut W,
) -> Result<Option<SpectralType>> {
    writeln!(stdout, "── STELLAR HOST ──")?;
    writeln!(stdout)?;
    writeln!(stdout, " The sun in this sky. Bigger = bluer, hotter, shorter-lived.")?;
    writeln!(stdout)?;
    writeln!(stdout, "  0) random")?;
    writeln!(stdout, "  1) M dwarf — red, dim, lives ~trillions of years")?;
    writeln!(stdout, "  2) K dwarf — orange, gentle, ~50 Gyr lifespan")?;
    writeln!(stdout, "  3) G  type — yellow, sun-like, ~10 Gyr lifespan")?;
    writeln!(stdout, "  4) F  type — yellow-white, hotter, ~3 Gyr")?;
    writeln!(stdout, "  5) A  type — white, hot, ~1 Gyr (race the clock)")?;
    writeln!(stdout)?;
    let pick = read_choice(stdin, stdout, 0..=5)?;
    let result = match pick {
        0 => None,
        1 => Some(SpectralType::M),
        2 => Some(SpectralType::K),
        3 => Some(SpectralType::G),
        4 => Some(SpectralType::F),
        5 => Some(SpectralType::A),
        _ => unreachable!(),
    };
    if matches!(result, Some(SpectralType::M)) {
        writeln!(
            stdout,
            "  → M dwarfs flare hard. expect rough early epochs unless the magnetosphere is strong."
        )?;
    }
    writeln!(stdout)?;
    Ok(result)
}

fn prompt_axial_tilt<R: BufRead, W: Write>(stdin: &mut R, stdout: &mut W) -> Result<Option<i64>> {
    writeln!(stdout, "── AXIAL TILT ──")?;
    writeln!(stdout)?;
    writeln!(stdout, " How tipped is the axis? Bigger tilt = sharper seasons.")?;
    writeln!(stdout)?;
    writeln!(stdout, "  0) random")?;
    writeln!(stdout, "  1) upright       (0°)   — no seasons")?;
    writeln!(stdout, "  2) Earth-like    (23°)  — moderate seasons")?;
    writeln!(stdout, "  3) tilted        (45°)  — extreme seasons")?;
    writeln!(stdout, "  4) Uranus-like   (90°)  — pole points at the sun")?;
    writeln!(stdout, "  5) custom — enter degrees 0–90")?;
    writeln!(stdout)?;
    let pick = read_choice(stdin, stdout, 0..=5)?;
    let result = match pick {
        0 => None,
        1 => Some(0),
        2 => Some(23),
        3 => Some(45),
        4 => Some(90),
        5 => Some(read_integer(stdin, stdout, "Axial tilt °", 0, 90)?),
        _ => unreachable!(),
    };
    writeln!(stdout)?;
    Ok(result)
}

fn prompt_day_length<R: BufRead, W: Write>(stdin: &mut R, stdout: &mut W) -> Result<Option<i64>> {
    writeln!(stdout, "── DAY LENGTH ──")?;
    writeln!(stdout)?;
    writeln!(stdout, " Sidereal day, in hours. A tidally-locked world has a")?;
    writeln!(stdout, " day equal to its year (one face perpetually star-ward).")?;
    writeln!(stdout)?;
    writeln!(stdout, "  0) random")?;
    writeln!(stdout, "  1) breakneck     (6 h)")?;
    writeln!(stdout, "  2) Earth-like    (24 h)")?;
    writeln!(stdout, "  3) long day      (100 h)")?;
    writeln!(stdout, "  4) Venus-like    (2800 h, glacial)")?;
    writeln!(stdout, "  5) custom — enter hours")?;
    writeln!(stdout)?;
    let pick = read_choice(stdin, stdout, 0..=5)?;
    let result = match pick {
        0 => None,
        1 => Some(6),
        2 => Some(24),
        3 => Some(100),
        4 => Some(2800),
        5 => Some(read_integer(stdin, stdout, "Day length in hours", 1, 10000)?),
        _ => unreachable!(),
    };
    writeln!(stdout)?;
    Ok(result)
}

fn prompt_year_length<R: BufRead, W: Write>(stdin: &mut R, stdout: &mut W) -> Result<Option<u32>> {
    writeln!(stdout, "── YEAR LENGTH ──")?;
    writeln!(stdout)?;
    writeln!(stdout, " Months per orbital period. 8 = tight orbit, 16 = wide.")?;
    writeln!(stdout)?;
    writeln!(stdout, "  0) random")?;
    writeln!(stdout, "  1) 8 months   (tight orbit)")?;
    writeln!(stdout, "  2) 10 months")?;
    writeln!(stdout, "  3) 12 months  (Earth-standard)")?;
    writeln!(stdout, "  4) 14 months")?;
    writeln!(stdout, "  5) 16 months  (wide orbit)")?;
    writeln!(stdout)?;
    let pick = read_choice(stdin, stdout, 0..=5)?;
    let result = match pick {
        0 => None,
        1 => Some(8),
        2 => Some(10),
        3 => Some(12),
        4 => Some(14),
        5 => Some(16),
        _ => unreachable!(),
    };
    writeln!(stdout)?;
    Ok(result)
}

fn prompt_moon_count<R: BufRead, W: Write>(stdin: &mut R, stdout: &mut W) -> Result<Option<u8>> {
    writeln!(stdout, "── MOONS ──")?;
    writeln!(stdout)?;
    writeln!(stdout, " How many companions ride the sky?")?;
    writeln!(stdout)?;
    writeln!(stdout, "  0) random")?;
    for i in 0..=4u8 {
        writeln!(stdout, "  {}) {} moon{}", i + 1, i, if i == 1 { "" } else { "s" })?;
    }
    writeln!(stdout)?;
    let pick = read_choice(stdin, stdout, 0..=5)?;
    let result = match pick {
        0 => None,
        n if (1..=5).contains(&n) => Some((n - 1) as u8),
        _ => unreachable!(),
    };
    writeln!(stdout)?;
    Ok(result)
}

fn prompt_magnetosphere<R: BufRead, W: Write>(
    stdin: &mut R,
    stdout: &mut W,
) -> Result<Option<Magnetosphere>> {
    writeln!(stdout, "── MAGNETOSPHERE ──")?;
    writeln!(stdout)?;
    writeln!(stdout, " The planet's magnetic shield. Strong = solar flares")?;
    writeln!(stdout, " bounce off; none = surface bakes in cosmic rays.")?;
    writeln!(stdout)?;
    writeln!(stdout, "  0) random")?;
    writeln!(stdout, "  1) none   — naked surface")?;
    writeln!(stdout, "  2) weak   — modest shield")?;
    writeln!(stdout, "  3) strong — Earth-grade shield")?;
    writeln!(stdout)?;
    let pick = read_choice(stdin, stdout, 0..=3)?;
    let result = match pick {
        0 => None,
        1 => Some(Magnetosphere::None),
        2 => Some(Magnetosphere::Weak),
        3 => Some(Magnetosphere::Strong),
        _ => unreachable!(),
    };
    writeln!(stdout)?;
    Ok(result)
}

fn prompt_crust<R: BufRead, W: Write>(stdin: &mut R, stdout: &mut W) -> Result<Option<Crust>> {
    writeln!(stdout, "── CRUST MINERAL ──")?;
    writeln!(stdout)?;
    writeln!(stdout, " What's locked in the rock? Crust drives what tech")?;
    writeln!(stdout, " the civs eventually unlock.")?;
    writeln!(stdout)?;
    writeln!(stdout, "  0) random")?;
    writeln!(stdout, "  1) basaltic      — Earth-default, balanced")?;
    writeln!(stdout, "  2) hydrocarbon   — oil/coal/methane clathrate")?;
    writeln!(stdout, "  3) piezoelectric — quartz/tourmaline, electric tech")?;
    writeln!(stdout, "  4) ferrous       — iron-rich, metalworking edge")?;
    writeln!(stdout, "  5) rare_earth    — lanthanide-rich, advanced electronics")?;
    writeln!(stdout)?;
    let pick = read_choice(stdin, stdout, 0..=5)?;
    let result = match pick {
        0 => None,
        1 => Some(Crust::Basaltic),
        2 => Some(Crust::Hydrocarbon),
        3 => Some(Crust::Piezoelectric),
        4 => Some(Crust::Ferrous),
        5 => Some(Crust::RareEarth),
        _ => unreachable!(),
    };
    writeln!(stdout)?;
    Ok(result)
}

fn prompt_biosphere<R: BufRead, W: Write>(
    stdin: &mut R,
    stdout: &mut W,
) -> Result<Option<BiosphereClass>> {
    writeln!(stdout, "── BIOSPHERE RICHNESS ──")?;
    writeln!(stdout)?;
    writeln!(stdout, " How abundant is life at the start?")?;
    writeln!(stdout)?;
    writeln!(stdout, "  0) random")?;
    writeln!(stdout, "  1) sparse           — thin life, harsh world")?;
    writeln!(stdout, "  2) lush             — Earth-like fecundity")?;
    writeln!(stdout, "  3) hyperbiodiverse  — Cambrian-explosion conditions")?;
    writeln!(stdout)?;
    let pick = read_choice(stdin, stdout, 0..=3)?;
    let result = match pick {
        0 => None,
        1 => Some(BiosphereClass::Sparse),
        2 => Some(BiosphereClass::Lush),
        3 => Some(BiosphereClass::HyperBiodiverse),
        _ => unreachable!(),
    };
    writeln!(stdout)?;
    Ok(result)
}

fn read_choice<R: BufRead, W: Write>(
    stdin: &mut R,
    stdout: &mut W,
    range: std::ops::RangeInclusive<u32>,
) -> Result<u32> {
    loop {
        write!(stdout, " > ")?;
        stdout.flush()?;
        let mut line = String::new();
        stdin.read_line(&mut line).context("stdin closed")?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Empty line = accept default (0 = random).
            return Ok(0);
        }
        match trimmed.parse::<u32>() {
            Ok(n) if range.contains(&n) => return Ok(n),
            _ => writeln!(
                stdout,
                " (please enter a number between {} and {}.)",
                range.start(),
                range.end()
            )?,
        }
    }
}

fn read_integer<R: BufRead, W: Write>(
    stdin: &mut R,
    stdout: &mut W,
    label: &str,
    lo: i64,
    hi: i64,
) -> Result<i64> {
    loop {
        write!(stdout, " {label} ({lo}..={hi}) > ")?;
        stdout.flush()?;
        let mut line = String::new();
        stdin.read_line(&mut line).context("stdin closed")?;
        match line.trim().parse::<i64>() {
            Ok(n) if n >= lo && n <= hi => return Ok(n),
            _ => writeln!(stdout, " (please enter an integer between {lo} and {hi}.)")?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Scripted run: pick the third option on every prompt + the
    /// default on numeric custom entries. Smoke-tests the prompt
    /// loop and verifies the override struct picks up the expected
    /// values without exercising real stdin.
    #[test]
    fn smoke_prompts_walk_through() {
        // Build a script line per prompt. Picks the second non-random
        // option on every menu so we exercise the conversion branches.
        let script = "2\n3\n4\n3\n3\n2\n2\n3\n2\n3\n1\n2\n";
        let mut stdin = Cursor::new(script);
        let mut stdout: Vec<u8> = Vec::new();
        let o = run_interactive(&mut stdin, &mut stdout).expect("prompts should succeed");
        assert_eq!(o.substrate, Some(MetabolicSubstrate::Ammoniacal));
        assert_eq!(o.atmosphere, Some(Atmosphere::Oxidising));
        assert_eq!(o.mean_temperature_k, Some(288)); // Earth-like (pick 4)
        assert_eq!(o.gravity_g_x100, Some(100)); // Earth-like (pick 3)
        assert_eq!(o.spectral_type, Some(SpectralType::G));
        assert_eq!(o.axial_tilt_deg, Some(23));
        assert_eq!(o.day_length_hours, Some(24));
        assert_eq!(o.orbital_period_months, Some(12));
        assert_eq!(o.moon_count, Some(1));
        assert_eq!(o.magnetosphere, Some(Magnetosphere::Strong));
        assert_eq!(o.crust, Some(Crust::Basaltic));
        assert_eq!(o.biosphere, Some(BiosphereClass::Lush));
    }

    /// All-zero script means every prompt accepts "random" → empty
    /// overrides, sim falls back to pure seed-driven sampling.
    #[test]
    fn zero_picks_all_random() {
        let script = "0\n".repeat(12);
        let mut stdin = Cursor::new(script);
        let mut stdout: Vec<u8> = Vec::new();
        let o = run_interactive(&mut stdin, &mut stdout).expect("prompts should succeed");
        assert!(o.substrate.is_none());
        assert!(o.atmosphere.is_none());
        assert!(o.mean_temperature_k.is_none());
        assert!(o.gravity_g_x100.is_none());
        assert!(o.spectral_type.is_none());
        assert!(o.axial_tilt_deg.is_none());
        assert!(o.day_length_hours.is_none());
        assert!(o.orbital_period_months.is_none());
        assert!(o.moon_count.is_none());
        assert!(o.magnetosphere.is_none());
        assert!(o.crust.is_none());
        assert!(o.biosphere.is_none());
    }

    /// Coherence warning fires when substrate + temperature disagree.
    #[test]
    fn substrate_temp_mismatch_warns() {
        // Aqueous substrate + Pluto temperature (40 K). Should print a
        // ⚠ note about being outside the liquid window.
        let script = "1\n0\n1\n0\n0\n0\n0\n0\n0\n0\n0\n0\n";
        let mut stdin = Cursor::new(script);
        let mut stdout: Vec<u8> = Vec::new();
        run_interactive(&mut stdin, &mut stdout).unwrap();
        let out = String::from_utf8(stdout).unwrap();
        assert!(out.contains("outside the Aqueous liquid window"));
    }
}
