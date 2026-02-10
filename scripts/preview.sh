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

set -euo pipefail

PORT=8000
SKIP_BUILD=false
REPO_URL="https://github.com/altendky/mujou"

# --- Parse arguments ---
while [[ $# -gt 0 ]]; do
	case "$1" in
	--port)
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
	dx bundle --release --package mujou --platform web --base-path app

	echo "==> Assembling preview directory..."
	rm -rf "$PREVIEW_DIR"
	mkdir -p "$PREVIEW_DIR/app"

	# Static landing page
	cp -r "$ROOT/site/"* "$PREVIEW_DIR/"
	sed -i "s|{{REPO_URL}}|${REPO_URL}|g" "$PREVIEW_DIR/index.html"

	# Dioxus WASM app
	cp -r "$ROOT/target/dx/mujou/release/web/public/"* "$PREVIEW_DIR/app/"
	cp "$PREVIEW_DIR/app/index.html" "$PREVIEW_DIR/app/404.html"

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
echo "    Press Ctrl+C to stop."
echo ""
python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$PREVIEW_DIR"
