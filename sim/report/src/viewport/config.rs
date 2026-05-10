//! `ViewportConfig` + `TempUnit`: user-facing knobs that govern
//! frame cadence, terminal control, and unit display.

#[allow(clippy::struct_excessive_bools)]
pub struct ViewportConfig {
    /// Render a frame every N ticks (months; 1 tick =
    /// 1 species-month). Smaller = smoother, larger = lower
    /// bandwidth. 12 = once per year, 1 = once per month, 50 = ~4
    /// years between frames.
    pub frame_every: u64,
    /// If true, wrap the session in alternate-screen mode and hide
    /// the cursor — the canonical interactive experience. Set false
    /// for tests or when piping to a file.
    pub use_alt_screen: bool,
    /// If true, emit 256-color ANSI escapes around per-civ
    /// identity symbols. Each civ id maps to a stable colour
    /// (24-hue palette, cycling) so adjacent civs are visually
    /// distinct. Disable when piping to a non-terminal sink that
    /// doesn't render ANSI codes.
    pub use_color: bool,
    /// Render a 2-line compact planet card above the
    /// caption — substrate, atmosphere, magnetosphere, moons,
    /// day length, axial tilt, year length, eccentricity. Static
    /// for the run; lets a viewer at a glance see "what kind of
    /// planet is this" without leaving the live display. Set
    /// `false` to skip (saves 3 lines on cramped terminals).
    pub show_planet_card: bool,
    /// Number of rows to reserve below the frame for a
    /// scrolling tail of significant events. The viewport tracks
    /// the most recent N "highlight" events (foundings, collapses,
    /// catastrophes, knowledge transmissions, conflicts, tech
    /// unlocks) and renders them as a scrolling log. `0` disables
    /// the log section entirely.
    pub log_lines: usize,
    /// Render each cell as a single character (no trailing
    /// space, no hex-row offset). Halves the on-screen grid
    /// width — a default 12-cell grid drops from 28 cols to 16.
    /// Useful on portrait phone terminals where the standard
    /// render wraps. Pure visual change; sim state and
    /// determinism unaffected.
    pub compact: bool,
    /// Temperature unit shown in the planet card. Default
    /// `Fahrenheit` because most readers find it more intuitive
    /// for surface-temperature comparisons; `Kelvin` is the
    /// internal sim unit and useful for physics-minded readers;
    /// `Celsius` is the third common choice.
    pub temperature_unit: TempUnit,
}

/// Viewport temperature unit. Affects the planet-card
/// rendering only — internal physics is always Kelvin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TempUnit {
    Kelvin,
    Celsius,
    Fahrenheit,
}

impl TempUnit {
    /// Convert a Kelvin value to this unit. Used by `planet_card`
    /// to format the displayed temperature.
    #[must_use]
    pub fn from_kelvin(self, k: f64) -> f64 {
        match self {
            Self::Kelvin => k,
            Self::Celsius => k - 273.15,
            Self::Fahrenheit => (k - 273.15) * 1.8 + 32.0,
        }
    }

    /// Suffix for the formatted temperature (`K`, `C`, `F`).
    #[must_use]
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Kelvin => "K",
            Self::Celsius => "C",
            Self::Fahrenheit => "F",
        }
    }
}

// Label functions live in `crate::labels`. The
// `host_species_status` / `planet_type` / `friendly_badge` /
// `substrate_biochem` / `atmosphere_descriptor` / `sociality_label`
// / `comm_label` / `short_modality` / `short_manip` / `cog_tier`
// helpers — and the `KNOWN_*` const arrays the metadata builder
// iterates — all live there. This module imports the ones it uses
// at the top of the file.

impl Default for ViewportConfig {
    fn default() -> Self {
        Self {
            frame_every: 50,
            use_alt_screen: true,
            use_color: true,
            show_planet_card: true,
            // 3 rows so the viewport fits in
            // a phone-with-keyboard-up portrait terminal (~25
            // visible rows) without scrolling the planet section
            // off the top.
            log_lines: 3,
            // Compact is the default. The standard hex-offset
            // layout was wider than portrait phone terminals can
            // hold; compact (1 char/cell, no offset) fits the
            // default 24×16 grid in ~27 cols.
            compact: true,
            temperature_unit: TempUnit::Fahrenheit,
        }
    }
}
