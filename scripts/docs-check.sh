#!/usr/bin/env bash
set -euo pipefail

fail=0

check_file() {
    local path="$1"
    if [[ ! -e "$path" ]]; then
        printf 'missing file: %s\n' "$path" >&2
        fail=1
    fi
}

check_link() {
    local source="$1"
    local link="$2"

    case "$link" in
        http://*|https://*|mailto:*|\#*) return 0 ;;
    esac

    local target="${link%%#*}"
    [[ -z "$target" ]] && return 0

    local resolved
    resolved="$(cd "$(dirname "$source")" && pwd)/$target"
    if [[ ! -e "$resolved" ]]; then
        printf 'broken link in %s: %s\n' "$source" "$link" >&2
        fail=1
    fi
}

check_links() {
    local files=("$@")
    local line source link
    while IFS= read -r line; do
        source="${line%%:*}"
        link="${line#*:}"
        check_link "$source" "$link"
    done < <(perl -nE 'while (/\[[^\]]+\]\(([^)]+)\)/g) { say "$ARGV:$1" }' "${files[@]}")
}

check_no_stale_progress_terms() {
    if rg -n '阶段 8.*进行中|阶段 9.*进行中|当前 HEAD 对应|stage 6' \
        README.md docs/README.md docs/status.md docs/ecscript-reference.md docs/shell-reference.md; then
        printf 'stale progress wording found\n' >&2
        fail=1
    fi
}

check_ecscript_builtin_coverage() {
    local name
    while IFS= read -r name; do
        if ! rg -q "\`$name\`|\`$name\\(" docs/ecscript-reference.md; then
            printf 'ecscript builtin missing from reference: %s\n' "$name" >&2
            fail=1
        fi
    done < <(rg -o '"[a-z_]+" => Some\(Builtin' src/ecscript/builtin/mod.rs | sed 's/"//g; s/ =>.*//')
}

check_shell_builtin_coverage() {
    local name
    while IFS= read -r name; do
        if ! rg -q "\`$name\`" docs/shell-reference.md; then
            printf 'shell builtin missing from reference: %s\n' "$name" >&2
            fail=1
        fi
    done < <(sed -n '/pub const BUILTIN_NAMES/,/];/p' src/builtin.rs | rg -o '"[^"]+"' | tr -d '"')
}

check_reference_markers() {
    rg -q '<!-- BEGIN CHECKED ECSCRIPT BUILTIN INDEX -->' docs/ecscript-reference.md || {
        printf 'missing ecscript builtin index begin marker\n' >&2
        fail=1
    }
    rg -q '<!-- END CHECKED ECSCRIPT BUILTIN INDEX -->' docs/ecscript-reference.md || {
        printf 'missing ecscript builtin index end marker\n' >&2
        fail=1
    }
}

check_file README.md
check_file docs/README.md
check_file docs/status.md
check_file docs/ecscript-reference.md
check_file docs/shell-reference.md
check_file docs/ecscript-manual.md
check_file docs/TODO.md
check_file docs/roadmap.md
check_file docs/design-archive.md
check_file examples/ecscript/README.md
check_file AGENTS.md
check_file CLAUDE.md
check_links README.md docs/README.md docs/status.md docs/ecscript-reference.md docs/shell-reference.md docs/ecscript-manual.md docs/TODO.md docs/roadmap.md docs/design-archive.md examples/ecscript/README.md AGENTS.md CLAUDE.md
check_no_stale_progress_terms
check_ecscript_builtin_coverage
check_shell_builtin_coverage
check_reference_markers

if [[ "$fail" -ne 0 ]]; then
    exit 1
fi

printf 'docs-check passed\n'
