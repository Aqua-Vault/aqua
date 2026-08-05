#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# analyze_wasm.sh — Soroban WASM size & footprint analysis
#
# Builds both contracts (aqua_vault, mock_pool) as optimized WASM and reports
# the raw + gzipped artifact sizes plus a top-sections breakdown, as Markdown.
# Exits non-zero when a threshold is breached, so it can gate CI.
#
# Guardrails (configurable via environment):
#   MAX_WASM_KB        hard cap per artifact (default 200 KB). Soroban enforces
#                      a 2 MB ledger class-size limit; 200 KB is a generous
#                      budget that still catches accidental bloat early.
#   MAX_WASM_DELTA_KB  allowed growth vs a baseline build (default 15 KB),
#                      enforced when --baseline is passed.
#
# Usage:
#   scripts/analyze_wasm.sh [--baseline DIR]
#
#   --baseline DIR   Compare artifact sizes against previously built .wasm
#                    files under DIR (any build's target dir, e.g. the output
#                    of a `git worktree` build of the base branch). Fails when
#                    any artifact grew by more than MAX_WASM_DELTA_KB.
#
# How to read the output:
#   Raw      — uncompressed .wasm size; what is charged against the 2 MB
#              class-size limit and drives upload/storage fees.
#   Gzipped  — compressed size; proxy for upload/fee cost.
#   Top sections — the largest WASM sections (Code, Data, custom Soroban
#              contract metadata). Code dominates and grows with every helper
#              and panic string added to the crate, so watch its trend.
#
# Dependencies:
#   - stellar CLI (or cargo + the wasm32 target as a fallback) to build.
#   - wasm-tools (or wasm-objdump/llvm-objdump) for the section table; the
#     size guardrails work without them.
# =============================================================================

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PKGS="aqua_vault mock_pool"
MAX_WASM_KB="${MAX_WASM_KB:-200}"
MAX_WASM_DELTA_KB="${MAX_WASM_DELTA_KB:-15}"
BASELINE=""

while [ $# -gt 0 ]; do
    case "$1" in
        --baseline)
            BASELINE="${2:?error: --baseline requires a directory}"
            [ -d "$BASELINE" ] || { echo "error: baseline dir '$BASELINE' does not exist" >&2; exit 2; }
            shift 2
            ;;
        *) echo "error: unknown argument '$1'" >&2; exit 2 ;;
    esac
done

hr() {
    awk -v n="$1" 'BEGIN {
        if (n < 1024) printf "%d B", n
        else printf "%.1f KB", n / 1024
    }'
}

find_wasm() {
    local pkg="$1" base="$2"
    for triple in wasm32v1-none wasm32-unknown-unknown; do
        local f="$base/$triple/release/$pkg.wasm"
        if [ -f "$f" ]; then
            printf '%s\n' "$f"
            return 0
        fi
    done
    return 1
}

build_contracts() {
    if command -v stellar >/dev/null 2>&1; then
        echo "==> Building optimized WASM via stellar CLI..."
        for p in $PKGS; do
            stellar contract build --package "$p"
        done
    else
        echo "==> stellar CLI not found; building via cargo" \
             "(artifact sizes may differ from stellar-optimized builds)..."
        local target="wasm32v1-none"
        if command -v rustup >/dev/null 2>&1 && \
            ! rustup target list --installed | grep -qx "$target"; then
            target="wasm32-unknown-unknown"
        fi
        for p in $PKGS; do
            cargo build --release --target "$target" --package "$p"
        done
    fi
}

top_sections() {
    local file="$1"
    if command -v wasm-tools >/dev/null 2>&1; then
        local rows=()
        mapfile -t rows < <(wasm-tools objdump "$file" 2>/dev/null | awk -F'|' '
            NF >= 3 {
                name = $1; gsub(/^[ \t]+|[ \t]+$/, "", name);
                size = $3; gsub(/[^0-9]/, "", size);
                if (size != "") print size "\t" name;
            }' | sort -rn)
        printf '| Section | Bytes |\n| --- | ---: |\n'
        local size name
        for row in "${rows[@]}"; do
            IFS=$'\t' read -r size name <<<"$row"
            printf '| %s | %s |\n' "$name" "$size"
        done
    elif command -v wasm-objdump >/dev/null 2>&1; then
        wasm-objdump -h "$file" | sed 's/^/    /'
    else
        printf '_No disassembler found (wasm-tools / wasm-objdump); section table omitted._\n'
    fi
}

build_contracts

echo
echo "# Aqua WASM size report"
echo
printf '| Artifact | Raw | Gzipped |\n| --- | --- | --- |\n'

declare -A RAW
declare -a FILES
for p in $PKGS; do
    f="$(find_wasm "$p" "target")" || {
        echo "error: no wasm artifact found for '$p' after build" >&2
        exit 2
    }
    raw="$(stat -c %s "$f")"
    gz="$(gzip -9 -c "$f" | wc -c | tr -d '[:space:]')"
    RAW["$p"]="$raw"
    FILES+=("$f")
    printf '| %s | %s | %s |\n' "$p.wasm" "$(hr "$raw")" "$(hr "$gz")"
done

echo
for f in "${FILES[@]}"; do
    echo "## Top sections — $(basename "$f")"
    echo
    top_sections "$f"
    echo
done

FAILED=0
for p in $PKGS; do
    raw="${RAW[$p]}"
    if [ "$raw" -gt $((MAX_WASM_KB * 1024)) ]; then
        echo "FAIL: $p.wasm is $(hr "$raw"), exceeding the ${MAX_WASM_KB} KB limit" >&2
        FAILED=1
    fi
done

if [ -n "$BASELINE" ]; then
    for p in $PKGS; do
        base_file="$(find_wasm "$p" "$BASELINE")" || {
            echo "FAIL: no baseline artifact found for '$p' under '$BASELINE'" >&2
            FAILED=1
            continue
        }
        base_raw="$(stat -c %s "$base_file")"
        growth=$((RAW["$p"] - base_raw))
        if [ "$growth" -gt $((MAX_WASM_DELTA_KB * 1024)) ]; then
            echo "FAIL: $p.wasm grew by $(hr "$growth") vs baseline, exceeding the ${MAX_WASM_DELTA_KB} KB delta" >&2
            FAILED=1
        fi
    done
fi

echo "## Result"
if [ "$FAILED" -eq 0 ]; then
    echo "PASS: all artifacts within thresholds (max ${MAX_WASM_KB} KB, delta ${MAX_WASM_DELTA_KB} KB)."
    exit 0
else
    echo "FAIL: threshold breach detected (see errors above)." >&2
    exit 1
fi
