#!/usr/bin/env bash
# Build and serve a production-like preview of the full site
# (static landing page + Dioxus WASM app) on localhost.
#
# Usage:
#   ./scripts/preview.sh              # build and serve on port 8000
#   ./scripts/preview.sh --port 9000  # custom port
#   ./scripts/preview.sh --skip-build # serve previous build without rebuilding
#
# Mirrors the GitHub Pages deployment structure:
#   /          → static landing page  (from site/)
#   /app/      → Dioxus WASM app      (from dx bundle)
#   /book/     → mdBook documentation (from docs/)

set -euo pipefail

PORT=8000
SKIP_BUILD=false
REPO_URL="https://github.com/altendky/mujou"

# --- Parse arguments ---
while [[ $# -gt 0 ]]; do
	case "$1" in
	--port)
		if [[ $# -lt 2 ]]; then
			echo "Error: --port requires a value" >&2
			exit 1
		fi
		PORT="$2"
		shift 2
		;;
	--skip-build)
		SKIP_BUILD=true
		shift
		;;
	--help | -h)
		sed -n '2,/^$/{ s/^# //; s/^#$//; p }' "$0"
		exit 0
		;;
	*)
		echo "Unknown option: $1" >&2
		exit 1
		;;
	esac
done

# Resolve the workspace root (where this script lives in scripts/).
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PREVIEW_DIR="$ROOT/target/preview"

if [ "$SKIP_BUILD" = false ]; then
	echo "==> Building WASM app (dx bundle --release)..."
	cd "$ROOT"
	dx bundle --release --package mujou-app --platform web --base-path app

	echo "==> Building documentation (mdbook build)..."
	mdbook build "$ROOT/docs"

	echo "==> Assembling preview directory..."
	rm -rf "$PREVIEW_DIR"
	mkdir -p "$PREVIEW_DIR/app"

	# Static landing page
	cp -r "$ROOT/site/"* "$PREVIEW_DIR/"
	"$ROOT/scripts/render-template.py" "$PREVIEW_DIR/index.html" "$PREVIEW_DIR/index.html" \
		"REPO_URL=${REPO_URL}" \
		"ANALYTICS=@$ROOT/site/analytics.html"

	# Dioxus WASM app
	cp -r "$ROOT/target/dx/mujou-app/release/web/public/"* "$PREVIEW_DIR/app/"
	cp "$PREVIEW_DIR/app/index.html" "$PREVIEW_DIR/app/404.html"

	# mdBook documentation
	cp -r "$ROOT/docs/book" "$PREVIEW_DIR/book"

	echo "==> Preview directory assembled at $PREVIEW_DIR"
else
	if [ ! -d "$PREVIEW_DIR" ]; then
		echo "Error: No previous build found at $PREVIEW_DIR" >&2
		echo "Run without --skip-build first." >&2
		exit 1
	fi
	echo "==> Skipping build, using previous preview directory."
fi

echo "==> Serving on http://localhost:$PORT"
echo "    Landing page: http://localhost:$PORT/"
echo "    App:          http://localhost:$PORT/app/"
echo "    Book:         http://localhost:$PORT/book/"
echo "    Press Ctrl+C to stop."
echo ""

# Restore terminal settings in case the build process (wasm-pack) left
# them in a non-default state (e.g. disabled SIGINT for a progress bar).
stty sane 2>/dev/null || true

# exec replaces the shell with python, so ctrl+c sends SIGINT directly
# to the server process with no bash signal handling in between.
exec python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$PREVIEW_DIR"
