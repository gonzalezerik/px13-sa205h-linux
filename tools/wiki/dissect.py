#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Print the named fields of a captured report, from docs/wiki/layouts.json.

The wiki's reference pages are the prose authority; this database is the
same information for tools, in the envytools spirit — a dissector that
knows this hardware's verified layouts and stays silent about the rest.

Usage (from the repo root):
    tools/wiki/dissect.py --list
    tools/wiki/dissect.py 0x41.output 41 01 FF 09 00 00 00
    tools/wiki/dissect.py digitizer.0x6e '6e 01 ee dd cc bb aa 03'
    tools/wiki/dissect.py 7 '07 <...24 bytes...>'

A selector is either a database key or a report id; a bare id that exists
on more than one bus or in more than one kind lists the choices instead
of guessing. Hex bytes may be separated by spaces or run together.
"""

import argparse
import json
import sys
from pathlib import Path

DEFAULT_LAYOUTS = Path(__file__).resolve().parents[2] / "docs" / "wiki" / "layouts.json"


def parse_hex(args):
    digits = "".join(args).replace(",", " ").split()
    if len(digits) == 1 and len(digits[0]) % 2 == 0:
        digits = [digits[i : i + 2] for i in range(0, len(digits[0]), 2)]
    try:
        return bytes(int(d, 16) for d in digits)
    except ValueError as e:
        sys.exit(f"not hex: {e}")


def resolve(selector, reports):
    if selector in reports:
        return [selector]
    want = selector.lower().removeprefix("0x")
    matches = [
        key
        for key, r in reports.items()
        if isinstance(r.get("report_id"), str)
        and r["report_id"].lower().removeprefix("0x") == want
    ]
    if not matches:
        matches = [key for key in reports if key.endswith("." + selector)]
    return matches


def fmt_field(field, data):
    off, ln = field["offset"], field["length"]
    if off + ln > len(data):
        return f"  {field['name']:<26} (needs byte {off}..{off + ln}, frame has {len(data)})"
    raw = data[off : off + ln]
    kind = field.get("type", "")
    if kind == "address-le":
        shown = ":".join(f"{b:02X}" for b in reversed(raw))
    elif kind == "bits":
        bits = field.get("bit meanings", {})
        named = [f"{name}" for mask, name in bits.items() if raw[0] & int(mask, 16)]
        shown = f"0x{raw[0]:02X}" + (f" ({', '.join(named)})" if named else "")
    elif kind == "uint-le" and ln <= 8:
        value = int.from_bytes(raw, "little")
        table = field.get("values", {})
        named = table.get(str(value), table.get(f"0x{value:02X}"))
        if named is None and "3+" in table and value >= 3:
            named = table["3+"]
        unit = field.get("unit")
        shown = f"0x{raw.hex().upper()} = {value}"
        if unit:
            shown += f" ({value} x {unit})"
        if named:
            shown += f" — {named}"
    else:
        shown = raw.hex(" ").upper()
    fixed = field.get("value")
    if fixed:
        if kind == "uint-le":
            ok = int.from_bytes(raw, "little") == int(fixed, 16)
        else:
            ok = raw.hex() == fixed.lower().removeprefix("0x").zfill(ln * 2)
        if not ok:
            shown += f"  !! expected {fixed}"
    note = field.get("note")
    if note:
        shown += f"   [{note}]"
    return f"  {field['name']:<26} {shown}"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("selector", nargs="?", help="database key or report id")
    ap.add_argument("hexbytes", nargs="*", help="the frame, including the report id byte")
    ap.add_argument("--layouts", type=Path, default=DEFAULT_LAYOUTS)
    ap.add_argument("--list", action="store_true", help="list known reports")
    args = ap.parse_args()

    db = json.loads(args.layouts.read_text())
    reports = db["reports"]
    if args.list or not args.selector:
        for key, r in reports.items():
            print(f"{key:<20} {r['kind']:<13} {r['name']}")
        return
    matches = resolve(args.selector, reports)
    if not matches:
        sys.exit(f"unknown report '{args.selector}' — try --list")
    if len(matches) > 1:
        sys.exit(f"'{args.selector}' is ambiguous: {', '.join(matches)} — pick a key")

    key = matches[0]
    report = reports[key]
    data = parse_hex(args.hexbytes)
    print(f"{key}: {report['name']}  [{report.get('confidence', '?')}]")
    if report.get("size_bytes") and len(data) != report["size_bytes"]:
        print(f"  note: frame is {len(data)} B, documented size is {report['size_bytes']} B")
    for field in report.get("fields", []):
        print(fmt_field(field, data))
    if report.get("fields") and report["fields"][0].get("offset", 1) == 0:
        consumed = max(f["offset"] + f["length"] for f in report["fields"])
    else:
        consumed = 0
    if report.get("other_fields"):
        print(f"  other_fields               {report['other_fields']}")
    if len(data) > consumed:
        print(f"  bytes {consumed}..{len(data)}             {data[consumed:].hex(' ').upper()} (undocumented here)")
    for extra in ("transport_quirk", "note", "direction"):
        if report.get(extra):
            print(f"  {extra:<26} {report[extra]}")
    if report.get("decoded"):
        print(f"  decoded                    {json.dumps(report['decoded'])}")


if __name__ == "__main__":
    main()
