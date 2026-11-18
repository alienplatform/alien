#!/usr/bin/env bash
# Guards the pull-target compatibility table. The release prepare step bumps
# NEXT_RELEASE (bump-next-release.sh); this catches hand edits. Fails when
#  - NEXT_RELEASE is not newer than the latest release tag (a feature gated at
#    it would claim a version Operators already run without it), or
#  - a feature gate names NEXT_RELEASE instead of a literal version (the bump
#    at release would move the gate to the following release), or
#  - a feature gate is newer than NEXT_RELEASE, or
#  - a gate at or below the latest release is missing from that release's table.
# Holds both at the release commit (latest tag X-1, NEXT_RELEASE X+1) and after
# the tag (latest X, NEXT_RELEASE X+1).
set -euo pipefail

file="crates/alien-manager/src/routes/pull_compat.rs"
next=$(sed -n 's/^pub(crate) const NEXT_RELEASE: &str = "\([0-9.]*\)";/\1/p' "$file")
if [ -z "$next" ]; then
  echo "::error file=$file::NEXT_RELEASE not found"
  exit 1
fi

latest=$(git tag --list "v[0-9]*" --sort=-v:refname | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -n1 || true)
if [ -z "$latest" ]; then
  echo "::error::no release tag found; fetch tags before running this check"
  exit 1
fi
latest=${latest#v}

newest=$(printf '%s\n%s\n' "$next" "$latest" | sort -V | tail -n1)
if [ "$next" = "$latest" ] || [ "$newest" != "$next" ]; then
  echo "::error file=$file::NEXT_RELEASE is $next but the latest release is $latest. Set NEXT_RELEASE to the next version and give features that shipped their literal version."
  exit 1
fi
# Every `feature(name, since, ...)` call as "name<TAB>since", in any layout:
# `since` is a string literal or the constant NEXT_RELEASE.
gates() {
  perl -0777 -ne 's{/\*.*?\*/}{}gs; s{(^|\s)//[^\n]*}{$1}g; while (/\bfeature\(\s*("(?:[^"\\]|\\.)*")\s*,\s*("[0-9.]*"|NEXT_RELEASE)\s*,/g) { print "$1\t$2\n" }'
}

current=$(gates < "$file")
if [ -z "$current" ]; then
  echo "::error file=$file::no feature gates found; the check cannot read this file"
  exit 1
fi

if printf '%s\n' "$current" | grep -q $'\tNEXT_RELEASE$'; then
  printf '%s\n' "$current" | grep $'\tNEXT_RELEASE$'
  echo "::error file=$file::A feature gate uses NEXT_RELEASE; write the literal version ($next) so the release bump does not move it."
  exit 1
fi

while IFS=$'\t' read -r name since; do
  since=${since//\"/}
  if [ "$(printf '%s\n%s\n' "$since" "$next" | sort -V | tail -n1)" != "$next" ]; then
    echo "::error file=$file::The gate $name names $since, newer than NEXT_RELEASE $next."
    exit 1
  fi
done <<< "$current"

# A gate at or below the latest release says Operators of that release read the
# feature. A gate written for the next release that merges after it shipped
# would claim exactly that, wrongly, so a gate at or below the latest release
# must already be in that release's copy of the table, by name, and may not be
# lower than the tag's version (the tag's gates may still read NEXT_RELEASE,
# which then means the tag's own NEXT_RELEASE value). The one legitimate way to gate a
# feature that already shipped is to list it below, in a reviewed edit of this
# script, once its code is confirmed to be in that release.
SHIPPED_WITHOUT_A_GATE=(
  # External Bedrock AI binding: in 3.3.69, gate added after that release.
  # Entry: name, a tab, the release it shipped in (the gate may not go lower).
  '"external AI bindings that use the workload'"'"'s cloud identity (Bedrock)"'$'\t3.3.69'
)
tagfile=$(git show "v$latest:$file" 2>/dev/null || true)
shipped=$(printf '%s\n' "$tagfile" | gates || true)
if [ -z "$shipped" ]; then
  echo "::error::v$latest has no pull-target table at $file; cannot compare gates with the latest release"
  exit 1
fi
# The tag's NEXT_RELEASE, for gates that still read the constant there.
tag_next=$(printf '%s\n' "$tagfile" | sed -n 's/^pub(crate) const NEXT_RELEASE: &str = "\([0-9.]*\)";/\1/p')
while IFS=$'\t' read -r name since; do
  since=${since//\"/}
  if [ "$(printf '%s\n%s\n' "$since" "$latest" | sort -V | tail -n1)" != "$latest" ]; then
    continue
  fi
  tag_since=$(printf '%s\n' "$shipped" | awk -F'\t' -v n="$name" '$1 == n { print $2; exit }')
  if [ -n "$tag_since" ]; then
    tag_since=${tag_since//\"/}
    [ "$tag_since" = "NEXT_RELEASE" ] && tag_since=$tag_next
    # A shipped gate may not be lowered: Operators of that release were
    # judged against the tag's version.
    if [ "$(printf '%s\n%s\n' "$since" "$tag_since" | sort -V | tail -n1)" != "$since" ]; then
      echo "::error file=$file::The gate $name names $since, lower than $tag_since in release $latest. A shipped gate cannot be lowered."
      exit 1
    fi
    continue
  fi
  allowed=false
  for listed in "${SHIPPED_WITHOUT_A_GATE[@]}"; do
    if [ "${listed%%$'\t'*}" = "$name" ]; then
      allowed=true
      floor=${listed#*$'\t'}
      if [ "$(printf '%s\n%s\n' "$since" "$floor" | sort -V | tail -n1)" != "$since" ]; then
        echo "::error file=$file::The gate $name names $since, lower than $floor, the release it shipped in. A shipped gate cannot be lowered."
        exit 1
      fi
    fi
  done
  if [ "$allowed" != true ]; then
    echo "::error file=$file::The gate $name names $since, but release $latest has no such gate: Operators of $latest may not read the feature. Gate it at $next, or if it shipped in $since, list it in SHIPPED_WITHOUT_A_GATE."
    exit 1
  fi
done <<< "$current"
echo "NEXT_RELEASE $next is newer than the latest release $latest"
