//! Live ASCII viewport: an `Emitter` wrapper that mirrors per-civ
//! territorial state from the event stream and re-renders the
//! world to a `Write` (typically stdout) every `frame_every`
//! ticks. Same `frame::render_world_frame` core as the post-run
//! report's spatial-timeline keyframes — the live frame and the
//! post-run frame are visually identical.
//!
//! State mirroring rules (the only events the viewport reads):
//! - `Planet` / `PlanetMap` — captured for the terrain backdrop.
//! - `CivFounded` — registers the civ with its founding claim.
//! - `CivTerritoryChanged` — replaces the civ's claim set.
//! - `CivCollapsed` — drops the civ from the active set.
//! - `Tick { phase: TickEnd }` — frame-cadence trigger.
//! - `RunEnd` — restore terminal + final frame.
//!
//! Every other event is forwarded verbatim. The viewport never
//! mutates events; it's a pure observer that happens to also
//! write a refreshing frame to stdout.

mod ansi;
mod config;
mod emitter;

pub use config::{TempUnit, ViewportConfig};
pub use emitter::ViewportEmitter;

#[cfg(test)]
mod tests;
