//! Severity / cooldown scaling factors driven by planet properties:
//! biosphere richness drives disease severity, crust drives volcanic
//! cooldown speed, and the planet's mean temperature drives ice-age
//! severity.

use sim_arith::Real;
use sim_world::{BiosphereClass, Crust};

/// More biosphere = more pathogens. None-biosphere worlds barely
/// register diseases; `HyperBiodiverse` worlds suffer hard.
#[must_use]
pub fn disease_severity_factor(biosphere: BiosphereClass) -> Real {
    match biosphere {
        BiosphereClass::None => Real::percent(20),
        BiosphereClass::Sparse => Real::percent(60),
        BiosphereClass::Lush => Real::ONE,
        BiosphereClass::HyperBiodiverse => Real::percent(150),
    }
}

/// volcanic cooldown multiplier from crust composition.
/// Basaltic crust (Earth-like) is the baseline; Hydrocarbon
/// crust correlates with high mantle activity (shorter cooldown);
/// older Piezoelectric/RareEarth crusts are stable (longer
/// cooldown).
#[must_use]
pub fn volcanic_cooldown_factor(crust: Crust) -> Real {
    match crust {
        Crust::Basaltic => Real::ONE,
        Crust::Hydrocarbon => Real::percent(80),
        Crust::Piezoelectric => Real::percent(140),
        Crust::Ferrous => Real::percent(110),
        Crust::RareEarth => Real::percent(150),
    }
}

/// ice-age severity scales with how far below 273 K the
/// planet's mean temperature sits. A planet at 273 K or above
/// gets the baseline pop-loss; a planet at 253 K (-20 °C) gets
/// 1.0× (no change since the deviation = 20 / 20 = 1.0); a planet
/// at 243 K (-30 °C) gets 1.5×; below that, scales further.
#[must_use]
pub fn ice_age_severity_factor(mean_temperature_k: Real) -> Real {
    let freeze = Real::from_int(273);
    if mean_temperature_k >= freeze {
        return Real::ONE;
    }
    let deviation = freeze - mean_temperature_k;
    let scale = Real::from_int(20);
    let factor = deviation / scale;
    if factor < Real::ONE {
        Real::ONE
    } else {
        factor
    }
}
