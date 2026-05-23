//! Radiation-law tests, extracted from the original monolithic
//! `radiation.rs`. Covers: bare equilibrium / seasons /
//! eccentricity, per-substance greenhouse forcing (h2o / co2 / ch4),
//! the Venus runaway plateau calibration, and the synchronous-lock
//! day-night gradient.

use super::greenhouse::greenhouse_cap_scaled;
use super::*;
use crate::chemistry::Substance;
use crate::grid::HexGrid;
use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::Real;

#[test]
fn equator_warmer_than_pole_at_equinox() {
    let rad = Radiation::for_planet(
        8,
        Real::from_int(1_361),
        30,
        Real::ZERO,
        0, // no tilt → no seasonal shift
        360,
        0,                             // circular orbit for tests
        sim_arith::Real::from_int(24), // Earth-like 24h day
    );
    // With zero tilt, every season's column should put the
    // hottest band at the equator (mid-grid).
    let mid = rad.t_eq_per_row_per_season[4][0];
    let pole = rad.t_eq_per_row_per_season[0][0];
    assert!(mid > pole);
}

#[test]
fn relaxation_pulls_cell_toward_equilibrium() {
    let mut state = PhysicsState::new(HexGrid::new(3, 3));
    for t in state.temperature_mut() {
        *t = Real::from_int(500);
    }
    let rad = Radiation::for_planet(
        3,
        Real::from_int(1_361),
        30,
        Real::ZERO,
        0,
        360,
        0,                             // circular orbit for tests
        sim_arith::Real::from_int(24), // Earth-like 24h day
    );
    let initial = state.temperature()[0];
    let row_eq = rad.t_eq_per_row_per_season[0][0];
    for _ in 0..100 {
        rad.integrate(&mut state, Real::ONE);
    }
    let final_t = state.temperature()[0];
    let initial_gap = (row_eq - initial).abs();
    let final_gap = (row_eq - final_t).abs();
    // The cell must have moved toward equilibrium.
    assert!(final_gap < initial_gap);
}

#[test]
fn season_index_advances_with_macro_step() {
    let rad = Radiation::for_planet(
        3,
        Real::from_int(1_361),
        30,
        Real::ZERO,
        23,
        360,                           // 12 months × 30 macro-steps
        0,                             // circular orbit
        sim_arith::Real::from_int(24), // 24h day
    );
    assert_eq!(rad.season_index(0), 0);
    assert_eq!(rad.season_index(30), 1);
    assert_eq!(rad.season_index(180), 6);
    assert_eq!(rad.season_index(360), 0); // wraps
}

#[test]
fn northern_hemisphere_warmer_in_n_summer_than_s_summer() {
    // With axial tilt, a northern-hemisphere row's T_eq
    // should peak in N-summer (season 0) and trough in
    // S-summer (season 6).
    let rad = Radiation::for_planet(
        9, // tall enough that tilt produces ≥1 row offset
        Real::from_int(1_361),
        30,
        Real::ZERO,
        45, // strong tilt — guaranteed nonzero axial_tilt_rows
        360,
        0,                             // circular orbit for tests
        sim_arith::Real::from_int(24), // Earth-like 24h day
    );
    // Row 1 sits firmly in the northern hemisphere (row 4 = mid).
    let n_summer = rad.t_eq_per_row_per_season[1][0];
    let s_summer = rad.t_eq_per_row_per_season[1][6];
    assert!(
        n_summer > s_summer,
        "N hemisphere row should warm in N summer relative to S summer: \
         n_summer={n_summer:?} s_summer={s_summer:?}"
    );
}

#[test]
fn no_tilt_means_no_seasonal_swing() {
    let rad = Radiation::for_planet(
        9,
        Real::from_int(1_361),
        30,
        Real::ZERO,
        0, // no tilt
        360,
        0,                             // circular orbit for tests
        sim_arith::Real::from_int(24), // Earth-like 24h day
    );
    // Every season's table column should be identical when
    // tilt is zero.
    for r in 0..9 {
        let s0 = rad.t_eq_per_row_per_season[r][0];
        let s6 = rad.t_eq_per_row_per_season[r][6];
        assert_eq!(s0, s6, "row {r}: tilt=0 must give identical seasons");
    }
}

#[test]
fn perihelion_warmer_than_aphelion_for_eccentric_orbit() {
    // With zero tilt and high eccentricity, perihelion (season 0)
    // should be hotter than aphelion (season 6) at every row.
    let rad = Radiation::for_planet(
        5,
        Real::from_int(1_361),
        30,
        Real::ZERO,
        0, // no tilt — isolate eccentricity effect
        360,
        30,                 // e = 0.30 (highly eccentric)
        Real::from_int(24), // 24h day
    );
    for r in 0..5 {
        let perihelion = rad.t_eq_per_row_per_season[r][0];
        let aphelion = rad.t_eq_per_row_per_season[r][6];
        assert!(
            perihelion > aphelion,
            "row {r}: perihelion T_eq should exceed aphelion T_eq with e=0.30: \
             perihelion={perihelion:?} aphelion={aphelion:?}"
        );
    }
}

#[test]
fn tidally_locked_planet_has_diurnal_amplitude_one() {
    // A tidally-locked planet (day_length >= 1000h)
    // gets full diurnal amplitude. Earth-like (24h) gets 0.
    let earth_like = Radiation::for_planet(
        5,
        Real::from_int(1_361),
        30,
        Real::ZERO,
        0,
        360,
        0,
        Real::from_int(24),
    );
    assert_eq!(earth_like.diurnal_amplitude, Real::ZERO);

    let tidally_locked = Radiation::for_planet(
        5,
        Real::from_int(1_361),
        30,
        Real::ZERO,
        0,
        360,
        0,
        Real::from_int(1_500),
    );
    assert_eq!(tidally_locked.diurnal_amplitude, Real::ONE);
}

#[test]
fn diurnal_modulation_warms_day_side_cools_night_side() {
    // Tidally-locked planet → permanent day side (sub-solar
    // longitude fixed at q=0) gets warmer than the antipodal
    // night side. Run from a uniform initial T and verify.
    let rad = Radiation::for_planet(
        5,
        Real::from_int(1_361),
        30,
        Real::ZERO,
        0,
        0,                     // no seasonal swing
        0,                     // circular orbit
        Real::from_int(2_000), // tidally locked
    );
    let mut state = PhysicsState::new(HexGrid::new(8, 1));
    for t in state.temperature_mut() {
        *t = Real::from_int(280);
    }
    // Run many ticks to reach quasi-steady state.
    for _ in 0..200 {
        rad.integrate(&mut state, Real::ONE);
    }
    let day_cell = state.temperature()[0];
    let night_cell = state.temperature()[4]; // q=4 is antipodal for width=8
    assert!(
        day_cell > night_cell,
        "tidally-locked day side should be warmer than night side: \
         day={day_cell:?} night={night_cell:?}"
    );
}

// P1.5 — synchronous-lock day-night gradient. These tests
// verify that a `LockingMode::Synchronous` planet develops a
// permanent hot day side / cold night side anchored on the
// sub-stellar point, that a `LockingMode::Other` planet stays
// zonally symmetric (rotation washes any gradient out), and
// that the terminator zone lands between the two extremes.

#[test]
fn synchronous_planet_has_hot_day_side_cold_night_side() {
    // A `Synchronous` planet's sub-stellar point is fixed at
    // (lat=0, lon=0); the cell closest to that point should
    // sit at a higher equilibrium T than the cell at the
    // antistellar point (lon=0.5). We use a 1-row grid to
    // remove latitudinal cooling — the only modulator left is
    // the day-night gradient, so any T separation must come
    // from the synchronous code path.
    let rad = Radiation::for_planet(
        1,
        Real::from_int(1_361),
        30,
        Real::ZERO,
        0,    // no axial tilt
        0,    // no seasonal swing
        0,    // circular orbit
        Real::from_int(24), // day length irrelevant under
                            // Synchronous mode (the fixed
                            // sub-stellar point is what
                            // drives the gradient)
    )
    .with_locking(LockingMode::Synchronous, Real::ZERO, Real::ZERO);
    let mut state = PhysicsState::new(HexGrid::new(8, 1));
    for t in state.temperature_mut() {
        *t = Real::from_int(280);
    }
    // Run many ticks to reach quasi-steady state. The
    // relaxation rate is ~2 % per tick (50-tick e-folding),
    // so 500 ticks brings every cell to within 5e-5 of its
    // (now per-cell) equilibrium.
    for _ in 0..500 {
        rad.integrate(&mut state, Real::ONE);
    }
    // Sub-stellar cell sits at q=0 (cos_angle=1, full
    // insolation). Antistellar at q=4 for width=8
    // (cos_angle=-1, clamped → night floor).
    let day_cell = state.temperature()[0];
    let night_cell = state.temperature()[4];
    assert!(
        day_cell > night_cell + Real::from_int(50),
        "synchronous-lock day side must be much hotter than night side: \
         day={day_cell:?} night={night_cell:?}"
    );
}

#[test]
fn free_rotator_has_zonal_symmetric_radiation() {
    // Same planet parameters as the synchronous test above,
    // but in `LockingMode::Other` (free rotator). With Earth-
    // like day length the diurnal amplitude is zero (fast
    // rotators average out at our macro-step resolution), so
    // all cells at the same latitude should converge to the
    // same equilibrium T — no day/night gradient.
    let rad = Radiation::for_planet(
        1,
        Real::from_int(1_361),
        30,
        Real::ZERO,
        0,
        0,
        0,
        Real::from_int(24),
    )
    .with_locking(LockingMode::Other, Real::ZERO, Real::ZERO);
    let mut state = PhysicsState::new(HexGrid::new(8, 1));
    for t in state.temperature_mut() {
        *t = Real::from_int(280);
    }
    for _ in 0..500 {
        rad.integrate(&mut state, Real::ONE);
    }
    // Every cell in this 1-row grid sits at the equator;
    // under `LockingMode::Other` they should converge to the
    // same T. Tolerance ~1 K covers Q32.32 LSB rounding and
    // the residual gap from the relaxation pull.
    let t0 = state.temperature()[0];
    for q in 1..8 {
        let tq = state.temperature()[q];
        let diff = (tq - t0).abs();
        assert!(
            diff < Real::from_int(1),
            "free-rotator row should be zonally symmetric: q=0 T={t0:?}, q={q} T={tq:?}, diff={diff:?}"
        );
    }
}

#[test]
fn terminator_zone_has_moderate_temperature() {
    // The terminator is the 90°-from-substellar great-circle
    // band — half the cell sees the star and half doesn't. A
    // cell sitting on the terminator should have an
    // equilibrium T between the hottest day-side cell (sub-
    // stellar) and the coldest night-side cell (antistellar).
    //
    // Use a 5-row grid so we can include latitude variation,
    // then check the terminator row's temperature is between
    // the day and night extremes.
    let rad = Radiation::for_planet(
        1,
        Real::from_int(1_361),
        30,
        Real::ZERO,
        0,
        0,
        0,
        Real::from_int(24),
    )
    .with_locking(LockingMode::Synchronous, Real::ZERO, Real::ZERO);
    let mut state = PhysicsState::new(HexGrid::new(8, 1));
    for t in state.temperature_mut() {
        *t = Real::from_int(280);
    }
    for _ in 0..500 {
        rad.integrate(&mut state, Real::ONE);
    }
    // For width=8 with sub-stellar at q=0: q=2 sits at
    // longitude 2/8 = 0.25 turns = 90° east — exactly on the
    // terminator. q=6 (270° = -90°) is the other terminator.
    // Both should land strictly between the day-side cell
    // (q=0) and the night-side cell (q=4).
    let day = state.temperature()[0];
    let term_east = state.temperature()[2];
    let term_west = state.temperature()[6];
    let night = state.temperature()[4];
    assert!(
        term_east < day && term_east > night,
        "east terminator should sit between day and night: \
         day={day:?} term={term_east:?} night={night:?}"
    );
    assert!(
        term_west < day && term_west > night,
        "west terminator should sit between day and night: \
         day={day:?} term={term_west:?} night={night:?}"
    );
    // The two terminators sit at symmetric great-circle
    // distances from the sub-stellar point, so their
    // equilibrium temperatures should match (within Q32.32
    // LSB rounding).
    let diff = (term_east - term_west).abs();
    assert!(
        diff < Real::from_int(1),
        "east and west terminators should be symmetric: \
         east={term_east:?} west={term_west:?} diff={diff:?}"
    );
}

#[test]
fn circular_orbit_no_eccentricity_swing() {
    let rad = Radiation::for_planet(
        5,
        Real::from_int(1_361),
        30,
        Real::ZERO,
        0,
        360,
        0,                  // circular
        Real::from_int(24), // 24h day
    );
    for r in 0..5 {
        let s0 = rad.t_eq_per_row_per_season[r][0];
        let s6 = rad.t_eq_per_row_per_season[r][6];
        assert_eq!(
            s0, s6,
            "row {r}: e=0 + tilt=0 must give identical perihelion/aphelion"
        );
    }
}

// Sprint 3 Item 14 — Per-substance greenhouse coupling tests.

#[test]
fn h2o_vapour_contributes_to_per_cell_greenhouse() {
    // Direct check that per-cell greenhouse increases T_eq by
    // the vapour-weighted constant. Two states differing only
    // in vapour density; the vapour-rich one warms faster
    // toward equilibrium because its T_eq is higher.
    let rad = Radiation::for_planet(
        3,
        Real::from_int(1_361),
        30,
        Real::ZERO,
        0,
        0, // no seasons
        0,
        Real::from_int(24),
    );
    let mut dry = PhysicsState::new(HexGrid::new(3, 3));
    let mut wet = PhysicsState::new(HexGrid::new(3, 3));
    for t in dry.temperature_mut() {
        *t = Real::from_int(280);
    }
    for t in wet.temperature_mut() {
        *t = Real::from_int(280);
    }
    // Seed only the wet state with vapour. A vapour density
    // of 50,000 × H2O_GREENHOUSE_K (0.002) = +100 K of
    // equilibrium greenhouse forcing — large enough that a
    // single 2%-relaxation tick produces a measurable T
    // shift.
    for v in wet.substance_mut(Substance::Vapour.idx()) {
        *v = Real::from_int(50_000);
    }
    // One integrate. Wet state's per-cell T should advance
    // further toward (higher) T_eq than dry state's.
    rad.integrate(&mut dry, Real::ONE);
    rad.integrate(&mut wet, Real::ONE);
    let dry_t = dry.temperature()[0];
    let wet_t = wet.temperature()[0];
    assert!(
        wet_t > dry_t,
        "vapour-rich cell should warm faster: dry={dry_t:?} wet={wet_t:?}"
    );
}

#[test]
fn co2_contributes_linearly_to_greenhouse() {
    // CO2's contribution is per-density × `co2_greenhouse_k`
    // (5.0 K per unit post-fix-C3). A modest CO2 load should
    // still produce a measurable T_eq lift; the greenhouse cap
    // clamps the contribution at 250 K for very dense columns.
    let rad = Radiation::for_planet(
        3,
        Real::from_int(1_361),
        30,
        Real::ZERO,
        0,
        0,
        0,
        Real::from_int(24),
    );
    let mut without_co2 = PhysicsState::new(HexGrid::new(3, 3));
    let mut with_co2 = PhysicsState::new(HexGrid::new(3, 3));
    for t in without_co2.temperature_mut() {
        *t = Real::from_int(280);
    }
    for t in with_co2.temperature_mut() {
        *t = Real::from_int(280);
    }
    for v in with_co2.substance_mut(Substance::CO2.idx()) {
        // 5000 × 5.0 = 25,000 K raw, clamped to the 250 K cap;
        // still well above the no-CO2 baseline so the assertion
        // below (with_co2 warmer than without_co2) holds.
        *v = Real::from_int(5_000);
    }
    rad.integrate(&mut without_co2, Real::ONE);
    rad.integrate(&mut with_co2, Real::ONE);
    assert!(
        with_co2.temperature()[0] > without_co2.temperature()[0],
        "CO2-rich cell should warm faster: without={:?} with={:?}",
        without_co2.temperature()[0],
        with_co2.temperature()[0]
    );
}

#[test]
fn ch4_decays_via_photolysis() {
    // CH4 should decay each tick via the photolysis factor
    // (~0.999/tick). Verify a seeded methane column shrinks
    // after a single integrate, and shrinks further over many.
    let rad = Radiation::for_planet(
        3,
        Real::from_int(1_361),
        30,
        Real::ZERO,
        0,
        0,
        0,
        Real::from_int(24),
    );
    let mut state = PhysicsState::new(HexGrid::new(3, 3));
    for v in state.substance_mut(Substance::Methane.idx()) {
        *v = Real::from_int(100);
    }
    let initial = state.substance(Substance::Methane.idx())[0];
    rad.integrate(&mut state, Real::ONE);
    let after_one = state.substance(Substance::Methane.idx())[0];
    assert!(
        after_one < initial,
        "CH4 should decay after one tick: initial={initial:?} after_one={after_one:?}"
    );
    for _ in 0..99 {
        rad.integrate(&mut state, Real::ONE);
    }
    let after_hundred = state.substance(Substance::Methane.idx())[0];
    assert!(
        after_hundred < after_one,
        "CH4 should keep decaying: after_one={after_one:?} after_hundred={after_hundred:?}"
    );
}

#[test]
fn hot_seed_slides_into_venus_state_via_h2o_runaway() {
    // Per Sprint 3 Item 14 acceptance criteria: a hot seed
    // (350 K) with vapour-rich atmosphere coupled to the
    // saturation cap should slide into a Venus-style
    // runaway. Final mean T must exceed 400 K — the
    // signature that the C-C-coupled feedback actually took
    // hold rather than cells relaxing to a hot equilibrium.
    //
    // The atmosphere-baseline `greenhouse_k` is zero — *all*
    // greenhouse forcing comes from the per-substance dynamic
    // term. That isolates the runaway signal from atmosphere-
    // class forcing.
    //
    // 1-row grid removes latitudinal cooling (the runaway
    // feedback is global, not latitude-dependent).
    //
    // Vapour pegged to `sat_cap(T)` each tick mimics "vapour
    // equilibrates instantly with the saturation cap" — the
    // limit a much-larger-than-hydrology-timescale tick
    // approximates. Without a peg the test would need
    // geological timescales (millions of ticks) for
    // hydrology's deliberately-slow sub-boil evap (Sprint 1
    // Item 4) to fill the cap as T climbs; the peg compresses
    // that into the test horizon.
    let rad = Radiation::for_planet(
        1,
        Real::from_int(2_500),    // strong stellar input
        10,                       // low albedo (heat-absorbing)
        Real::ZERO,               // baseline greenhouse=0
        0,
        0,                        // no seasons
        0,
        Real::from_int(24),
    );
    let mut state = PhysicsState::new(HexGrid::new(3, 1));
    for t in state.temperature_mut() {
        *t = Real::from_int(350);
    }
    // Initial vapour seeded at sat_cap(350); will be re-
    // pegged each tick.
    for v in state.substance_mut(Substance::Vapour.idx()) {
        *v = crate::hydrology::saturation_vapour_cap(Real::from_int(350));
    }
    for _ in 0..2_000 {
        rad.integrate(&mut state, Real::ONE);
        // Peg vapour to current sat_cap(T) per cell.
        let temps = state.temperature().to_vec();
        for (i, v) in state
            .substance_mut(Substance::Vapour.idx())
            .iter_mut()
            .enumerate()
        {
            *v = crate::hydrology::saturation_vapour_cap(temps[i]);
        }
    }
    let final_mean_t = {
        let temps = state.temperature();
        let mut sum = Real::ZERO;
        for t in temps {
            sum = sum + *t;
        }
        sum / Real::from_int(i64::try_from(temps.len()).unwrap_or(1))
    };
    assert!(
        final_mean_t > Real::from_int(400),
        "H2O runaway should drive mean T above 400 K; got {final_mean_t:?}"
    );
}

#[test]
fn runaway_threshold_at_published_t_temp() {
    // Komabayashi-Ingersoll-style threshold: under Earth-like
    // stellar input + a near-saturation initial atmosphere
    // the climate stays sub-runaway (mean T plateau well
    // below 400 K). Boost stellar irradiance significantly
    // and the H2O runaway feedback tips the planet into a
    // Venus-style hot state.
    //
    // Run both planets from the same temperate seed (290 K)
    // with identical vapour loads pegged to `sat_cap(T)`
    // each tick (the C-C feedback path); only stellar
    // irradiance differs. The earth-like one settles into a
    // temperate equilibrium; the boosted one diverges past
    // 400 K.
    //
    // The boost magnitude in this test (~85 %) is larger
    // than the real-Earth K-I threshold (~10 % above modern
    // solar) because the per-cell greenhouse coefficients
    // and the [`greenhouse_cap_scaled`] ceiling are tuned for
    // the simulation's Q32.32 range rather than calibrated
    // against the real K-I value. The qualitative bistable
    // threshold — temperate equilibrium vs. runaway — is
    // what matters; the exact tipping irradiance is a tuning
    // choice tied to the rest of the constants.
    let earth_like_irradiance = Real::from_int(1_361);
    let boosted_irradiance = Real::from_int(2_500);
    // 1-row grid so no latitude variation contaminates the
    // mean-T signal (the runaway feedback is global, not
    // latitude-dependent).
    let build = |stellar: Real| {
        Radiation::for_planet(
            1,
            stellar,
            10,
            Real::ZERO,
            0,
            0,
            0,
            Real::from_int(24),
        )
    };
    let seed_state = || {
        let mut s = PhysicsState::new(HexGrid::new(3, 1));
        for t in s.temperature_mut() {
            *t = Real::from_int(290);
        }
        // Initial vapour at sat_cap(290); the peg loop below
        // re-evaluates each tick (the C-C feedback path).
        for v in s.substance_mut(Substance::Vapour.idx()) {
            *v = crate::hydrology::saturation_vapour_cap(Real::from_int(290));
        }
        s
    };
    // Run radiation only with a per-tick vapour peg to
    // `sat_cap(T)` — same approach as the runaway test, for
    // the same reason: hydrology's deliberately-slow sub-
    // boil evap (Sprint 1 Item 4) would need geological
    // timescales to fill the rising sat_cap as T climbs.
    // The peg compresses the C-C feedback into the test
    // horizon. Both states see the same dynamics; only
    // stellar irradiance differs.
    let run = |rad: &Radiation, state: &mut PhysicsState| -> Real {
        for _ in 0..2_000 {
            rad.integrate(state, Real::ONE);
            let temps = state.temperature().to_vec();
            for (i, v) in state
                .substance_mut(Substance::Vapour.idx())
                .iter_mut()
                .enumerate()
            {
                *v = crate::hydrology::saturation_vapour_cap(temps[i]);
            }
        }
        let temps = state.temperature();
        let mut sum = Real::ZERO;
        for t in temps {
            sum = sum + *t;
        }
        sum / Real::from_int(i64::try_from(temps.len()).unwrap_or(1))
    };
    let mut earth_state = seed_state();
    let mut boosted_state = seed_state();
    let rad_earth = build(earth_like_irradiance);
    let rad_boost = build(boosted_irradiance);
    let earth_mean = run(&rad_earth, &mut earth_state);
    let boost_mean = run(&rad_boost, &mut boosted_state);
    assert!(
        earth_mean < Real::from_int(360),
        "earth-like stellar input should not run away; got {earth_mean:?}"
    );
    assert!(
        boost_mean > Real::from_int(400),
        "boosted stellar input should trigger runaway; got {boost_mean:?}"
    );
}

// T11 (any-planet backlog) — Venus runaway plateau calibration.
//
// Literature anchor: Venus surface T ~ 735 K under ~2613 W/m²
// stellar irradiance and a 90-bar CO2 atmosphere, with the H2O
// runaway-greenhouse Komabayashi-Ingersoll plateau falling in
// the 700-770 K band. The simulation's greenhouse coupling
// (`co2_greenhouse_k` + `h2o_greenhouse_k` + C-C-coupled vapour
// cap) and bounding cap (`greenhouse_cap_scaled`, pressure-
// scaled per calibration fix C1) lets a Venus-equivalent
// planet settle on (or near) that plateau when the surface
// pressure is set to the Venus 90-bar value via
// `Radiation::with_surface_pressure`.
//
// With the pressure-scaled cap (`250 + 100 × log10(P/P_earth)`,
// clamped to `[50, 600]`), Venus's 9.2×10⁶ Pa column gives a
// cap of ~446 K above the bare ~309 K T_eq → ~755 K plateau,
// squarely in the literature band.
#[test]
fn venus_runaway_plateau_t_in_700_to_770_k() {
    // Venus-equivalent radiation environment:
    //   - stellar irradiance ~2600 W/m² at Venus's orbit
    //   - low albedo so we maximise net stellar absorption
    //     (Venus's *real* albedo is high, ~0.75, because of
    //     sulphuric-acid clouds — but the greenhouse + atmospheric
    //     dynamics keep the *surface* hot; for this test we drop
    //     albedo so the surface forcing matches the surface-
    //     temperature anchor rather than top-of-atmosphere
    //     balance, which the model doesn't separate)
    //   - zero atmosphere-class baseline greenhouse; the per-cell
    //     dynamic term carries all greenhouse forcing so the
    //     calibration target sits purely on
    //     `co2_greenhouse_k` × CO2 + `h2o_greenhouse_k` × vapour
    //     + the pressure-scaled saturation cap.
    //   - surface pressure pinned to Venus's ~9.2×10⁶ Pa
    //     (≈90.8 bar) so the pressure-scaled cap lifts to
    //     ~446 K (calibration fix C1).
    let rad = Radiation::for_planet(
        1,
        Real::from_int(2_600),
        10,
        Real::ZERO,
        0,
        0,
        0,
        Real::from_int(24),
    )
    .with_surface_pressure(Real::from_int(9_200_000));
    let mut state = PhysicsState::new(HexGrid::new(3, 1));
    // Seed at 500 K so the runaway path triggers immediately
    // (vapour cap is already well above the K-I threshold at
    // 500 K). The plateau is asymptotic — initial T just sets
    // how fast we reach it, not where it lands.
    for t in state.temperature_mut() {
        *t = Real::from_int(500);
    }
    // Dense CO2 column — Venus has ~90 bar CO2, ~2000× Earth's
    // total atmospheric column. Pegging CO2 to a high value so
    // the per-cell CO2 greenhouse term saturates the cap
    // independent of any biogeochemistry.
    for v in state.substance_mut(Substance::CO2.idx()) {
        *v = Real::from_int(1_000_000);
    }
    // Initial vapour seeded at sat_cap(500). The per-tick peg
    // below keeps vapour at sat_cap(T) so the C-C-coupled
    // feedback path stays armed (same approach as the
    // `hot_seed_slides_into_venus_state_via_h2o_runaway`
    // baseline test).
    for v in state.substance_mut(Substance::Vapour.idx()) {
        *v = crate::hydrology::saturation_vapour_cap(Real::from_int(500));
    }
    // 5000 ticks to reach steady state. The relaxation
    // timescale is ~50 ticks (2 %/tick), so 5000 ticks =
    // ~100 e-foldings — fully converged.
    for _ in 0..5_000 {
        rad.integrate(&mut state, Real::ONE);
        // Re-peg vapour to sat_cap(T) each tick (the C-C
        // feedback path). Re-peg CO2 too so the photolysis /
        // chemistry channels can't bleed it down inside this
        // radiation-only test.
        let temps = state.temperature().to_vec();
        for (i, v) in state
            .substance_mut(Substance::Vapour.idx())
            .iter_mut()
            .enumerate()
        {
            *v = crate::hydrology::saturation_vapour_cap(temps[i]);
        }
        for v in state.substance_mut(Substance::CO2.idx()) {
            *v = Real::from_int(1_000_000);
        }
    }
    let final_mean_t = {
        let temps = state.temperature();
        let mut sum = Real::ZERO;
        for t in temps {
            sum = sum + *t;
        }
        sum / Real::from_int(i64::try_from(temps.len()).unwrap_or(1))
    };
    // Literature plateau: T ∈ [700, 770] K. With the
    // pressure-scaled cap (calibration fix C1) Venus's 90-bar
    // surface pressure lifts the greenhouse ceiling from the
    // legacy 250 K to ~446 K, putting the saturated runaway
    // plateau in the literature band.
    //
    // Decomposition: T_eq_base for stellar=2600 W/m²,
    // albedo=10 %, equator 1-row grid:
    // `(2600 × 0.9 × 1.0 × 0.25 / σ)^(1/4) ≈ 309 K`, plus the
    // pressure-scaled cap `250 + 100 × log10(9.2×10⁶ /
    // 101325) ≈ 446 K`, gives ~755 K — squarely in the band.
    let plateau_lo = Real::from_int(700);
    let plateau_hi = Real::from_int(770);
    assert!(
        final_mean_t >= plateau_lo && final_mean_t <= plateau_hi,
        "Venus-equivalent plateau out of literature 700-770 K band: \
         got {final_mean_t:?}"
    );
}

// Calibration fix C1 — verify `greenhouse_cap_scaled` honours
// the documented anchors: Earth pressure → ~250 K (legacy
// baseline preserved), Venus pressure → ~446 K (lifts the
// runaway plateau into the literature 700-770 K band).
#[test]
fn greenhouse_cap_scales_with_pressure() {
    // Earth-pressure caller (101 325 Pa) reproduces the
    // legacy 250 K cap exactly (the formula is anchored on
    // log10(P/P_earth) so the Earth case evaluates the
    // additive term to zero).
    let earth_cap = greenhouse_cap_scaled(Real::from_int(101_325));
    let earth_lo = Real::from_int(249);
    let earth_hi = Real::from_int(251);
    assert!(
        earth_cap >= earth_lo && earth_cap <= earth_hi,
        "Earth-pressure cap should be ~250 K; got {earth_cap:?}"
    );

    // Venus-pressure caller (9.2×10⁶ Pa) lifts the cap into
    // the ~440-450 K band. With `100 × log10(90.8) ≈ 195.8`
    // the formula evaluates to ~446 K; allow ±10 K so the
    // assertion survives Q32.32 rounding without becoming a
    // brittle exact-value check.
    let venus_cap = greenhouse_cap_scaled(Real::from_int(9_200_000));
    let venus_lo = Real::from_int(436);
    let venus_hi = Real::from_int(456);
    assert!(
        venus_cap >= venus_lo && venus_cap <= venus_hi,
        "Venus-pressure cap should be ~446 K; got {venus_cap:?}"
    );

    // Mars-pressure caller (~610 Pa) clamps at the 50 K
    // floor — the raw log10 evaluation would be negative
    // (28 K), but the floor keeps the cap usable for thin-
    // atmosphere worlds without collapsing it to zero.
    let mars_cap = greenhouse_cap_scaled(Real::from_int(610));
    assert!(
        mars_cap == Real::from_int(50),
        "Mars-pressure cap should clamp to the 50 K floor; got {mars_cap:?}"
    );

    // Defensive: a degenerate zero pressure should fall back
    // to the 50 K floor (the ln(0) panic guard inside
    // `greenhouse_cap_scaled`).
    let zero_cap = greenhouse_cap_scaled(Real::ZERO);
    assert!(
        zero_cap == Real::from_int(50),
        "Zero-pressure cap should fall back to the 50 K floor; got {zero_cap:?}"
    );
}
