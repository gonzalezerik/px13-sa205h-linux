#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-or-later
# Static accessibility checks for the settings window. They run from
# the workspace test suite (crates/px13pen-settings/tests/a11y.rs), so a
# widget that cannot announce itself fails the build long before it
# reaches a screen reader. Portable: POSIX sh, grep and sed only.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
src="$root/crates/px13pen-settings/src"
if [ ! -d "$src" ]; then
    echo "check.sh: no sources at $src" >&2
    exit 2
fi

fails=0

report() {
    printf '%s:%s: %s\n' "$2" "$3" "$1"
    fails=$((fails + 1))
}

# near FILE LINE SPAN PATTERN — is PATTERN within SPAN lines of LINE?
near() {
    _file=$1 _line=$2 _span=$3 _pat=$4
    _lo=$((_line - _span))
    [ "$_lo" -lt 1 ] && _lo=1
    sed -n "${_lo},$((_line + _span))p" "$_file" | grep -q -- "$_pat"
}

for file in "$src"/*.rs; do
    # A button built around an icon has to name itself some other way.
    for line in $(grep -nE 'Button::builder|Button::new\(' "$file" | cut -d: -f1); do
        if near "$file" "$line" 6 '.icon_name('; then
            if ! near "$file" "$line" 6 '\.label(\|set_label\|accessible'; then
                report "icon button without a text or accessible label" "$file" "$line"
            fi
        fi
    done

    # A slider carries no title of its own; it needs an accessible name.
    for line in $(grep -nE 'Scale::builder|Scale::new\(' "$file" | cut -d: -f1); do
        if ! near "$file" "$line" 8 'accessible::Property::Label\|accessible_label'; then
            report "slider without an accessible label" "$file" "$line"
        fi
    done

    # A switch row inside a card is identified by its title alone.
    for line in $(grep -n 'SwitchRow::builder' "$file" | cut -d: -f1); do
        if ! near "$file" "$line" 4 '\.title('; then
            report "switch row without a title" "$file" "$line"
        fi
    done

    # An entry row says what you are typing into through its title.
    for line in $(grep -n 'EntryRow::builder' "$file" | cut -d: -f1); do
        if ! near "$file" "$line" 4 '\.title('; then
            report "entry row without a title" "$file" "$line"
        fi
    done

    # Every window says what it is.
    if grep -q 'ApplicationWindow::new(' "$file"; then
        if ! grep -q '\.set_title(\|WindowTitle' "$file"; then
            report "window without a title" "$file" 1
        fi
    fi
done

if [ "$fails" -gt 0 ]; then
    echo "check.sh: $fails accessibility issue(s)" >&2
    exit 1
fi
echo "check.sh: no accessibility issues in $src"
