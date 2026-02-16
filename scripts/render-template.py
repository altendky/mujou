#!/usr/bin/env python3
"""Render a template file by replacing ``{{KEY}}`` placeholders.

Usage::

    render-template.py INPUT OUTPUT KEY=value [KEY=@file] ...

Values prefixed with ``@`` are read from the named file (trailing
newline stripped).  All replacements are literal string substitutions
— no regex or special-character interpretation.

Examples::

    # Simple value
    render-template.py in.html out.html REPO_URL=https://github.com/org/repo

    # Value read from a file
    render-template.py in.html out.html ANALYTICS=@site/analytics.html
"""

# TODO: Review the templating setup — if placeholder count or complexity
# grows, consider a proper templating library (e.g., Jinja2) or a
# unified build step that handles all site asset processing.

from __future__ import annotations

import pathlib
import sys


def main() -> None:
    if len(sys.argv) < 3:
        print(
            f"Usage: {sys.argv[0]} INPUT OUTPUT KEY=value ...",
            file=sys.stderr,
        )
        sys.exit(1)

    input_path = pathlib.Path(sys.argv[1])
    output_path = pathlib.Path(sys.argv[2])

    replacements: dict[str, str] = {}
    for arg in sys.argv[3:]:
        key, sep, value = arg.partition("=")
        if not key or not sep:
            print(
                f"Invalid argument: {arg!r} (expected KEY=value)",
                file=sys.stderr,
            )
            sys.exit(1)
        if value.startswith("@"):
            value = pathlib.Path(value[1:]).read_text().rstrip("\n")
        replacements[key] = value

    html = input_path.read_text()
    for key, value in replacements.items():
        placeholder = "{{" + key + "}}"
        html = html.replace(placeholder, value)

    output_path.write_text(html)


if __name__ == "__main__":
    main()
