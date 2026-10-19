#!/bin/sh
# Turns `px13penctl status --json` (docs/px13penctl.md) into one Waybar JSON
# module. No connected pens means no output at all, which is how Waybar
# hides a custom module. Needs jq.
px13penctl status --json | jq -r '
  .pens
  | map(
      .name + " "
      + (if .battery == null then "battery ?"
         else (.battery | tostring) + "%" end)
    )
  | if length == 0 then empty
    else { text: join("  "), tooltip: join("\n") } | tostring
    end
'
