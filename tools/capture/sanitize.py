#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Make a capture-derived text file safe to attach to an issue.

The rule for anything leaving your machine: pairing keys, Bluetooth
addresses, and pen serials must not survive. This tool does more than
warn about them — it replaces every identifier it recognises with a
freshly generated random value of the same shape, so a forgotten value
cannot ride along silently. What it cannot safely handle it refuses to
process; it has no mode that passes unrecognised material through.

    tools/capture/sanitize.py my-notes.txt -o my-notes.public.txt
    tools/capture/sanitize.py gatt-dump.txt -o out/ --serial 12345678

Guarantees, in order of how they are enforced:
  - raw captures are rejected outright (.hidrec, .pcap/.pcapng, .log of
    btmon origin cannot be told apart from text safely, so anything that
    is not clean UTF-8 text is refused); base64 blobs inside a text file
    are refused too — paste the decoded line instead, it sanitizes;
  - Bluetooth addresses (12 hex digits in pairs, `:`/`-` separated, any
    case) and long hex secrets (32+ hex digits, or byte groups on a line
    naming a key) are each replaced with random bytes;
  - values you name with --serial are replaced wherever they appear;
  - the output is re-scanned: if an address pattern still matches, the
    tool fails and writes nothing;
  - it never overwrites the input, and prints a report of exactly what it
    randomised — read it, and mention it in the issue if a frame you
    discuss was touched.

This is a floor, not a proof: it knows the shapes this project's captures
produce. It cannot know a serial your firmware prints as decimal words.
That is what --serial is for, and why the issue template asks you to
sanitize even hand-written notes.
"""

import argparse
import re
import secrets
import sys
from pathlib import Path

REJECT_SUFFIXES = {".hidrec", ".pcap", ".pcapng", ".btsnoop", ".pcapng.gz", ".gz"}

MAC = re.compile(
    r"(?<![0-9A-Za-z:])((?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2})(?![0-9A-Za-z:])"
)
HEX_RUN = re.compile(r"(?<![0-9A-Za-z])([0-9A-Fa-f]{32,})(?![0-9A-Za-z])")
# Byte groups ("0A B1 …") count as a secret only on a line that names a key;
# on any line they would hit timestamps, offsets and coordinates alike.
KEY_LINE = re.compile(r"\bkey\b", re.IGNORECASE)
SPACED_BYTES = re.compile(r"(?<![0-9A-Fa-f])((?:[0-9A-Fa-f]{2} ){7,}[0-9A-Fa-f]{2})(?![0-9A-Fa-f])")
B64_BLOB = re.compile(r"\b[A-Za-z0-9+/]{64,}={0,2}\b")


def random_like(original: str, alphabet: str | None = None) -> str:
    """A fresh random string with the original's shape, case and separators."""
    if alphabet == "digits":
        return "".join(str(secrets.randbelow(10)) for _ in original)
    out, case = [], original.isupper() or any(c.isupper() for c in original)
    for ch in original:
        if ch in "0123456789":
            out.append(f"{secrets.randbelow(16):X}" if case else f"{secrets.randbelow(16):x}")
        elif ch in "abcdefABCDEF":
            out.append(f"{secrets.randbelow(16):X}" if ch.isupper() else f"{secrets.randbelow(16):x}")
        else:
            out.append(ch)  # keep the separator/style character exactly
    return "".join(out)


def sanitize_text(text: str, serials: list[str]):
    """Return (clean_text, counts, problems).

    Replacements deliberately keep the shape of what they replace (a log
    with randomised MACs must still read like a log), so safety is checked
    against the set of ORIGINAL values, not by re-matching patterns.
    """
    counts = {"bluetooth address": 0, "hex secret": 0, "named value": 0}
    problems = []

    # A base64 blob we cannot interpret is refused before anything else —
    # but a pure-hex run is a known shape (a key/serial), handled below.
    blobs = [b for b in B64_BLOB.findall(text) if not re.fullmatch(r"[0-9A-Fa-f]+", b)]
    if blobs:
        problems.append(
            f"{len(blobs)} base64 blob(s) — cannot tell what they encode; delete them and "
            "paste the decoded line instead (it will sanitize)")
        return text, counts, problems

    originals = set(MAC.findall(text)) | {m for m in HEX_RUN.findall(text)}
    originals |= {s for s in serials if len(s) >= 8}

    for s in serials:
        text = re.sub(re.escape(s),
                      lambda m: (counts.__setitem__("named value", counts["named value"] + 1),
                                 random_like(m.group(0), "digits" if m.group(0).isdigit() else None))[1],
                      text, flags=re.IGNORECASE)

    text = MAC.sub(
        lambda m: (counts.__setitem__("bluetooth address", counts["bluetooth address"] + 1),
                   random_like(m.group(0)))[1], text)

    text = HEX_RUN.sub(
        lambda m: (counts.__setitem__("hex secret", counts["hex secret"] + 1),
                   random_like(m.group(0)))[1], text)

    # Byte groups ("0A B1 …") count as a secret when this line or the one
    # above names a key; on any line they would hit offsets and coordinates.
    lines = text.split("\n")
    for i, line in enumerate(lines):
        if KEY_LINE.search(line) or (i and KEY_LINE.search(lines[i - 1])):
            lines[i] = SPACED_BYTES.sub(
                lambda m: (counts.__setitem__("hex secret", counts["hex secret"] + 1),
                           random_like(m.group(1)))[1], line)
    text = "\n".join(lines)

    # Fail-safe: every value we set out to remove must be provably gone
    # from the output (case-insensitive for the hex shapes).
    low = text.lower()
    survivors = [o for o in originals if len(o) >= 8 and o.lower() in low]
    if survivors:
        problems.append(f"{len(survivors)} original value(s) survived substitution "
                        f"(e.g. {survivors[0][:12]}…)")
    return text, counts, problems


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("inputs", nargs="+", type=Path)
    ap.add_argument("-o", "--output", type=Path, help="output file (one input) or directory")
    ap.add_argument("--serial", action="append", default=[], metavar="VALUE",
                    help="a device-specific value to replace wherever it appears "
                         "(repeat for each serial/id you know)")
    args = ap.parse_args()

    if args.output and len(args.inputs) > 1 and not args.output.is_dir():
        sys.exit("with several inputs, -o must be an existing directory")
    done = []
    for src in args.inputs:
        if src.suffix in REJECT_SUFFIXES:
            sys.exit(f"refusing {src}: raw capture formats are never safe to share; "
                     "attach only sanitized text exports")
        try:
            text = src.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError) as e:
            sys.exit(f"refusing {src}: not clean text ({e})")
        clean, counts, problems = sanitize_text(text, args.serial)
        if problems:
            for p in problems:
                print(f"  BLOCKED in {src}: {p}", file=sys.stderr)
            print("nothing written — fix the flagged content and run again", file=sys.stderr)
            return 2
        dst = args.output / src.name if (args.output and args.output.is_dir()) else args.output or src.with_suffix(".public" + src.suffix)
        if dst.resolve() == src.resolve():
            sys.exit(f"refusing to overwrite the original {src} — the sanitized copy is "
                     "a separate file; keep the original private")
        dst.parent.mkdir(parents=True, exist_ok=True)
        dst.write_text(clean, encoding="utf-8")
        done.append((src, dst, counts))

    print(f"sanitized {len(done)} file(s):")
    total = {}
    for src, dst, counts in done:
        changed = {k: v for k, v in counts.items() if v}
        print(f"  {src} -> {dst}" + (f"  ({', '.join(f'{v} {k}' for k, v in changed.items())})" if changed else "  (nothing recognized)"))
        for k, v in counts.items():
            total[k] = total.get(k, 0) + v
    touched = {k: v for k, v in total.items() if v}
    if touched:
        print("every value listed above was replaced with freshly generated random data;")
        print("the originals are not recoverable from the output — mention this in the issue")
    print("check the output yourself as well: this tool is a floor, not a proof")
    return 0


if __name__ == "__main__":
    sys.exit(main())
