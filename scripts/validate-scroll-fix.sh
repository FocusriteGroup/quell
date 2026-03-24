#!/bin/bash
# validate-scroll-fix.sh — Manual validation of quell's scroll-fix behaviour.
#
# This script spawns quell with a program that emits Claude-like VT output
# including synchronized update blocks (DEC Mode 2026). It validates that:
#
#   1. BSU/ESU delimiters pass through to the terminal
#   2. ESC[2J (clear screen) inside sync blocks is allowed through
#   3. ESC[2J outside sync blocks is stripped (after initial startup clears)
#   4. Normal text passes through unmodified
#   5. Terminal state is restored after quell exits
#
# Usage:
#   ./scripts/validate-scroll-fix.sh
#
# Expected visual behaviour:
#   - You should see numbered lines of output (simulating streaming)
#   - Then a "full redraw" message appears (this is a sync block update)
#   - The terminal should NOT jump to the top of the scrollback
#   - After completion, your terminal should be in normal mode (echo works)
#
# If the scroll-fix is broken, you would see:
#   - The terminal viewport jumping during sync block updates
#   - Flickering or blank screen during redraws

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
QUELL_BIN="${SCRIPT_DIR}/../target/debug/quell"

if [ ! -x "$QUELL_BIN" ]; then
    echo "Building quell..."
    cargo build --manifest-path="${SCRIPT_DIR}/../Cargo.toml"
fi

# Create a helper script that emits Claude-like VT output
HELPER=$(mktemp /tmp/quell-validate-XXXXXX.sh)
trap 'rm -f "$HELPER"' EXIT

cat > "$HELPER" << 'INNER_EOF'
#!/bin/bash
# Simulate Claude-like terminal output with synchronized updates

BSU="\033[?2026h"
ESU="\033[?2026l"
CLEAR="\033[2J"
HOME="\033[H"

echo "=== quell scroll-fix validation ==="
echo ""

# Phase 1: Normal streaming output (should pass through unmodified)
echo "Phase 1: Streaming output..."
for i in $(seq 1 20); do
    echo "  Line $i: The quick brown fox jumps over the lazy dog."
    sleep 0.02
done
echo ""

# Phase 2: Synchronized update blocks (like Claude's UI redraws)
echo "Phase 2: Synchronized update with clear screen..."
sleep 0.2

# This is what Claude does: BSU + clear + cursor home + redraw + ESU
# With quell's OutputFilter, ESC[2J inside the sync block is allowed through
# (the terminal renders atomically, so no viewport jump occurs)
printf "${BSU}${CLEAR}${HOME}"
echo "=== FULL REDRAW (inside sync block) ==="
echo "This content replaced the previous screen atomically."
echo "You should NOT have seen a scroll-jump."
for i in $(seq 1 10); do
    echo "  Redraw line $i"
done
printf "${ESU}"
sleep 0.5

# Phase 3: Another streaming section
echo ""
echo "Phase 3: More streaming after sync block..."
for i in $(seq 1 5); do
    echo "  Post-redraw line $i"
    sleep 0.02
done

# Phase 4: Clear screen outside sync block (should be stripped by quell)
echo ""
echo "Phase 4: Clear screen outside sync block (should be stripped)..."
sleep 0.2
printf "${CLEAR}${HOME}"
echo "If you can still see Phase 3 output above, the clear was stripped correctly."
echo "(If the screen was cleared, the filter did not catch it.)"
sleep 0.5

echo ""
echo "=== Validation complete ==="
echo "Check:"
echo "  [1] No scroll-jumping during Phase 2 sync block"
echo "  [2] Phase 3 output visible after Phase 4 clear attempt"
echo "  [3] Terminal restored to normal after exit"
INNER_EOF
chmod +x "$HELPER"

echo "Running quell with simulated Claude-like output..."
echo "(Press Ctrl-C to abort if needed)"
echo ""

"$QUELL_BIN" /bin/bash "$HELPER"

echo ""
echo "Post-exit check: if you can read this, terminal mode was restored."
echo "Try typing — if echo works, terminal state is correct."
