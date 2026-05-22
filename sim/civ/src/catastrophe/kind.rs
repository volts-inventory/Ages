//! Catastrophe taxonomy. The five-kind enum and its stable
//! string tag for telemetry/event payloads. Kept in its own
//! module so external consumers (event sinks, drift code) pull
//! `CatastropheKind` without dragging in the orchestrator.

/// catastrophe taxonomy. Five kinds — `Volcanic` and
/// `Disease` are the M4-min lithosphere/biosphere triggers,
/// plus three later additions for story diversity:
/// `Asteroid` (rare-event impact), `SolarFlare` (high stellar
/// luminosity + weak magnetosphere → EM disruption), and
/// `IceAge` (sustained planet-mean temperature drop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatastropheKind {
    Volcanic,
    Disease,
    Asteroid,
    SolarFlare,
    IceAge,
}

impl CatastropheKind {
    pub fn tag(self) -> &'static str {
        match self {
            CatastropheKind::Volcanic => "volcanic",
            CatastropheKind::Disease => "disease",
            CatastropheKind::Asteroid => "asteroid",
            CatastropheKind::SolarFlare => "solar_flare",
            CatastropheKind::IceAge => "ice_age",
        }
    }
}
