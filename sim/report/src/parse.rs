//! NDJSON event-log parsing. One `Event` per non-blank line; blank
//! lines are tolerated (some editors append them). Malformed lines
//! abort the parse with the line number so users can fix the log.

use protocol::Event;
use std::io::{BufRead, BufReader, Read};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("io error reading event log: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed event on line {line}: {source}")]
    Json {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
}

/// Parse every event in the NDJSON log. Returns the events in
/// file-order — the digest layer's two-pass walk depends on that
/// (`relation_id` → name map filled by the first pass before refinement
/// and transmission events look up against it).
pub fn events_from_reader<R: Read>(reader: R) -> Result<Vec<Event>, ParseError> {
    let buf = BufReader::new(reader);
    let mut events = Vec::new();
    for (idx, line) in buf.lines().enumerate() {
        let raw = line?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let ev: Event = serde_json::from_str(trimmed).map_err(|e| ParseError::Json {
            line: idx + 1,
            source: e,
        })?;
        events.push(ev);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{Phase, RunHeader, TickEvent, SCHEMA_VERSION};

    #[test]
    fn parses_two_events_round_trip() {
        let mut buf = String::new();
        let evs = vec![
            Event::RunStart(RunHeader {
                schema_version: SCHEMA_VERSION,
                seed: 7,
                ages_version: "test".into(),
            }),
            Event::Tick(TickEvent {
                tick: 0,
                phase: Phase::TickStart,
            }),
        ];
        for ev in &evs {
            buf.push_str(&serde_json::to_string(ev).unwrap());
            buf.push('\n');
        }
        // Blank line tolerance.
        buf.push('\n');
        let parsed = events_from_reader(buf.as_bytes()).unwrap();
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn surfaces_malformed_line_number() {
        let bad =
            "{\"kind\":\"run_start\",\"schema_version\":0,\"seed\":1,\"ages_version\":\"x\"}\n\
                   not-json\n";
        let err = events_from_reader(bad.as_bytes()).unwrap_err();
        match err {
            ParseError::Json { line, .. } => assert_eq!(line, 2),
            ParseError::Io(io) => panic!("expected Json error on line 2; got io {io:?}"),
        }
    }
}
