#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# Meyatu Code — Performance Baseline Script (DEVELOPMENT_GUIDE §15)
#
# Targets:
#   Cold start        ≤ 3s
#   Binary size       ≤ 50MB (release), measured from debug for now
#   Memory (idle)     ≤ 200MB
#   Memory (chatting) ≤ 500MB
# ---------------------------------------------------------------------------
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TAURI_DIR="$PROJECT_DIR/src-tauri"
NOW="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

echo "=========================================="
echo "  Meyatu Code — Performance Baseline"
echo "  Timestamp: $NOW"
echo "=========================================="
echo ""

# ── Binary size ───────────────────────────────────────────────────────

DEBUG_BIN="$TAURI_DIR/target/debug/meyatu-code"
RELEASE_BIN="$TAURI_DIR/target/release/meyatu-code"

if [[ -f "$DEBUG_BIN" ]]; then
    SIZE=$(du -h "$DEBUG_BIN" | cut -f1)
    BYTES=$(stat -c%s "$DEBUG_BIN" 2>/dev/null || stat -f%z "$DEBUG_BIN" 2>/dev/null)
    MB=$(( BYTES / 1024 / 1024 ))
    echo "[binary] debug: ${SIZE} — ${MB}MB"
else
    echo "[binary] debug: not built"
fi

if [[ -f "$RELEASE_BIN" ]]; then
    SIZE=$(du -h "$RELEASE_BIN" | cut -f1)
    BYTES=$(stat -c%s "$RELEASE_BIN" 2>/dev/null || stat -f%z "$RELEASE_BIN" 2>/dev/null)
    MB=$(( BYTES / 1024 / 1024 ))
    echo "[binary] release: ${SIZE} — ${MB}MB"
else
    echo "[binary] release: not built (build with: cargo build --release)"
fi

echo ""

# ── Cold start timing ─────────────────────────────────────────────────

echo "[cold-start] Requires display (X11/Wayland). Skipping."
echo "  Run manually:  time $DEBUG_BIN"
echo ""

# ── Build timing ──────────────────────────────────────────────────────

echo "[build] Measuring clean release build time..."
cd "$PROJECT_DIR"
START_TS=$(date +%s)
cargo build --release 2>&1 | tail -1
END_TS=$(date +%s)
BUILD_SEC=$(( END_TS - START_TS ))
echo "[build] Release build: ${BUILD_SEC}s"
echo ""

if [[ -f "$RELEASE_BIN" ]]; then
    SIZE=$(du -h "$RELEASE_BIN" | cut -f1)
    BYTES=$(stat -c%s "$RELEASE_BIN" 2>/dev/null || stat -f%z "$RELEASE_BIN" 2>/dev/null)
    MB=$(( BYTES / 1024 / 1024 ))
    echo "[binary] release (post-build): ${SIZE} — ${MB}MB"
fi

echo ""
echo "=========================================="
echo "  Baseline captured at $NOW"
echo "=========================================="
