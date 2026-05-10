//! Species "Ages" — emergent eras derived from the event log.
//! The project is named *Ages*; this module turns that into a
//! first-class report section. Each age starts when its defining
//! milestone first fires and ends when the next age begins (or at
//! run-end). On a short run (single-civ `species_extinction`) only
//! the first age or two will fire — that's accurate biography:
//! the species never reached the later eras.
//!
//! Ages are derived, not authored. The triggers are concrete events
//! with no overlap:
//!
//! - **Foundational** — start of run; species exists, no science yet.
//! - **Empirical** — first `RelationConfirmed`.
//! - **Refinement** — first `RefinementConfirmed`.
//! - **Tool** — first `TechUnlocked`.
//! - **Concurrent** — first `CivContact` (two civs co-exist).
//! - **Successor** — first `CivCollapsed` while another civ is alive
//!   (knowledge effectively survived the first extinction event).

use crate::digest::Digest;
use protocol::Event;
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct AgeRecord {
    pub name: &'static str,
    pub started_year: u64,
    /// `None` means the age was the last one and ran to run-end.
    pub ended_year: Option<u64>,
    pub trigger_text: String,
}

/// Threshold for the Industrial Age — sustained tech adoption
/// across multiple civs. Picked at the count where ~half the
/// civ chain is running mature toolsets. Tunable.
const INDUSTRIAL_TECH_THRESHOLD: u32 = 5;

/// Walk the event log in tick order and emit one `AgeRecord` per
/// triggered milestone, in the order they fired. The Foundational
/// Age always lands (it covers year 0 onwards until the first
/// scientific event); subsequent ages only land if their trigger
/// fired within the run.
#[must_use]
pub fn derive_ages(digest: &Digest) -> Vec<AgeRecord> {
    // Planet-relative year accounting. Each AgeRecord's
    // `started_year` is in the planet's own years (orbital_period_months).
    let period = digest
        .planet
        .as_ref()
        .map_or(protocol::BASELINE_MONTHS_PER_YEAR as u32, |p| {
            p.orbital_period_months
        });
    let year_of = |t: u64| -> u64 { protocol::year_of_tick_for_period(t, period) };
    let mut ages: Vec<AgeRecord> = Vec::new();
    ages.push(AgeRecord {
        name: "Foundational Age",
        started_year: 0,
        ended_year: None,
        trigger_text: "the species' first generation; no relations confirmed yet".to_string(),
    });

    let mut have_empirical = false;
    let mut have_refinement = false;
    let mut have_tool = false;
    let mut have_concurrent = false;
    let mut have_successor = false;
    let mut have_industrial = false;
    let mut have_transcendence = false;
    let mut tech_unlock_count: u32 = 0;

    // Track active civs as we walk so the Successor Age trigger can
    // tell "first collapse happened while another civ was alive."
    let mut active_civ_ids: BTreeSet<u32> = BTreeSet::new();

    for ev in &digest.events {
        match ev {
            Event::CivFounded(c) => {
                active_civ_ids.insert(c.civ_id);
            }
            Event::CivCollapsed(c) => {
                active_civ_ids.remove(&c.civ_id);
                if !have_successor && !active_civ_ids.is_empty() {
                    have_successor = true;
                    push_age(
                        &mut ages,
                        year_of(c.tick),
                        AgeRecord {
                            name: "Successor Age",
                            started_year: year_of(c.tick),
                            ended_year: None,
                            trigger_text: format!(
                                "civ {} collapsed but the species lived on through other civs — knowledge survived its first extinction event",
                                c.civ_id
                            ),
                        },
                    );
                }
            }
            Event::RelationConfirmed(r) if !have_empirical => {
                have_empirical = true;
                push_age(
                    &mut ages,
                    year_of(r.tick),
                    AgeRecord {
                        name: "Empirical Age",
                        started_year: year_of(r.tick),
                        ended_year: None,
                        trigger_text: format!(
                            "the species' first confirmed relation: `{}` \u{2194} `{}` ({})",
                            r.template_name, r.channel, r.form
                        ),
                    },
                );
            }
            Event::RefinementConfirmed(r) if !have_refinement => {
                have_refinement = true;
                push_age(
                    &mut ages,
                    year_of(r.tick),
                    AgeRecord {
                        name: "Refinement Age",
                        started_year: year_of(r.tick),
                        ended_year: None,
                        trigger_text: format!(
                            "the species began revising its theories: relation {} re-fitted from {} to {}",
                            r.relation_id, r.old_form, r.new_form
                        ),
                    },
                );
            }
            Event::TechUnlocked(t) => {
                tech_unlock_count = tech_unlock_count.saturating_add(1);
                if !have_tool {
                    have_tool = true;
                    push_age(
                        &mut ages,
                        year_of(t.tick),
                        AgeRecord {
                            name: "Tool Age",
                            started_year: year_of(t.tick),
                            ended_year: None,
                            trigger_text: format!(
                                "first sensorium-extending tool: civ {}'s `{}` (tier {})",
                                t.civ_id, t.tool_name, t.tier
                            ),
                        },
                    );
                } else if !have_industrial && tech_unlock_count >= INDUSTRIAL_TECH_THRESHOLD {
                    have_industrial = true;
                    push_age(
                        &mut ages,
                        year_of(t.tick),
                        AgeRecord {
                            name: "Industrial Age",
                            started_year: year_of(t.tick),
                            ended_year: None,
                            trigger_text: format!(
                                "{tech_unlock_count} tools unlocked across the species — sustained engineering tradition rather than one-off invention"
                            ),
                        },
                    );
                }
                // Transcendence Age fires on the first tier-5
                // tool. Tier 5 in `sim_civ::tech` is reserved for
                // late-game capabilities — bioelectric resonator,
                // field propulsion engine, metamaterial lattice —
                // each of which gates on a substrate-aligned set
                // of crust + magnetic + observational prereqs. The
                // age caption names which one fired first so the
                // biography records the species' summit ascent path.
                if !have_transcendence && t.tier >= 5 {
                    have_transcendence = true;
                    push_age(
                        &mut ages,
                        year_of(t.tick),
                        AgeRecord {
                            name: "Transcendence Age",
                            started_year: year_of(t.tick),
                            ended_year: None,
                            trigger_text: format!(
                                "civ {} unlocked the first tier-5 capability: `{}` — the species reached its tech-tree summit",
                                t.civ_id, t.tool_name
                            ),
                        },
                    );
                }
            }
            Event::CivContact(c) if !have_concurrent => {
                have_concurrent = true;
                push_age(
                    &mut ages,
                    c.tick,
                    AgeRecord {
                        name: "Concurrent Age",
                        started_year: year_of(c.tick),
                        ended_year: None,
                        trigger_text: format!(
                            "civs {} and {} first co-existed — multiple traditions running in parallel",
                            c.civ_a, c.civ_b
                        ),
                    },
                );
            }
            _ => {}
        }
    }

    // Close the final age at run-end (or last-known tick).
    if let Some(last) = ages.last_mut() {
        if last.ended_year.is_none() {
            if let Some(end) = &digest.run_end {
                last.ended_year = None; // open-ended at run-end
                let _ = end;
            }
        }
    }
    ages
}

/// Insert a new age, closing the previous open one at the new
/// age's start.
fn push_age(ages: &mut Vec<AgeRecord>, started_year: u64, new_age: AgeRecord) {
    if let Some(prev) = ages.last_mut() {
        if prev.ended_year.is_none() {
            prev.ended_year = Some(started_year);
        }
    }
    ages.push(new_age);
}
