#!/usr/bin/env python3
"""narrate.py — tell the story of an ages run from its NDJSON event log.

Usage:
    ./narrate.py runs/2026-05-05-1430-1234567890.ndjson

Reads the run's NDJSON event log, extracts the high-impact narrative
events (foundings, tech unlocks, transmissions, conflicts,
catastrophes, collapses, cosmology shifts), and prints a templated
prose summary to stdout. Pure-stdlib Python 3; no external deps.

Templated, not LLM-driven. The narrator's job is "describe what
happened" — the sim already produced the story; this script just
reads it back as English.
"""

import json
import sys
from collections import defaultdict


# Q32.32 fixed-point: raw i64 / 2^32 = float.
Q32_DIVISOR = 1 << 32

MONTHS_PER_YEAR = 12


# ── Label tables read from the NDJSON `run_metadata` event.
# Source-of-truth lives in `sim/report/src/labels.rs`; the sim
# emits the populated tables once at run start so we don't
# duplicate them here. The `Metadata` class below is populated
# from the event when present; if the event is missing (older
# logs that pre-date the metadata event) we fall back to a small
# set of last-known-good defaults so old runs still narrate.

DEFAULT_PLANET_TYPE = {
    "aqueous": "ocean world",
    "ammoniacal": "ammonia world",
    "hydrocarbon": "methane world",
    "silicate": "lava world",
}

DEFAULT_BIOCHEM = {
    "silicate": "silicon",
    "aqueous": "carbon",
    "ammoniacal": "carbon",
    "hydrocarbon": "carbon",
}

DEFAULT_ATMOSPHERE = {
    "none": "vacuum",
    "thin": "thin",
    "oxidising": "oxygen-rich",
    "reducing": "methane-rich",
    "hazy": "hazy",
}

DEFAULT_FRIENDLY_BADGE = {
    "frozen-out": "frozen",
    "near-freezing": "cold",
    "thriving": "habitable",
    "near-boiling": "hot",
    "boiling-off": "scorching",
    "vacuum": "vacuum",
}

DEFAULT_SUBSTRATE_FREEZE_K = {
    "aqueous": 273.15,
    "ammoniacal": 195.4,
    "hydrocarbon": 90.7,
    "silicate": 1687.0,
}

DEFAULT_SUBSTRATE_BOIL_K = {
    "aqueous": 373.15,
    "ammoniacal": 239.8,
    "hydrocarbon": 111.7,
    "silicate": 3538.0,
}

DEFAULT_TIER_THRESHOLDS = [0.34, 0.67]
DEFAULT_COG_TIER = ["low", "medium", "high"]
DEFAULT_SOCIALITY_TIER = ["solitary", "social", "eusocial"]
DEFAULT_COMM_TIER = ["noisy", "clear", "precise"]

# Narrator-specific prose labels for sensory modalities and
# manipulation modes. These differ deliberately from the
# viewport's short labels (which target a 40-col grid card and
# read as adjectives/verbs like `tactile`, `secrete`); the
# narrator wants noun-phrase forms that read as English prose
# ("touch", "secreted chemicals"). Prose forms are *not*
# duplicates of the viewport tables — different audiences,
# different vocabularies.
PROSE_MOD = {
    "AcousticAir": "airborne sound",
    "AcousticWater": "underwater sound",
    "Seismic": "ground vibration",
    "VisualLight": "vision",
    "VisualPolarization": "polarized light",
    "Bioluminescent": "bioluminescence",
    "ChemicalPheromone": "pheromones",
    "ChemicalTaste": "taste / smell",
    "Tactile": "touch",
    "ElectricField": "electric fields",
    "MagneticSense": "magnetic sense",
    "InfraredThermal": "infrared / thermal",
    "RadioNative": "native radio",
    "Gestural": "gesture",
    "Postural": "posture",
}

PROSE_MANIP = {
    "LimbGrasp": "grasping limbs",
    "Tentacle": "tentacles",
    "MouthBeak": "beaks",
    "TonguePrehensile": "prehensile tongues",
    "Trunk": "trunks",
    "Mandible": "mandibles",
    "FluidJet": "fluid jets",
    "ToolExtension": "tool-using limbs",
    "WebConstruct": "constructed webs",
    "Burrow": "burrows",
    "ElectricDischarge": "electric discharge",
    "ChemicalSecretion": "secreted chemicals",
}


class Metadata:
    """Bag of label tables + thresholds, populated from the
    `run_metadata` NDJSON event when present, otherwise from the
    in-script defaults so old logs that pre-date the metadata
    event still narrate."""

    def __init__(self):
        self.substrate_freeze_k = dict(DEFAULT_SUBSTRATE_FREEZE_K)
        self.substrate_boil_k = dict(DEFAULT_SUBSTRATE_BOIL_K)
        self.planet_type_labels = dict(DEFAULT_PLANET_TYPE)
        self.planet_biochem_labels = dict(DEFAULT_BIOCHEM)
        self.atmosphere_labels = dict(DEFAULT_ATMOSPHERE)
        self.friendly_badge_labels = dict(DEFAULT_FRIENDLY_BADGE)
        self.modality_short_labels = {}
        self.manipulation_short_labels = {}
        self.tier_thresholds = list(DEFAULT_TIER_THRESHOLDS)
        self.cog_tier_labels = list(DEFAULT_COG_TIER)
        self.sociality_tier_labels = list(DEFAULT_SOCIALITY_TIER)
        self.comm_tier_labels = list(DEFAULT_COMM_TIER)

    def load_from_event(self, ev):
        # Each field is optional — if the event is malformed or
        # truncated, we keep the default for any missing key.
        for key in (
            "substrate_freeze_k", "substrate_boil_k",
            "planet_type_labels", "planet_biochem_labels",
            "atmosphere_labels", "friendly_badge_labels",
            "modality_short_labels", "manipulation_short_labels",
            "tier_thresholds", "cog_tier_labels",
            "sociality_tier_labels", "comm_tier_labels",
        ):
            if key in ev and ev[key]:
                setattr(self, key, ev[key])

    def tier(self, value: float, labels):
        t0, t1 = self.tier_thresholds[0], self.tier_thresholds[1]
        if value < t0:
            return labels[0]
        if value < t1:
            return labels[1]
        return labels[2]

    def cog(self, value: float) -> str:
        return self.tier(value, self.cog_tier_labels)

    def soc(self, value: float) -> str:
        return self.tier(value, self.sociality_tier_labels)

    def comm(self, value: float) -> str:
        return self.tier(value, self.comm_tier_labels)


def host_badge_phrase(meta: Metadata, substrate: str, mean_t_k: float, atmosphere: str) -> str:
    """Friendly host-species habitability sentence built from the
    internal badge name plus a friendly descriptor word."""
    if substrate not in meta.substrate_freeze_k:
        return "of unknown habitability"
    if atmosphere == "none" and substrate != "silicate":
        return "vacuum-bound — the atmosphere is gone"
    freeze = meta.substrate_freeze_k[substrate]
    boil = meta.substrate_boil_k[substrate]
    span = boil - freeze
    pos = (mean_t_k - freeze) / span if span > 0 else 0.5
    badge_phrases = {
        "frozen": "frozen — the world has chilled past its inhabitants' tolerance",
        "cold": "cold — life clings to the warmer fringes",
        "habitable": "habitable — comfortably mid-range for its substrate",
        "hot": "hot — life endures the upper edge of its substrate's window",
        "scorching": "scorching — the world has heated past its inhabitants' tolerance",
        "vacuum": "vacuum-bound — the atmosphere is gone",
    }
    if pos < 0.0:
        word = meta.friendly_badge_labels.get("frozen-out", "frozen")
    elif pos < 0.25:
        word = meta.friendly_badge_labels.get("near-freezing", "cold")
    elif pos > 1.0:
        word = meta.friendly_badge_labels.get("boiling-off", "scorching")
    elif pos > 0.75:
        word = meta.friendly_badge_labels.get("near-boiling", "hot")
    else:
        word = meta.friendly_badge_labels.get("thriving", "habitable")
    return badge_phrases.get(word, f"{word} — habitability {pos:.2f} of substrate window")


# ── Event reading ──

def q32(v):
    return v / Q32_DIVISOR


def k_to_f(k):
    return (k - 273.15) * 1.8 + 32.0


def year_of(tick):
    return tick // MONTHS_PER_YEAR


def read_events(path):
    with open(path, "r") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                yield json.loads(line)
            except json.JSONDecodeError:
                continue


# ── Narrative builders ──

def opening(meta: Metadata, planet, species, run_start, run_end):
    name = planet["name"]
    seed = planet["seed"]
    substrate = planet["metabolic_substrate"]
    atmosphere = planet["atmosphere"]
    mean_t_k = q32(planet["mean_temperature_q32"])
    mean_t_f = k_to_f(mean_t_k)
    day_h = q32(planet["day_length_hours_q32"])
    tilt = q32(planet["axial_tilt_deg_q32"])
    year_mo = planet["orbital_period_months"]
    moons = planet["moon_count"]
    mag = planet["magnetosphere"]
    biosphere = planet.get("biosphere", "biosphere")

    final_year = year_of(run_end["tick"]) if run_end else None

    title = f"The Story of {name}"
    out = []
    out.append("═" * 60)
    out.append(title.center(60))
    seed_line = f"seed {seed}"
    if final_year is not None:
        seed_line += f"  ·  {final_year} simulated years"
    out.append(seed_line.center(60))
    out.append("═" * 60)
    out.append("")

    # WORLD — every label sourced from the run's metadata event.
    ptype = meta.planet_type_labels.get(substrate, "world")
    atm_desc = meta.atmosphere_labels.get(atmosphere, atmosphere)
    badge = host_badge_phrase(meta, substrate, mean_t_k, atmosphere)
    mag_desc = {
        "none": "no protective magnetosphere",
        "weak": "a weak magnetosphere",
        "strong": "a strong magnetosphere",
    }.get(mag, f"a {mag} magnetosphere")
    moon_phrase = f"{moons} moon" + ("s" if moons != 1 else "")

    out.append("THE WORLD")
    out.append("─────────")
    out.append(
        f"{name} is an {ptype}. The atmosphere is {atm_desc}; mean surface "
        f"temperature {mean_t_f:.0f}°F. The planet has {mag_desc} and "
        f"{moon_phrase}. Days last {day_h:.0f} hours; the year runs {year_mo} "
        f"months at a {tilt:.0f}° axial tilt. The biosphere is "
        f"{biosphere.replace('_', ' ')}."
    )
    out.append("")
    out.append(f"It is {badge}.")
    out.append("")

    if species is None:
        out.append("(No species event in the log — narrative ends here.)")
        return "\n".join(out)

    # INHABITANTS
    sp_name = species["name"]
    cog = q32(species["cognition_q32"])
    soc = q32(species["sociality_q32"])
    comm = q32(species["communication_fidelity_q32"])
    lifespan = int(q32(species["lifespan_years_q32"]))
    topo = species.get("cognition_topology", "centralized")
    primary_mod_raw = species["modalities"][0] if species.get("modalities") else "?"
    primary_manip_raw = (
        species["manipulation_modes"][0] if species.get("manipulation_modes") else "?"
    )
    primary_mod = PROSE_MOD.get(primary_mod_raw, primary_mod_raw.lower())
    primary_manip = PROSE_MANIP.get(primary_manip_raw, primary_manip_raw.lower())
    biochem = meta.planet_biochem_labels.get(substrate, "carbon")

    out.append("THE INHABITANTS")
    out.append("───────────────")
    out.append(
        f"The {sp_name} are {meta.soc(soc)} {biochem}-based life — {topo} "
        f"cognition at the {meta.cog(cog)} tier. They sense their world "
        f"through {primary_mod}, and manipulate it with {primary_manip}. "
        f"Lifespans average {lifespan} years; their communication is "
        f"{meta.comm(comm)}."
    )
    if len(species.get("modalities", [])) > 1:
        secondaries = [PROSE_MOD.get(m, m.lower()) for m in species["modalities"][1:5]]
        out.append(f"Secondary senses: {', '.join(secondaries)}.")
    out.append("")
    return "\n".join(out)


def narrate_civ_arcs(events_by_kind, planet_map_grid_width):
    """Return a list of (tick, paragraph) tuples for chronological merge."""
    paragraphs = []

    def cell_to_loc(cell_id):
        if planet_map_grid_width is None or planet_map_grid_width == 0:
            return None
        row = cell_id // planet_map_grid_width
        col = cell_id % planet_map_grid_width
        return f"row {row}, col {col}"

    for ev in events_by_kind.get("civ_founded", []):
        tick = ev.get("tick", 0)
        civ_id = ev["civ_id"]
        figs = ev.get("founding_figure_count", 0)
        cells = len(ev.get("claimed_cells", []))
        parent = ev.get("parent_civ_id")
        first_cell = ev.get("claimed_cells", [None])[0]
        loc = cell_to_loc(first_cell) if first_cell is not None else None
        loc_phrase = f" at {loc}" if loc else ""
        # civ_founded events carry a deterministic kingdom-style
        # name. Older NDJSON files that pre-date the name field
        # don't — fall back to "Civilization {id}" for those.
        name = ev.get("name") or ""
        subject = name if name else f"Civilization {civ_id}"
        parent_label = f"civ {parent}"
        if parent is None:
            paragraphs.append(
                (tick, f"{subject} took root{loc_phrase}, founded by "
                       f"{figs} figure{'s' if figs != 1 else ''} across "
                       f"{cells} cell{'s' if cells != 1 else ''}.")
            )
        else:
            paragraphs.append(
                (tick, f"{subject} branched off from {parent_label}{loc_phrase}, "
                       f"with {figs} figure{'s' if figs != 1 else ''} claiming "
                       f"{cells} cell{'s' if cells != 1 else ''}.")
            )

    for ev in events_by_kind.get("tech_unlocked", []):
        tick = ev.get("tick", 0)
        civ_id = ev["civ_id"]
        tool = ev.get("tool_name", "an unnamed tool")
        paragraphs.append(
            (tick, f"Civ {civ_id} unlocked {tool.replace('_', ' ')}.")
        )

    for ev in events_by_kind.get("knowledge_transmitted", []):
        tick = ev.get("tick", 0)
        src = ev.get("source_civ_id")
        dst = ev.get("dest_civ_id")
        rel = ev.get("relation_id", "?")
        paragraphs.append(
            (tick, f"Civ {src} transmitted relation {rel} to civ {dst} — "
                   f"a knowledge exchange across the divide.")
        )

    for ev in events_by_kind.get("civ_contact", []):
        tick = ev.get("tick", 0)
        a = ev.get("civ_a_id", "?")
        b = ev.get("civ_b_id", "?")
        paragraphs.append(
            (tick, f"Civs {a} and {b} made first contact.")
        )

    for ev in events_by_kind.get("conflict_resolved", []):
        tick = ev.get("tick", 0)
        winner = ev.get("winner_civ_id", "?")
        loser = ev.get("loser_civ_id", "?")
        defeated = ev.get("loser_defeated", False)
        if defeated:
            paragraphs.append(
                (tick, f"War: civ {winner} defeated civ {loser}, taking disputed cells.")
            )
        else:
            paragraphs.append(
                (tick, f"A border dispute between civs {winner} and {loser} was resolved.")
            )

    for ev in events_by_kind.get("catastrophe_fired", []):
        tick = ev.get("tick", 0)
        civ_id = ev.get("civ_id", "?")
        kind = ev.get("catastrophe_kind", "an unnamed catastrophe")
        frac = q32(ev.get("fraction_lost_q32", 0))
        paragraphs.append(
            (tick, f"Catastrophe — a {kind} struck civ {civ_id}, "
                   f"claiming {frac * 100:.0f}% of its population.")
        )

    for ev in events_by_kind.get("cosmology_shifted", []):
        tick = ev.get("tick", 0)
        civ_id = ev.get("civ_id", "?")
        paragraphs.append(
            (tick, f"Civ {civ_id}'s cosmology shifted — a paradigm change in "
                   f"how they understand their world.")
        )

    for ev in events_by_kind.get("civ_collapsed", []):
        tick = ev.get("tick", 0)
        civ_id = ev["civ_id"]
        reason = ev.get("reason", "unknown reasons")
        paragraphs.append(
            (tick, f"Civilization {civ_id} collapsed — {reason.replace('_', ' ')}.")
        )

    paragraphs.sort(key=lambda x: x[0])
    return paragraphs


def render_arcs(paragraphs):
    if not paragraphs:
        return "Nothing of narrative consequence happened. The world simply turned.\n"
    out = []
    out.append("MAJOR ARCS")
    out.append("──────────")
    out.append("")
    last_year = None
    for tick, para in paragraphs:
        year = year_of(tick)
        month = tick % MONTHS_PER_YEAR
        if year != last_year:
            if last_year is not None:
                out.append("")
            out.append(f"⌚ Year {year}")
            last_year = year
        out.append(f"   M{month:>2} — {para}")
    out.append("")
    return "\n".join(out)


def closing(events_by_kind, run_end):
    out = []
    out.append("THE ENDING")
    out.append("──────────")

    founded = len(events_by_kind.get("civ_founded", []))
    collapsed = len(events_by_kind.get("civ_collapsed", []))
    surviving = founded - collapsed

    tech = len(events_by_kind.get("tech_unlocked", []))
    transmissions = len(events_by_kind.get("knowledge_transmitted", []))
    conflicts = len(events_by_kind.get("conflict_resolved", []))
    catastrophes = len(events_by_kind.get("catastrophe_fired", []))
    relations = len(events_by_kind.get("relation_confirmed", []))

    final_year = year_of(run_end["tick"]) if run_end else "?"

    out.append(
        f"After {final_year} simulated years, {founded} civilization{'s' if founded != 1 else ''} "
        f"had ever risen on this world. {collapsed} of them collapsed; "
        f"{max(surviving, 0)} remained at sim-end."
    )
    out.append("")
    out.append("Across that history they collectively:")
    out.append(f"  · unlocked {tech} tools")
    out.append(f"  · confirmed {relations} natural relations")
    out.append(f"  · transmitted knowledge {transmissions} times across civilizations")
    out.append(f"  · resolved {conflicts} conflicts")
    out.append(f"  · weathered {catastrophes} catastrophes")
    out.append("")
    return "\n".join(out)


# ── Main ──

def main(argv):
    if len(argv) != 2:
        print("usage: narrate.py <events.ndjson>", file=sys.stderr)
        sys.exit(2)

    path = argv[1]

    planet = None
    planet_map = None
    species = None
    run_start = None
    run_end = None
    metadata = Metadata()
    events_by_kind = defaultdict(list)

    for ev in read_events(path):
        kind = ev.get("kind")
        if kind == "planet":
            planet = ev
        elif kind == "planet_map":
            planet_map = ev
        elif kind == "species":
            species = ev
        elif kind == "run_start":
            run_start = ev
        elif kind == "run_end":
            run_end = ev
        elif kind == "run_metadata":
            # presentation metadata — substrate ranges + label
            # tables. Populates the Metadata bag so the rest of
            # the script reads everything from one place.
            metadata.load_from_event(ev)
        elif kind in {
            "civ_founded", "civ_collapsed", "civ_contact",
            "tech_unlocked", "knowledge_transmitted",
            "conflict_resolved", "catastrophe_fired",
            "cosmology_shifted", "relation_confirmed",
        }:
            events_by_kind[kind].append(ev)

    if planet is None:
        print(f"No `planet` event in {path}; nothing to narrate.", file=sys.stderr)
        sys.exit(1)

    grid_width = planet_map["grid_width"] if planet_map else None
    print(opening(metadata, planet, species, run_start, run_end))
    print(render_arcs(narrate_civ_arcs(events_by_kind, grid_width)))
    print(closing(events_by_kind, run_end))


if __name__ == "__main__":
    main(sys.argv)
