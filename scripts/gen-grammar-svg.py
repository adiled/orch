#!/usr/bin/env python3
"""Generate docs/grammar.svg from the Orchfile EBNF grammar.

Uses the railroad-diagrams library (pip install railroad-diagrams).
This script is the rendering counterpart to grammar.ebnf — keep them in sync.
"""

import sys
import os

try:
    import railroad as rr
except ImportError:
    print("error: pip install railroad-diagrams", file=sys.stderr)
    sys.exit(1)


def T(text):
    """Terminal (keyword/literal)."""
    return rr.Terminal(text)


def NT(text):
    """Non-terminal reference."""
    return rr.NonTerminal(text)


def section_comment(text):
    return rr.Comment(text)


def build_diagrams():
    """Build all grammar rule diagrams, grouped by section."""
    diagrams = []

    def add(name, *items, label=None):
        d = rr.Diagram(rr.Sequence(*items), type="complex")
        diagrams.append((label or name, d))

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

    add("common_directive",
        rr.Choice(0,
            rr.Sequence(T("WORKDIR"), NT("path")),
            rr.Sequence(T("ENV"), NT("env_key"), T("="), NT("value")),
            rr.Sequence(T("ENV_FILE"), NT("path")),
            rr.Sequence(T("REQUIRES"), NT("service_name"),
                        rr.ZeroOrMore(NT("service_name"))),
            rr.Sequence(T("AFTER"), NT("service_name"),
                        rr.ZeroOrMore(NT("service_name"))),
            rr.Sequence(T("HEALTHCHECK"),
                        rr.Choice(0, NT("url"), NT("command_string"))),
            rr.Sequence(T("READINESS_TIMEOUT"), NT("duration")),
            rr.Sequence(T("ONESHOT"), NT("bool")),
            rr.Sequence(T("DISABLED"), NT("bool")),
            rr.Sequence(T("RECREATE"), NT("recreate_policy")),
            rr.Sequence(T("RESTART"), NT("restart_policy")),
            rr.Sequence(T("RESTART_DELAY"), NT("duration")),
            rr.Sequence(T("TIMEOUT_START"), NT("duration")),
            rr.Sequence(T("TIMEOUT_STOP"), NT("duration")),
            rr.Sequence(T("START_LIMIT_BURST"), NT("integer")),
            rr.Sequence(T("START_LIMIT_INTERVAL"), NT("duration")),
            rr.Sequence(T("MEMORY"), NT("memory_size")),
            rr.Sequence(T("CPUS"), NT("number")),
            rr.Sequence(T("CPU_QUOTA"), NT("percentage")),
            rr.Sequence(T("LIMIT_NOFILE"), NT("integer")),
            rr.Sequence(T("LIMIT_NPROC"), NT("integer")),
            rr.Sequence(T("TASKS_MAX"), NT("integer")),
            rr.Sequence(T("IO_WEIGHT"), NT("integer"),
                        section_comment("10-1000")),
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
        section_comment("max 63"),
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


def render_svg(diagrams, output_path):
    """Render all diagrams into a single SVG file."""
    with open(output_path, "w") as f:
        f.write('<?xml version="1.0" encoding="UTF-8"?>\n')
        f.write('<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">\n')

        # Collect all individual SVGs as strings first to compute layout
        items = []
        for name, diagram in diagrams:
            from io import StringIO
            buf = StringIO()
            diagram.writeSvg(buf.write)
            items.append((name, buf.getvalue()))

        f.write('<style>\n')
        f.write('  svg.railroad-group { font: bold 14px monospace; }\n')
        f.write('  .rule-label { font: bold 16px sans-serif; fill: #333; }\n')
        f.write('</style>\n')

        y_offset = 10
        for name, svg_content in items:
            # Write label
            f.write(f'<text x="10" y="{y_offset + 18}" class="rule-label">{name}</text>\n')
            y_offset += 30

            # Extract inner SVG dimensions and content
            # The railroad library outputs a full <svg> element; we embed as <g>
            import re
            # Get width/height from the SVG
            w_match = re.search(r'width="([^"]+)"', svg_content)
            h_match = re.search(r'height="([^"]+)"', svg_content)
            width = float(w_match.group(1)) if w_match else 800
            height = float(h_match.group(1)) if h_match else 100

            # Strip outer <svg> tags, keep inner content
            inner = re.sub(r'<\?xml[^>]*\?>\s*', '', svg_content)
            inner = re.sub(r'<svg[^>]*>', f'<g transform="translate(10,{y_offset})">', inner, count=1)
            inner = inner.replace('</svg>', '</g>')

            f.write(inner)
            f.write('\n')

            y_offset += height + 20

        # Now set the total dimensions
        f.write('</svg>\n')

        # Rewrite with correct dimensions
        total_height = y_offset + 10

    # Re-read and fix the outer SVG dimensions
    with open(output_path, "r") as f:
        content = f.read()

    content = content.replace(
        '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">',
        f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
        f'width="900" viewBox="0 0 900 {int(total_height)}">'
    )

    with open(output_path, "w") as f:
        f.write(content)


def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    output = os.path.join(root, "docs", "grammar.svg")
    os.makedirs(os.path.dirname(output), exist_ok=True)

    diagrams = build_diagrams()
    render_svg(diagrams, output)
    print(f"wrote {output}")


if __name__ == "__main__":
    main()
