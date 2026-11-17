#!/usr/bin/env python3
"""Draft crates/sc-core/data/options_layout.toml for a game build.

    python3 -I tools/options_layout_draft.py --exe StarCitizen.exe \
        --attributes attributes.xml --strings global.ini \
        --keep crates/sc-core/data/options_layout.toml > draft.toml

The game's Options menu is compiled into the exe. Its label keys sit there as
`@pause_Options*` / `@pause_options_*` strings, roughly in the order of the
default attributes.xml. Each attribute is paired with a label key by

  (a) the name `pause_Options<Name>`,
  (b) a normalised name match against the exe list,
  (c) the exe list position, aligned to the attribute order between two
      pairings of (a) or (b). This is a guess and is written as a comment.

Only (a) and (b) pairings are placed into a section, taken from the " - "
prefixes of the label text ("Group - Part - Name" gives the section
"Group - Part"). A pairing whose label has no prefix, and
every (c) guess, is left out so the setting lands in Other. Ranges and choices
are never guessed.

Entries already in --keep are copied unchanged: the keep file is written first,
verbatim. New entries follow in a block of their own. Section fallbacks come out
as "TODO rewrite: <heading>" because the heading is the game's text; replace it
with your own wording before committing. Nothing in the output may be copied
from global.ini.
"""

import argparse
import json
import re
import sys
import tomllib
import xml.etree.ElementTree as ET

EXE_KEY = re.compile(rb"@(pause_options?[A-Za-z0-9_]*)", re.IGNORECASE)
TAB_KEY = "pause_ForegroundOptionMenuButtonGameSettings"
TAB_FALLBACK = "Game settings"
SEPARATOR = " - "


def exe_keys(path):
    with open(path, "rb") as exe:
        data = exe.read()
    seen = {}
    for match in EXE_KEY.finditer(data):
        seen.setdefault(match.group(1).decode("ascii"), None)
    return list(seen)


def read_strings(path):
    strings = {}
    with open(path, encoding="utf-8-sig") as ini:
        for line in ini:
            key, sep, value = line.rstrip("\r\n").partition("=")
            if not sep:
                continue
            key = key.removesuffix(",P").strip().lower()
            strings[key] = value
    return strings


def read_attributes(path):
    root = ET.parse(path).getroot()
    return [
        (attr.get("name"), attr.get("value", ""))
        for attr in root.iter("Attr")
        if attr.get("name")
    ]


def squash(text):
    return re.sub(r"[^a-z0-9]", "", text.lower())


def pair_confident(names, keys):
    """Rules (a) and (b). Returns {attribute index: exe list index}."""
    by_lower = {key.lower(): i for i, key in enumerate(keys)}
    by_squashed = {}
    for i, key in enumerate(keys):
        by_squashed.setdefault(squash(re.sub(r"^pause_options?_?", "", key, flags=re.I)), i)
    taken, pairs = set(), {}
    for index, name in enumerate(names):
        found = by_lower.get(f"pause_options{name}".lower())
        if found is None:
            found = by_squashed.get(squash(name))
        if found is not None and found not in taken:
            taken.add(found)
            pairs[index] = found
    return pairs


def monotonic_anchors(pairs):
    kept, last = [], -1
    for index in sorted(pairs):
        if pairs[index] > last:
            kept.append((index, pairs[index]))
            last = pairs[index]
    return kept


def pair_guesses(count, keys, pairs):
    """Rule (c): attributes and exe strings left over between two anchors line up one to one."""
    anchors = [(-1, -1), *monotonic_anchors(pairs), (count, len(keys))]
    claimed = set(pairs.values())
    guesses = {}
    for (low_attr, low_pos), (high_attr, high_pos) in zip(anchors, anchors[1:]):
        attrs = [i for i in range(low_attr + 1, high_attr) if i not in pairs]
        spare = [p for p in range(low_pos + 1, high_pos) if p not in claimed]
        if attrs and len(attrs) == len(spare):
            guesses.update(zip(attrs, spare))
    return guesses


def kind_from_default(default):
    if default in ("0", "1"):
        return "toggle"
    try:
        float(default)
    except ValueError:
        return "text"
    return "number"


def fallback_label(name):
    spaced = re.sub(r"(?<=[a-z0-9])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])", " ", name)
    return spaced[:1].upper() + spaced[1:].lower()


def prefix_of(label):
    parts = label.split(SEPARATOR)
    return SEPARATOR.join(part.strip() for part in parts[:-1]) if len(parts) > 1 else None


def quoted(text):
    return json.dumps(text, ensure_ascii=True)


def entry_block(name, key, default, comment=""):
    lines = [
        "[[tab.section.entry]]",
        f"name = {quoted(name)}",
        f"label_key = {quoted(key)}",
        f"fallback = {quoted(fallback_label(name))}",
        f'kind = "{kind_from_default(default)}" # from the default value, check',
    ]
    if comment:
        lines.insert(0, comment)
    return lines


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--exe", required=True)
    parser.add_argument("--attributes", required=True)
    parser.add_argument("--strings", required=True)
    parser.add_argument("--keep")
    args = parser.parse_args()

    keys = exe_keys(args.exe)
    strings = read_strings(args.strings)
    attributes = read_attributes(args.attributes)
    names = [name for name, _ in attributes]
    defaults = dict(attributes)

    kept_text, kept_names = "", set()
    if args.keep:
        with open(args.keep, encoding="utf-8") as keep:
            kept_text = keep.read()
        kept = tomllib.loads(kept_text)
        kept_names = {
            entry["name"]
            for tab in kept.get("tab", [])
            for section in tab.get("section", [])
            for entry in section.get("entry", [])
        }

    confident = pair_confident(names, keys)
    guesses = pair_guesses(len(names), keys, confident)

    sections = {}
    unplaced = []
    for index, key_pos in sorted(confident.items()):
        name, key = names[index], keys[key_pos]
        if name in kept_names:
            continue
        prefix = prefix_of(strings.get(key.lower(), ""))
        if prefix is None:
            unplaced.append(name)
            continue
        sections.setdefault(prefix, []).append(entry_block(name, key, defaults[name]))

    out = []
    if kept_text:
        out.append(kept_text.rstrip("\n"))
        out.append("")
    if sections:
        out += [
            "# New in this draft. Review each entry, move it into the tab it belongs to",
            "# and replace every TODO with your own wording.",
            "[[tab]]",
            f"key = {quoted(TAB_KEY)}",
            f"fallback = {quoted(TAB_FALLBACK)}",
            "",
        ]
        for prefix, entries in sections.items():
            out += ["[[tab.section]]", f"fallback = {quoted('TODO rewrite: ' + prefix)}", ""]
            for entry in entries:
                out += entry + [""]

    guessed = [(i, p) for i, p in sorted(guesses.items()) if names[i] not in kept_names]
    if guessed:
        out += ["# Guesses from the exe list position. Not placed. Uncomment one only if it matches the game."]
        for index, key_pos in guessed:
            block = entry_block(names[index], keys[key_pos], defaults[names[index]], "guess")
            out += ["# " + line for line in block] + [""]

    sys.stdout.write("\n".join(out).rstrip("\n") + "\n")

    absent = sorted(kept_names - set(names))
    print(
        f"{len(keys)} exe strings, {len(names)} attributes, {len(confident)} by name, "
        f"{len(guesses)} guessed, {len(unplaced)} named but without a heading prefix, "
        f"{len(kept_names)} kept, {len(absent)} kept but not in this build",
        file=sys.stderr,
    )
    for name in absent:
        print(f"kept entry not in attributes.xml: {name}", file=sys.stderr)


if __name__ == "__main__":
    main()
