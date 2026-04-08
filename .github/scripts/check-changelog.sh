#!/usr/bin/env bash
# Validates release metadata consistency for ai-dispatch.
# Checks Cargo.toml package version, optional release tag, and CHANGELOG.md.
# Dependencies: bash, awk, grep.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cargo_file="${repo_root}/Cargo.toml"
changelog_file="${repo_root}/CHANGELOG.md"
expected_tag="${1:-}"

fail() {
  echo "changelog validation failed: $*" >&2
  exit 1
}

package_version="$(
  awk -F'"' '
    /^\[package\]/ { in_package = 1; next }
    /^\[/ && in_package { exit }
    in_package && $1 ~ /^version = / { print $2; exit }
  ' "${cargo_file}"
)"

[[ -n "${package_version}" ]] || fail "could not read [package].version from Cargo.toml"
[[ -f "${changelog_file}" ]] || fail "CHANGELOG.md is missing"

if [[ -n "${expected_tag}" ]]; then
  expected_version="${expected_tag#v}"
  [[ "${package_version}" == "${expected_version}" ]] || fail \
    "tag ${expected_tag} does not match Cargo.toml version ${package_version}"
fi

expected_heading="## v${package_version} ("
first_heading="$(
  grep -m1 '^## v[0-9]\+\.[0-9]\+\.[0-9]\+ (' "${changelog_file}" || true
)"

[[ -n "${first_heading}" ]] || fail "CHANGELOG.md has no version headings"
[[ "${first_heading}" == "${expected_heading}"* ]] || fail \
  "latest CHANGELOG.md entry must be ${expected_heading}..."

section_has_bullets="$(
  awk -v expected="${expected_heading}" '
    index($0, expected) == 1 { in_section = 1; next }
    /^## v[0-9]+\.[0-9]+\.[0-9]+ \(/ && in_section { exit }
    in_section && /^- / { found = 1 }
    END { if (found) print "yes" }
  ' "${changelog_file}"
)"

[[ "${section_has_bullets}" == "yes" ]] || fail \
  "CHANGELOG.md entry for v${package_version} must include at least one bullet"

echo "changelog validation passed for v${package_version}"
