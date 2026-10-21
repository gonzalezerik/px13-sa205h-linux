# SPDX-License-Identifier: AGPL-3.0-or-later
"""axe-core AA check for the built wiki.

Renders every page of a built mkdocs site in headless Chromium under both
colour schemes and fails on any WCAG 2.x AA violation. The wiki-a11y flake
check runs it against `nix build .#wiki`; the point is that no page ships
without this having said yes to it.

Usage: check.py <site-dir>   (site-dir is the mkdocs build output)
"""

import functools
import http.server
import json
import os
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

# The flake check pins axe-core through AXE_JS; next to this script works
# for a local run that already has the file.
AXE = Path(os.environ.get("AXE_JS", Path(__file__).with_name("axe.min.js")))
SCHEMES = ["light", "dark"]
# Every AA-tagged axe rule, no exclusions: an exclusion here needs a
# one-line reason and a plan to remove it.
RUN_ONLY = {"runOnly": {"type": "tag", "values": ["wcag2a", "wcag2aa"]}}


class _Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


def pages(site_dir: Path):
    return sorted(site_dir.rglob("*.html"))


def main(site_dir: Path) -> int:
    if not site_dir.is_dir():
        print(f"not a directory: {site_dir}", file=sys.stderr)
        return 2
    found = pages(site_dir)
    if not found:
        print(f"no pages under {site_dir} — built the site?", file=sys.stderr)
        return 2

    # Served over localhost, not file://, because the built 404 page loads
    # its assets with ../ relative URLs that resolve correctly only when the
    # page is served the way a real host serves them; over file:// the page
    # renders unstyled and the results are meaningless.
    handler = functools.partial(_Quiet, directory=str(site_dir))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    port = server.server_address[1]
    threading.Thread(target=server.serve_forever, daemon=True).start()

    failures = []
    with sync_playwright() as pw:
        browser = pw.chromium.launch()
        for scheme in SCHEMES:
            ctx = browser.new_context(color_scheme=scheme)
            page = ctx.new_page()
            for path in found:
                rel = path.relative_to(site_dir).as_posix()
                page.goto(f"http://127.0.0.1:{port}/{rel}")
                page.add_script_tag(path=str(AXE))
                result = page.evaluate(
                    "async () => await axe.run(document, %s)" % json.dumps(RUN_ONLY)
                )
                for v in result["violations"]:
                    for node in v["nodes"]:
                        failures.append((scheme, path.name, v["id"], v["help"],
                                         node["target"][0], node.get("failureSummary", "")[:200]))
            ctx.close()
        browser.close()
    server.shutdown()

    print(f"{len(found)} page(s) x {len(SCHEMES)} scheme(s)")
    if not failures:
        print("axe AA: clean")
        return 0
    for scheme, page, rule, help_, target, why in failures:
        print(f"  [{scheme}] {page}: {rule} — {help_}\n      {target}: {why}")
    print(f"axe AA: {len(failures)} violation(s)")
    return 1


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    sys.exit(main(Path(sys.argv[1])))
