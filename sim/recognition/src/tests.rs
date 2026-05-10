use super::*;
use sim_physics::HexGrid;

fn fresh(width: u32, height: u32) -> PhysicsState {
    PhysicsState::new(HexGrid::new(width, height))
}

#[test]
fn fire_template_fires_when_hot_with_fuel_and_oxidiser() {
    // 700 K is above the 500 K fire threshold.
    let mut state = fresh(3, 3);
    state.temperature_mut()[0] = Real::from_int(700);
    state.substance_mut(Substance::Fuel.idx())[0] = Real::from_int(1);
    state.substance_mut(Substance::Oxidiser.idx())[0] = Real::from_int(1);
    let lib = RecognitionLibrary::earth_like_default();
    let firings = lib.scan(&state, 0, &PlanetContext::earth_like());
    let fire_firings: Vec<_> = firings.iter().filter(|f| f.template_id == 1).collect();
    assert_eq!(fire_firings.len(), 1);
    assert_eq!(fire_firings[0].cell, 0);
}

#[test]
fn fire_template_does_not_fire_without_fuel() {
    let mut state = fresh(3, 3);
    state.temperature_mut()[0] = Real::from_int(700);
    state.substance_mut(Substance::Oxidiser.idx())[0] = Real::from_int(1);
    // No fuel.
    let lib = RecognitionLibrary::earth_like_default();
    let firings = lib.scan(&state, 0, &PlanetContext::earth_like());
    let fire_firings: Vec<_> = firings.iter().filter(|f| f.template_id == 1).collect();
    assert!(fire_firings.is_empty());
}

#[test]
fn lightning_buildup_fires_on_negative_too() {
    let mut state = fresh(3, 3);
    state.charge_mut()[0] = Real::from_int(-60);
    let lib = RecognitionLibrary::earth_like_default();
    let firings = lib.scan(&state, 0, &PlanetContext::earth_like());
    let lightning: Vec<_> = firings.iter().filter(|f| f.template_id == 2).collect();
    assert_eq!(lightning.len(), 1);
}

#[test]
fn scan_is_deterministic() {
    let mut state = fresh(4, 4);
    state.temperature_mut()[3] = Real::from_int(700);
    state.substance_mut(Substance::Fuel.idx())[3] = Real::from_int(1);
    state.substance_mut(Substance::Oxidiser.idx())[3] = Real::from_int(1);
    state.charge_mut()[5] = Real::from_int(-60);

    let lib = RecognitionLibrary::earth_like_default();
    let a = lib.scan(&state, 0, &PlanetContext::earth_like());
    let b = lib.scan(&state, 0, &PlanetContext::earth_like());
    assert_eq!(a, b);
}

#[test]
fn earth_equivalent_seed_produces_sensible_firings() {
    // SI-validation: hand-build an Earth-equivalent
    // physics state and confirm the canonical templates light
    // up where a reasonable observer would expect them.
    let mut state = fresh(5, 5);

    // Cell 0: standing water (1.5 m column) → surface_water.
    state.water_depth_mut()[0] = Real::from_ratio(15, 10);

    // Cell 1: cold ice cap (above ice floor) → ice_present.
    state.substance_mut(Substance::Ice.idx())[1] = Real::from_ratio(5, 10);

    // Cell 2: humid air → vapour_present.
    state.substance_mut(Substance::Vapour.idx())[2] = Real::from_ratio(2, 10);

    // Cell 3: igniting wildfire (700 K, fuel + oxidiser) → fire.
    state.temperature_mut()[3] = Real::from_int(700);
    state.substance_mut(Substance::Fuel.idx())[3] = Real::from_int(1);
    state.substance_mut(Substance::Oxidiser.idx())[3] = Real::from_int(1);

    // Cell 4: pre-discharge thunderhead → lightning_buildup.
    state.charge_mut()[4] = Real::from_int(60);

    let lib = RecognitionLibrary::earth_like_default();
    let firings = lib.scan(&state, 0, &PlanetContext::earth_like());

    let by_id: std::collections::HashMap<u32, Vec<u32>> =
        firings
            .iter()
            .fold(std::collections::HashMap::new(), |mut acc, f| {
                acc.entry(f.template_id).or_default().push(f.cell);
                acc
            });

    // surface_water at cell 0
    assert!(by_id.get(&5).is_some_and(|v| v.contains(&0)));
    // ice_present at cell 1
    assert!(by_id.get(&3).is_some_and(|v| v.contains(&1)));
    // vapour_present at cell 2
    assert!(by_id.get(&4).is_some_and(|v| v.contains(&2)));
    // fire at cell 3
    assert!(by_id.get(&1).is_some_and(|v| v.contains(&3)));
    // lightning_buildup at cell 4
    assert!(by_id.get(&2).is_some_and(|v| v.contains(&4)));
}

#[test]
fn metallic_hydrogen_template_fires_when_hot_charged_and_vaporous() {
    let mut state = fresh(3, 3);
    for i in 0..9 {
        state.temperature_mut()[i] = Real::from_int(700);
        state.charge_mut()[i] = Real::from_int(35);
        state.substance_mut(Substance::Vapour.idx())[i] = Real::from_int(5);
    }
    let lib = RecognitionLibrary::earth_like_default();
    let firings = lib.scan(&state, 0, &PlanetContext::earth_like());
    let count_18 = firings.iter().filter(|f| f.template_id == 18).count();
    assert!(
        count_18 > 0,
        "expected template 18 to fire; got {firings:?}"
    );
}

#[test]
fn polar_winter_fires_in_north_during_january() {
    // Northern winter: month 0 (January). With every cell cold
    // enough, only northern-half cells should fire.
    let mut state = fresh(4, 4);
    for i in 0..16 {
        state.temperature_mut()[i] = Real::from_int(220);
    }
    let lib = RecognitionLibrary::earth_like_default();
    let firings = lib.scan(&state, 0, &PlanetContext::earth_like());
    let polar: Vec<u32> = firings
        .iter()
        .filter(|f| f.template_id == 26)
        .map(|f| f.cell)
        .collect();
    assert!(!polar.is_empty(), "expected polar_winter firings");
    // height = 4, half = 2. Northern cells have row < 2: ids
    // 0..7 (rows 0 and 1). Southern: ids 8..15.
    for cell in &polar {
        assert!(*cell < 8, "northern winter must not fire in south: {cell}");
    }
}

#[test]
fn polar_winter_fires_in_south_during_july() {
    // Month 6 (July) — southern winter, northern summer.
    let mut state = fresh(4, 4);
    for i in 0..16 {
        state.temperature_mut()[i] = Real::from_int(220);
    }
    let lib = RecognitionLibrary::earth_like_default();
    let firings = lib.scan(&state, 6, &PlanetContext::earth_like());
    let polar: Vec<u32> = firings
        .iter()
        .filter(|f| f.template_id == 26)
        .map(|f| f.cell)
        .collect();
    assert!(!polar.is_empty(), "expected polar_winter firings in south");
    for cell in &polar {
        assert!(*cell >= 8, "southern winter must not fire in north: {cell}");
    }
}

#[test]
fn polar_winter_silent_in_shoulder_seasons() {
    // March (month 2): neither hemisphere wintering, even with
    // a fully-cold planet polar_winter must not fire.
    let mut state = fresh(4, 4);
    for i in 0..16 {
        state.temperature_mut()[i] = Real::from_int(220);
    }
    let lib = RecognitionLibrary::earth_like_default();
    let firings = lib.scan(&state, 2, &PlanetContext::earth_like());
    let polar = firings.iter().filter(|f| f.template_id == 26).count();
    assert_eq!(polar, 0);
}
