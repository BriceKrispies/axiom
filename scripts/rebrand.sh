#!/usr/bin/env bash
# rebrand.sh — case-preserving whole-repo token rename (content + paths).
#
#   ./rebrand.sh <old> <new>          # e.g. ./rebrand.sh axiom graffiti
#   ./rebrand.sh <old> <new> --check  # dry run: report only, change nothing
#
# Reversible: ./rebrand.sh graffiti axiom  undoes  ./rebrand.sh axiom graffiti.
# Run from the repo root. Uses git (renames are staged via `git mv`).
set -euo pipefail

OLD="${1:?usage: rebrand.sh <old> <new> [--check]}"
NEW="${2:?usage: rebrand.sh <old> <new> [--check]}"
CHECK="${3:-}"

OLD_L="${OLD,,}"; NEW_L="${NEW,,}"          # axiom  -> graffiti
OLD_T="${OLD_L^}"; NEW_T="${NEW_L^}"        # Axiom  -> Graffiti
OLD_U="${OLD_L^^}"; NEW_U="${NEW_L^^}"      # AXIOM  -> GRAFFITI

say() { printf '\n== %s ==\n' "$*"; }

# ---- Pre-flight guards (never edit; just surface risk) ----------------------
say "pre-flight"
printf 'occurrences (any case): %s in %s files\n' \
  "$(git grep -ioh "$OLD_L" | wc -l)" "$(git grep -lie "$OLD_L" | wc -l)"

# (a) real-word collisions: token glued to letters on either side. These WILL be
# renamed (e.g. AxiomClient->GraffitiClient, usually desired). Eyeball for a
# genuine unrelated word (an "${OLD_L}atic"-style false positive).
say "token welded to other letters (review — all get renamed)"
git grep -iohE "[a-z]?${OLD_L}[a-z]+|[a-z]${OLD_L}" | sort | uniq -c | sort -rn | head -30 || true

# (b) files git treats as BINARY but that still contain the token. Blindly
# byte-rewriting a real binary corrupts it. Confirm each is actually text.
say "binary-flagged files containing the token (confirm they are text)"
comm -13 <(git grep -lIie "$OLD_L" | sort) <(git grep -lie "$OLD_L" | sort) || true

[ "$CHECK" = "--check" ] && { say "check-only: no changes made"; exit 0; }

# ---- Phase 1: content ------------------------------------------------------
# Case-sensitive, disjoint substitutions preserve casing. Applied to every
# tracked file containing the token (the .bin text fixtures included).
say "phase 1: rewriting file contents"
git grep -lie "$OLD_L" | tr '\n' '\0' | xargs -0 --no-run-if-empty \
  perl -i -pe "s/\Q${OLD_L}\E/${NEW_L}/g; s/\Q${OLD_T}\E/${NEW_T}/g; s/\Q${OLD_U}\E/${NEW_U}/g"

# ---- Phase 2: directories (deepest-first) ----------------------------------
say "phase 2: renaming directories"
git ls-files | awk -F/ '{p=""; for(i=1;i<NF;i++){p=(i==1?$i:p"/"$i); print p}}' \
  | sort -u \
  | awk -F/ -v t="$OLD_L" 'BEGIN{IGNORECASE=1} $NF ~ t {print NF"\t"$0}' \
  | sort -rn | cut -f2- \
  | while IFS= read -r d; do
      nb=$(basename "$d" | perl -pe "s/\Q${OLD_L}\E/${NEW_L}/g; s/\Q${OLD_T}\E/${NEW_T}/g; s/\Q${OLD_U}\E/${NEW_U}/g")
      pd=$(dirname "$d")
      new=$([ "$pd" = "." ] && printf '%s' "$nb" || printf '%s/%s' "$pd" "$nb")
      git mv "$d" "$new"
    done

# ---- Phase 3: files --------------------------------------------------------
say "phase 3: renaming files"
git ls-files | awk -F/ -v t="$OLD_L" 'BEGIN{IGNORECASE=1} $NF ~ t' \
  | while IFS= read -r f; do
      nb=$(basename "$f" | perl -pe "s/\Q${OLD_L}\E/${NEW_L}/g; s/\Q${OLD_T}\E/${NEW_T}/g; s/\Q${OLD_U}\E/${NEW_U}/g")
      pd=$(dirname "$f")
      new=$([ "$pd" = "." ] && printf '%s' "$nb" || printf '%s/%s' "$pd" "$nb")
      git mv "$f" "$new"
    done

# ---- Verify ----------------------------------------------------------------
say "verify"
printf 'remaining in contents: %s\n' "$(git grep -ic "$OLD_L" | wc -l)"
printf 'remaining in paths:    %s\n' "$(git ls-files | grep -ic "$OLD_L")"
say "done — now build: cargo check --workspace --all-targets"
