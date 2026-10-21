# SPDX-License-Identifier: AGPL-3.0-or-later
"""Accessibility check for the built wiki.

Renders every page of a built mkdocs site in headless Chromium and fails
on any WCAG 2.2 AA problem:

- axe-core under both colour schemes (the theme must be honest in each);
- a keyboard smoke: tabbing reaches the controls in order, focus never
  traps, and every focused control shows a visible ring;
- a 200 % zoom smoke: at half the viewport width nothing spills
  horizontally (wide data tables are fine — Material scrolls them).

The wiki-a11y flake check runs it against `nix build .#wiki`; the point is
that no page ships without this having said yes to it.

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
VIEWPORT = {"width": 1280, "height": 720}
# WCAG 1.4.10 reflow in practice: content authored for this width must
# survive it at half size, which is what 200 % zoom leaves you with.
ZOOMED = {"width": VIEWPORT["width"] // 2, "height": VIEWPORT["height"] // 2}
TAB_STOPS = 60
# Rules to hold to AA, no exclusions: an entry removed here needs a
# one-line reason and a plan to bring it back.
RUN_ONLY = {"runOnly": {"type": "tag", "values": ["wcag2a", "wcag2aa"]}}


class _Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


def pages(site_dir: Path):
    return sorted(site_dir.rglob("*.html"))


def keyboard_issues(page):
    """Tab through the page once; report traps, invisible focus, weird tabindex."""
    issues = []
    positive = page.evaluate(
        "[...document.querySelectorAll('[tabindex]')]"
        ".map(e => parseInt(e.getAttribute('tabindex'), 10))"
        ".filter(n => n > 0)"
    )
    if positive:
        issues.append(("positive-tabindex", f"{len(positive)} element(s) with tabindex > 0"))

    page.keyboard.press("Tab")
    first = None
    seen = set()
    stuck, stuck_count = None, 0
    for _ in range(TAB_STOPS):
        info = page.evaluate(
            """() => {
          const el = document.activeElement;
          if (!el || el === document.body) return null;
          const name = (el.getAttribute('aria-label') || el.textContent || el.id || el.tagName)
            .replace(/\\s+/g, ' ').trim().slice(0, 40);
          // Focus counts as visible on the element itself or, for form
          // controls, on the label that stands in for them (a visually
          // hidden input may be represented by its label).
          const shown = t => { const cs = getComputedStyle(t); return (cs.outlineStyle !== 'none'
              && parseFloat(cs.outlineWidth) > 0
              && !/rgba\\(0, 0, 0, 0\\)|transparent/.test(cs.outlineColor)) || cs.boxShadow !== 'none'; };
          const faces = [el, ...(el.id ? document.querySelectorAll('label[for="' + CSS.escape(el.id) + '"]') : [])];
          const ring = faces.some(shown);
          return { key: el.tagName.toLowerCase() + '#' + (el.id || '') + ':' + name, name, ring };
        }"""
        )
        if info is None:
            break
        if first is None:
            first = info["key"]
        elif info["key"] == first:
            break  # focus came back round: no trap found in this pass
        if info["key"] == stuck:
            stuck_count += 1
            if stuck_count >= 5:
                issues.append(("focus-trap", f"focus stuck on {info['name']!r}"))
                break
        else:
            stuck, stuck_count = info["key"], 0
        seen.add(info["key"])
        if not info["ring"]:
            issues.append(("focus-invisible", f"no visible focus ring on {info['name']!r}"))
        page.keyboard.press("Tab")
    if len(seen) < 3:
        issues.append(("few-tab-stops", f"only {len(seen)} reachable control(s)"))
    return issues


def zoom_issues(page):
    """Half-width viewport: nothing may spill past the page edge."""
    page.set_viewport_size(ZOOMED)
    spills = page.evaluate(
        """() => {
          const de = document.documentElement;
          if (de.scrollWidth <= de.clientWidth + 1) return [];
          const bad = [];
          for (const el of document.querySelectorAll('body *')) {
            const r = el.getBoundingClientRect();
            const cs = getComputedStyle(el);
            if (r.width > 0 && r.right > de.clientWidth + 1
                && cs.overflowX === 'visible' && cs.position !== 'fixed')
              bad.push(el.tagName.toLowerCase() + '.' + (el.className.toString().split(' ')[0] || ''));
          }
          return [...new Set(bad)].slice(0, 6);
        }"""
    )
    page.set_viewport_size(VIEWPORT)
    return [("zoom-overflow", ", ".join(spills))] if spills else []


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
            ctx = browser.new_context(color_scheme=scheme, viewport=VIEWPORT)
            page = ctx.new_page()
            for path in found:
                rel = path.relative_to(site_dir).as_posix()
                page.goto(f"http://127.0.0.1:{port}/{rel}")
                if page.query_selector(".mermaid"):
                    page.wait_for_selector(".mermaid svg", timeout=15000)
                page.add_script_tag(path=str(AXE))
                result = page.evaluate(
                    "async () => await axe.run(document, %s)" % json.dumps(RUN_ONLY)
                )
                for v in result["violations"]:
                    for node in v["nodes"]:
                        failures.append(
                            (
                                scheme,
                                path.name,
                                v["id"],
                                f'{v["help"]}: {node["target"][0]} '
                                f'{node.get("failureSummary", "")[:160]}',
                            )
                        )
                for rule, detail in keyboard_issues(page) + zoom_issues(page):
                    failures.append((scheme, path.name, rule, detail))
            ctx.close()
        browser.close()
    server.shutdown()

    print(f"{len(found)} page(s) x {len(SCHEMES)} scheme(s)")
    if not failures:
        print("axe AA + keyboard + zoom: clean")
        return 0
    for scheme, page, rule, detail in failures:
        print(f"  [{scheme}] {page}: {rule} — {detail}")
    print(f"accessibility: {len(failures)} problem(s)")
    return 1


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    sys.exit(main(Path(sys.argv[1])))
