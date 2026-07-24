#!/usr/bin/env bash

set -euo pipefail

readonly max_lines=1000
readonly repo_root="$(git rev-parse --show-toplevel)"
readonly baseline_file="$repo_root/scripts/max-lines-baseline.txt"

cd "$repo_root"

source_pathspecs=(
  ":(glob)*.rs"
  ":(glob)**/*.rs"
  ":(glob)*.ts"
  ":(glob)**/*.ts"
  ":(glob)*.tsx"
  ":(glob)**/*.tsx"
  ":(glob)*.js"
  ":(glob)**/*.js"
  ":(glob)*.jsx"
  ":(glob)**/*.jsx"
  ":(exclude,glob)client-sdks/**"
  ":(exclude,glob)packages/sdk/src/worker-runtime/generated/**"
)

git ls-files -z -- "${source_pathspecs[@]}" |
  xargs -0 wc -l |
  awk -v max_lines="$max_lines" '
    NR == FNR {
      if (NF != 2 || $1 !~ /^[0-9]+$/ || $1 <= max_lines) {
        printf "%s:%s has an invalid baseline entry\n", FILENAME, FNR
        invalid_baseline = 1
        next
      }
      if ($2 in baseline) {
        printf "%s:%s duplicates %s\n", FILENAME, FNR, $2
        invalid_baseline = 1
        next
      }
      baseline[$2] = $1
      next
    }
    $2 != "total" {
      line_count = $1
      path = $2
      seen[path] = 1

      if (line_count <= max_lines) {
        if (path in baseline) {
          printf "%s now has %s lines; remove its stale baseline entry\n",
            path, line_count
          found = 1
        }
        next
      }

      if (!(path in baseline)) {
        printf "%s has %s lines (max %s); split this file\n",
          path, line_count, max_lines
        found = 1
        next
      }

      if (line_count > baseline[path]) {
        printf "%s grew to %s lines (grandfathered cap %s); split it before adding code\n",
          path, line_count, baseline[path]
        found = 1
      }
    }
    END {
      for (path in baseline) {
        if (!(path in seen)) {
          printf "%s is missing; remove its stale baseline entry\n", path
          found = 1
        }
      }
      exit invalid_baseline || found
    }
  ' "$baseline_file" -
