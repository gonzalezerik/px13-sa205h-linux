# SPDX-License-Identifier: AGPL-3.0-or-later
# The wiki toolchain: mkdocs with the Material theme merged into one
# environment so the `mkdocs` command and the theme share a Python.
{ python3 }:
python3.withPackages (ps: [
  ps.mkdocs
  ps.mkdocs-material
])
