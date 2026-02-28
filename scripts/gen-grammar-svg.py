#!/usr/bin/env python3
"""Generate docs/grammar.svg from the Orchfile EBNF grammar.

Uses the railroad-diagrams library (pip install railroad-diagrams).
This script is the rendering counterpart to grammar.ebnf — keep them in sync.

GitHub strips <style> tags from SVGs for security, so we configure the
library to use inline styles and inject them via post-processing.
"""

import re
import sys
import os
from io import StringIO

try:
    import railroad as rr
except ImportError:
    print("error: pip install railroad-diagrams", file=sys.stderr)
    sys.exit(1)

# -- Configure library defaults for GitHub-safe rendering --
# These set the default CSS applied inline by the library.
rr.INTERNAL_ALIGNMENT = "left"


def T(text):
    """Terminal (keyword/literal)."""
    return rr.Terminal(text)


def NT(text):
    """Non-terminal reference."""
    return rr.NonTerminal(text)


def Cmt(text):
    """Annotation comment."""
    return rr.Comment(text)


def build_diagrams():
    """Build all grammar rule diagrams, grouped by section."""
    diagrams = []

    def add(name, *items):
        d = rr.Diagram(rr.Sequence(*items), type="complex")
        diagrams.append((name, d))

    # ── Document structure ──

    add("orchfile",
        rr.ZeroOrMore(
            rr.Sequence(
                rr.Choice(0,
                    NT("blank"),
                    NT("comment"),
                    NT("arg"),
                    NT("service_decl"),
                    NT("directive"),
                ),
                T("\\n"),
            ),
        ),
    )

    add("comment", T("#"), rr.ZeroOrMore(NT("any")))

    # ── Top-level ──

    add("arg",
        T("ARG"), NT("identifier"), T("="), NT("value"),
    )

    add("service_decl",
        T("SERVICE"), NT("service_name"),
    )

    # ── Directives ──

    add("directive",
        rr.Choice(0,
            NT("mode_directive"),
            NT("container_directive"),
            NT("host_directive"),
            NT("common_directive"),
        ),
    )

    add("mode_directive",
        rr.Choice(0,
            rr.Sequence(T("FROM"), NT("image_ref")),
            rr.Sequence(T("RUN"), NT("command_string")),
        ),
    )

    add("container_directive",
        rr.Choice(0,
            rr.Sequence(T("ENTRYPOINT"), NT("command_string")),
            rr.Sequence(T("CMD"), NT("command_string")),
            rr.Sequence(T("PUBLISH"), NT("port"), T(":"), NT("port")),
            rr.Sequence(T("VOLUME"), NT("volume_source"), T(":"), NT("path")),
        ),
    )

    add("host_directive",
        rr.Choice(0,
            rr.Sequence(T("USER"), NT("identifier")),
            rr.Sequence(T("STOP"), NT("command_string")),
            rr.Sequence(T("RELOAD"), NT("command_string")),
        ),
    )

    # Split common_directive into logical sub-groups to keep diagrams readable

    add("common_directive",
        rr.Choice(0,
            NT("env_directive"),
            NT("dependency_directive"),
            NT("health_directive"),
            NT("lifecycle_directive"),
            NT("restart_directive"),
            NT("timeout_directive"),
            NT("resource_directive"),
            NT("logging_directive"),
        ),
    )

    add("env_directive",
        rr.Choice(0,
            rr.Sequence(T("WORKDIR"), NT("path")),
            rr.Sequence(T("ENV"), NT("env_key"), T("="), NT("value")),
            rr.Sequence(T("ENV_FILE"), NT("path")),
        ),
    )

    add("dependency_directive",
        rr.Choice(0,
            rr.Sequence(T("REQUIRES"), NT("service_name"),
                        rr.ZeroOrMore(NT("service_name"))),
            rr.Sequence(T("AFTER"), NT("service_name"),
                        rr.ZeroOrMore(NT("service_name"))),
        ),
    )

    add("health_directive",
        rr.Choice(0,
            rr.Sequence(T("HEALTHCHECK"),
                        rr.Choice(0, NT("url"), NT("command_string"))),
            rr.Sequence(T("READINESS_TIMEOUT"), NT("duration")),
        ),
    )

    add("lifecycle_directive",
        rr.Choice(0,
            rr.Sequence(T("ONESHOT"), NT("bool")),
            rr.Sequence(T("DISABLED"), NT("bool")),
            rr.Sequence(T("RECREATE"), NT("recreate_policy")),
        ),
    )

    add("restart_directive",
        rr.Choice(0,
            rr.Sequence(T("RESTART"), NT("restart_policy")),
            rr.Sequence(T("RESTART_DELAY"), NT("duration")),
            rr.Sequence(T("START_LIMIT_BURST"), NT("integer")),
            rr.Sequence(T("START_LIMIT_INTERVAL"), NT("duration")),
        ),
    )

    add("timeout_directive",
        rr.Choice(0,
            rr.Sequence(T("TIMEOUT_START"), NT("duration")),
            rr.Sequence(T("TIMEOUT_STOP"), NT("duration")),
        ),
    )

    add("resource_directive",
        rr.Choice(0,
            rr.Sequence(T("MEMORY"), NT("memory_size")),
            rr.Sequence(T("CPUS"), NT("number")),
            rr.Sequence(T("CPU_QUOTA"), NT("percentage")),
            rr.Sequence(T("LIMIT_NOFILE"), NT("integer")),
            rr.Sequence(T("LIMIT_NPROC"), NT("integer")),
            rr.Sequence(T("TASKS_MAX"), NT("integer")),
            rr.Sequence(T("IO_WEIGHT"), NT("integer"), Cmt("10-1000")),
        ),
    )

    add("logging_directive",
        rr.Choice(0,
            rr.Sequence(T("STDOUT"), NT("path")),
            rr.Sequence(T("STDERR"), NT("path")),
        ),
    )

    # ── Terminals ──

    add("service_name",
        NT("letter"),
        rr.ZeroOrMore(
            rr.Choice(0, NT("letter"), NT("digit"), T("-")),
        ),
        Cmt("max 63"),
    )

    add("var_ref",
        T("${"), NT("identifier"), T("}"),
    )

    add("bool",
        rr.Choice(0, T("true"), T("false")),
    )

    add("restart_policy",
        rr.Choice(0, T("no"), T("always"), T("on-failure")),
    )

    add("recreate_policy",
        rr.Choice(0, T("always"), T("never")),
    )

    add("duration",
        NT("integer"), rr.Choice(0, T("s"), T("m")),
    )

    add("memory_size",
        NT("integer"), rr.Choice(0, T("K"), T("M"), T("G")),
    )

    add("percentage",
        NT("integer"), T("%"),
    )

    add("number",
        NT("integer"),
        rr.Optional(rr.Sequence(T("."), rr.OneOrMore(NT("digit")))),
    )

    return diagrams


def diagram_to_svg(diagram):
    """Render a single diagram to SVG string."""
    buf = StringIO()
    diagram.writeSvg(buf.write)
    return buf.getvalue()


def extract_dimensions(svg_str):
    """Extract width and height from an SVG string."""
    w = re.search(r'width="([^"]+)"', svg_str)
    h = re.search(r'height="([^"]+)"', svg_str)
    return (
        float(w.group(1)) if w else 600,
        float(h.group(1)) if h else 100,
    )


def strip_svg_wrapper(svg_str):
    """Remove outer <svg> tags, return inner content."""
    s = re.sub(r'<\?xml[^>]*\?>\s*', '', svg_str)
    s = re.sub(r'<svg[^>]*>', '', s, count=1)
    s = s.replace('</svg>', '')
    return s.strip()


def inject_inline_styles(inner_svg):
    """Replace class-based styling with inline styles for GitHub compatibility.

    GitHub strips <style> tags from SVGs. We inject inline attributes
    only into library-generated diagram content (not our wrapper elements).
    """
    # Strip all class attributes (GitHub ignores them without <style>)
    inner_svg = re.sub(r'\s*class="[^"]*"', '', inner_svg)

    # <rect> without existing fill: add white fill + dark border
    # Use negative lookahead to skip rects that already have fill=
    inner_svg = re.sub(
        r'<rect (?!fill=)',
        '<rect fill="#fff" stroke="#222" ',
        inner_svg,
    )

    # <path> without existing stroke: add dark stroke
    inner_svg = re.sub(
        r'<path (?!fill=)',
        '<path fill="none" stroke="#222" stroke-width="2" ',
        inner_svg,
    )

    # <text> without existing fill: add dark fill + font
    inner_svg = re.sub(
        r'<text (?!fill=)',
        '<text fill="#222" font-family="monospace" font-size="14" ',
        inner_svg,
    )

    return inner_svg


def render_svg(diagrams, output_path):
    """Render all diagrams into a single SVG file with inline styles."""
    # First pass: render each diagram and collect dimensions
    rendered = []
    max_width = 0
    for name, diagram in diagrams:
        svg_str = diagram_to_svg(diagram)
        w, h = extract_dimensions(svg_str)
        inner = strip_svg_wrapper(svg_str)
        max_width = max(max_width, w)
        rendered.append((name, inner, w, h))

    padding = 20
    total_width = max_width + padding * 2
    label_height = 28
    gap = 16

    # Second pass: lay out vertically
    y = padding
    body_parts = []

    for name, inner, w, h in rendered:
        # Inject inline styles into this diagram's content
        styled_inner = inject_inline_styles(inner)

        # Rule label
        body_parts.append(
            f'<text x="{padding}" y="{y + 18}" '
            f'fill="#333" font-family="sans-serif" font-size="16" font-weight="bold">'
            f'{name}</text>'
        )
        y += label_height

        # Diagram content
        body_parts.append(
            f'<g transform="translate({padding},{y})">{styled_inner}</g>'
        )
        y += h + gap

    total_height = y + padding

    # Assemble final SVG
    svg = (
        f'<svg xmlns="http://www.w3.org/2000/svg" '
        f'width="{int(total_width)}" '
        f'viewBox="0 0 {int(total_width)} {int(total_height)}">\n'
        f'<rect width="100%" height="100%" fill="#fff"/>\n'
    )
    svg += '\n'.join(body_parts)
    svg += '\n</svg>\n'

    with open(output_path, 'w') as f:
        f.write(svg)


def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    output = os.path.join(root, "docs", "grammar.svg")
    os.makedirs(os.path.dirname(output), exist_ok=True)

    diagrams = build_diagrams()
    render_svg(diagrams, output)
    print(f"wrote {output}")


if __name__ == "__main__":
    main()
