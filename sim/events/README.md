# sim/events

NDJSON event emission. **Schema is the contract** (see `protocol/`);
this crate is the writer.

## Status

- **M0+ shipped**: `Emitter` trait, `JsonLinesEmitter`, live CLI tee
  via stdout fan-out, `FilterEmitter` (powers `--cli=highlights`),
  `ThrottledEmitter` (powers `--tick-rate-ms`).

## Design

- One emitter per run. Trait method `emit(&Event) -> Result<(), Err>`.
- `JsonLinesEmitter` writes one JSON object per line to any
  `Write` sink. Used for both the canonical event log file and the
  live CLI stream (the run binary opens both and tees).
- `TeeEmitter<A, B>` fans one event to two emitters. The `ages`
  binary uses it to write the canonical NDJSON file AND stream to
  stdout in the same iteration.
- `FilterEmitter<E, F>` forwards events that match a predicate; the
  `ages` binary wraps the stdout side with `is_highlight_event`
  for `--cli=highlights` mode.
- `ThrottledEmitter<E>` sleeps `tick_rate` between ticks (only on
  the `Tick { phase: TickEnd }` event so the sleep is once per tick
  at the canonical boundary, not per-event). Pacing the live stream
  helps human readability and any future UI consumer; the file
  emitter is never throttled.
- Determinism contract: emission order = iteration order in the
  caller. Emitters never reorder, batch, or sort. `ThrottledEmitter`
  reads wall-clock time via `thread::sleep` but doesn't feed it
  back into any sim computation — the upstream event log is
  bit-for-bit identical regardless of throttle setting.

## Cited by

[docs/architecture.md](../../docs/architecture.md) (Live CLI event
stream, per-tick event ordering).
