#!/usr/bin/env bash
# Verifies orphan hygiene checks in release.sh using a throwaway git repository.
# Exports: process exit code for fail/pass assertions around check_orphans and --skip-hygiene.
# Dependencies: bash, git, mktemp, sed, grep.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
release_script="${script_dir}/release.sh"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/release-orphan-check.XXXXXX")"
repo_dir="${tmp_dir}/repo"
origin_dir="${tmp_dir}/origin.git"
bin_dir="${tmp_dir}/bin"
notes_file="${tmp_dir}/notes.md"

# Removes the temporary test repository and artifacts.
cleanup() {
  rm -rf "${tmp_dir}"
}
trap cleanup EXIT

# Prints a failure message and exits non-zero.
fail() {
  echo "release orphan check test failed: $*" >&2
  exit 1
}

mkdir -p "${repo_dir}/scripts" "${repo_dir}/.github/scripts" "${bin_dir}"
git init --bare "${origin_dir}" >/dev/null
git init -b main "${repo_dir}" >/dev/null
git -C "${repo_dir}" config user.name "Test User"
git -C "${repo_dir}" config user.email "test@example.com"
git -C "${repo_dir}" remote add origin "${origin_dir}"

cp "${release_script}" "${repo_dir}/scripts/release.sh"

cat <<'EOF' > "${repo_dir}/Cargo.toml"
[package]
name = "release-test"
version = "1.0.0"
edition = "2021"
EOF

cat <<'EOF' > "${repo_dir}/CHANGELOG.md"
# Changelog
EOF

cat <<'EOF' > "${repo_dir}/.github/scripts/check-changelog.sh"
#!/usr/bin/env bash
set -euo pipefail
exit 0
EOF
chmod +x "${repo_dir}/.github/scripts/check-changelog.sh"

cat <<'EOF' > "${bin_dir}/cargo"
#!/usr/bin/env bash
set -euo pipefail

case "${1:-}" in
  test)
    exit 0
    ;;
  metadata)
    : > Cargo.lock
    printf '{"packages":[],"workspace_members":[],"version":1}\n'
    exit 0
    ;;
  *)
    exit 0
    ;;
esac
EOF
chmod +x "${bin_dir}/cargo"

git -C "${repo_dir}" add Cargo.toml CHANGELOG.md .github/scripts/check-changelog.sh scripts/release.sh
git -C "${repo_dir}" commit -m "chore: seed release test repo" >/dev/null
git -C "${repo_dir}" push -u origin main >/dev/null

git -C "${repo_dir}" checkout -b merged-branch >/dev/null
echo "merged branch content" > "${repo_dir}/merged.txt"
git -C "${repo_dir}" add merged.txt
git -C "${repo_dir}" commit -m "feat: add merged branch content" >/dev/null
git -C "${repo_dir}" checkout main >/dev/null
git -C "${repo_dir}" merge --no-ff merged-branch -m "merge merged branch" >/dev/null

git -C "${repo_dir}" checkout -b live-branch >/dev/null
echo "live branch content" > "${repo_dir}/live.txt"
git -C "${repo_dir}" add live.txt
git -C "${repo_dir}" commit -m "feat: add live branch content" >/dev/null
git -C "${repo_dir}" checkout main >/dev/null

printf '%s\n' '- Release orphan hygiene coverage' > "${notes_file}"
sed '$d' "${repo_dir}/scripts/release.sh" > "${repo_dir}/scripts/release-lib.sh"

if (
  source "${repo_dir}/scripts/release-lib.sh"
  check_orphans
) >"${tmp_dir}/check.stdout" 2>"${tmp_dir}/check.stderr"; then
  fail "expected check_orphans to fail on merged-branch"
fi

grep -q 'merged-branch' "${tmp_dir}/check.stderr" || fail "missing merged orphan branch in report"
if grep -q 'live-branch' "${tmp_dir}/check.stderr"; then
  fail "reported live-branch as an orphan"
fi

rm -f "${repo_dir}/scripts/release-lib.sh"

PATH="${bin_dir}:${PATH}" bash "${repo_dir}/scripts/release.sh" --skip-hygiene 1.0.1 "${notes_file}" \
  >"${tmp_dir}/skip.stdout" 2>"${tmp_dir}/skip.stderr" \
  || fail "--skip-hygiene should allow release.sh to proceed"

grep -q 'merged-branch' "${tmp_dir}/skip.stderr" || fail "skip-hygiene run did not report orphan branch"
git -C "${repo_dir}" rev-parse -q --verify refs/tags/v1.0.1 >/dev/null \
  || fail "release.sh did not create v1.0.1 tag during skip-hygiene run"

exit 0
