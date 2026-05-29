//! `sim-report` — post-run markdown report generator (M6).
//!
//! Reads an NDJSON event log produced by `ages` and renders a
//! structured markdown report. Pipeline:
//!
//!   1. `parse::events_from_reader` — line-by-line NDJSON decode into
//!      `protocol::Event` values (skips blank lines; surfaces malformed
//!      ones as errors).
//!   2. `digest::Digest::from_events` — single fold across the event
//!      stream that aggregates every section the renderer needs:
//!      planet card, species card, civ-by-civ chapters, transmissions,
//!      contacts, conflicts, diffusions, run-end. Two-pass for the
//!      `relation_id → (template, channel)` map so refinement and
//!      transmission events render with names.
//!   3. `render::markdown` — turns the digest into a markdown string.
//!      Templated prose where prose helps readability; no LLM.
//!
//! The "interesting moments" highlight reel uses the structural-pin
//! plus scored-long-tail hybrid: founding / collapse / catastrophe /
//! tech-tier crossings / first-of-kind discoveries always make the cut;
//! everything else is scored by novelty + magnitude + figure-significance
//! + arc-coherence and the top N pulled in.

// Display-only crate: numeric values come from the sim's
// deterministic Q32.32 encoding and are only ever rendered for
// humans. Lossy `usize`/`u64`/`i64` → `f64` casts are intentional;
// the renderer does not feed values back into any sim path.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::module_name_repetitions,
    clippy::too_many_lines
)]

mod ages;
mod digest;
pub mod frame;
mod highlights;
mod html;
pub mod labels;
pub mod narration;
mod parse;
mod q32;
mod render;
pub mod tui;
pub mod viewport;

pub use digest::{CivChapter, Digest};
pub use frame::{
    render_density_frame, render_world_frame, render_world_frame_styled, CivClaim, FrameStyle,
    WorldFrame,
};
pub use html::html;
pub use narration::{
    narrate_event, replay_narration, replay_narration_from_reader, NarrateError, NarratingEmitter,
    NarratorState,
};
pub use parse::{events_from_reader, ParseError};
pub use render::markdown;
pub use tui::{run_interactive_tui, TuiOptions};
pub use viewport::{CivPanel, TempUnit, ViewportConfig, ViewportEmitter};

use std::io::Read;

/// Top-level convenience: NDJSON in → markdown out. Equivalent to
/// `parse::events_from_reader` → `Digest::from_events` → `render::markdown`.
pub fn render_from_reader<R: Read>(reader: R) -> Result<String, ParseError> {
    let events = events_from_reader(reader)?;
    let digest = Digest::from_events(&events);
    Ok(markdown(&digest))
}

/// HTML variant of `render_from_reader`. Wraps the markdown in a
/// minimal HTML shell with a stylesheet tuned for the ASCII map +
/// sparkline blocks. Same digest, same content — just a second
/// surface for downstream consumers (browsers, static-site
/// generators).
pub fn render_html_from_reader<R: Read>(reader: R) -> Result<String, ParseError> {
    let events = events_from_reader(reader)?;
    let digest = Digest::from_events(&events);
    Ok(html(&digest))
}
