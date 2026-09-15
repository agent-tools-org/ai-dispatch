#!/usr/bin/env bash
# Verifies orphan hygiene checks in release.sh using a throwaway git repository.
# Exports: process exit code for fail/pass assertions around check_orphans, --dry-run, and --skip-hygiene.
# Dependencies: bash, git, mktemp, sed, grep, sqlite3.

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

touch "${repo_dir}/Cargo.lock"
git -C "${repo_dir}" add Cargo.toml Cargo.lock CHANGELOG.md .github/scripts/check-changelog.sh scripts/release.sh
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

if PATH="${bin_dir}:${PATH}" bash "${repo_dir}/scripts/release.sh" --dry-run 1.0.1 "${notes_file}" \
  >"${tmp_dir}/dry.stdout" 2>"${tmp_dir}/dry.stderr"; then
  fail "expected --dry-run to fail on orphan hygiene"
fi
grep -q 'dry-run: would fail: release hygiene check (1 orphan branches, 0 orphan worktrees)' \
  "${tmp_dir}/dry.stderr" || fail "missing dry-run hygiene failure message"
if grep -q 'would create tag' "${tmp_dir}/dry.stdout" "${tmp_dir}/dry.stderr"; then
  fail "dry-run printed would-create-tag despite hygiene failure"
fi

command -v sqlite3 >/dev/null 2>&1 || fail "sqlite3 is required for worktree task-id mapping coverage"

wt_dir="${tmp_dir}/orphan-wt"
git -C "${repo_dir}" worktree add "${wt_dir}" merged-branch >/dev/null
wt_listed=""
while IFS= read -r line; do
  case "${line}" in
    worktree\ *)
      p="${line#worktree }"
      [[ "${p}" != "${repo_dir}" ]] && wt_listed="${p}"
      ;;
  esac
done < <(git -C "${repo_dir}" worktree list --porcelain)
[[ -n "${wt_listed}" ]] || fail "failed to resolve orphan worktree path"

fake_home="${tmp_dir}/home"
mkdir -p "${fake_home}/.aid"
wt_sql="${wt_listed//\'/\'\'}"
sqlite3 "${fake_home}/.aid/aid.db" \
  "CREATE TABLE tasks (id TEXT PRIMARY KEY, worktree_path TEXT, created_at DATETIME);
INSERT INTO tasks (id, worktree_path, created_at) VALUES
  ('t-old', '${wt_sql}', '2026-01-01T00:00:00Z'),
  ('t-new', '${wt_sql}', '2026-06-01T00:00:00Z'),
  ('t-other', '/not/this/worktree', '2026-07-01T00:00:00Z');"

empty_home="${tmp_dir}/empty-home"
mkdir -p "${empty_home}"
sed '$d' "${repo_dir}/scripts/release.sh" > "${repo_dir}/scripts/release-lib.sh"
if (
  HOME="${empty_home}"
  source "${repo_dir}/scripts/release-lib.sh"
  check_orphans
) >"${tmp_dir}/nodb.stdout" 2>"${tmp_dir}/nodb.stderr"; then
  fail "expected check_orphans to fail when sqlite db is missing"
fi
grep -Fq "${wt_listed}" "${tmp_dir}/nodb.stderr" || fail "missing orphan worktree in report without db"
if grep -q 'aid accept' "${tmp_dir}/nodb.stderr"; then
  fail "printed aid accept commands without a sqlite db"
fi

if (
  HOME="${fake_home}"
  source "${repo_dir}/scripts/release-lib.sh"
  check_orphans
) >"${tmp_dir}/ids.stdout" 2>"${tmp_dir}/ids.stderr"; then
  fail "expected check_orphans to fail on orphan worktree"
fi
grep -Fq "${wt_listed}" "${tmp_dir}/ids.stderr" || fail "missing orphan worktree in mapped report"
grep -q 'aid accept t-new' "${tmp_dir}/ids.stderr" || fail "missing aid accept t-new"
grep -q 'aid gc --task t-new' "${tmp_dir}/ids.stderr" || fail "missing aid gc --task t-new"
grep -q 'aid accept t-old' "${tmp_dir}/ids.stderr" || fail "missing aid accept t-old"
grep -q 'aid gc --task t-old' "${tmp_dir}/ids.stderr" || fail "missing aid gc --task t-old"
if grep -q 't-other' "${tmp_dir}/ids.stderr"; then
  fail "reported a task id for a different worktree"
fi
new_line="$(grep -n 'aid accept t-new' "${tmp_dir}/ids.stderr" | head -1 | cut -d: -f1)"
old_line="$(grep -n 'aid accept t-old' "${tmp_dir}/ids.stderr" | head -1 | cut -d: -f1)"
[[ -n "${new_line}" && -n "${old_line}" ]] || fail "could not locate mapped task command lines"
(( new_line < old_line )) || fail "task ids were not ordered by created_at desc"

rm -f "${repo_dir}/scripts/release-lib.sh"

if HOME="${fake_home}" PATH="${bin_dir}:${PATH}" bash "${repo_dir}/scripts/release.sh" --dry-run 1.0.1 "${notes_file}" \
  >"${tmp_dir}/dry-wt.stdout" 2>"${tmp_dir}/dry-wt.stderr"; then
  fail "expected --dry-run to fail on orphan worktree hygiene"
fi
grep -q 'dry-run: would fail: release hygiene check (1 orphan branches, 1 orphan worktrees)' \
  "${tmp_dir}/dry-wt.stderr" || fail "missing dry-run hygiene failure message with worktree: $(tr '\n' '|' < "${tmp_dir}/dry-wt.stderr")"
grep -q 'aid accept t-new' "${tmp_dir}/dry-wt.stderr" || fail "dry-run report missing mapped aid accept"
if grep -q 'would create tag' "${tmp_dir}/dry-wt.stdout" "${tmp_dir}/dry-wt.stderr"; then
  fail "dry-run printed would-create-tag despite worktree hygiene failure"
fi

if HOME="${fake_home}" PATH="${bin_dir}:${PATH}" bash "${repo_dir}/scripts/release.sh" --dry-run --skip-hygiene 1.0.1 "${notes_file}" \
  >"${tmp_dir}/dry-skip.stdout" 2>"${tmp_dir}/dry-skip.stderr"; then
  :
else
  fail "--skip-hygiene should allow --dry-run to proceed"
fi
grep -q 'would create tag' "${tmp_dir}/dry-skip.stdout" || fail "skip-hygiene dry-run did not print would-create-tag"
if grep -q 'dry-run: would fail:' "${tmp_dir}/dry-skip.stdout" "${tmp_dir}/dry-skip.stderr"; then
  fail "skip-hygiene dry-run still reported would-fail"
fi
git -C "${repo_dir}" rev-parse -q --verify refs/tags/v1.0.1 >/dev/null \
  && fail "skip-hygiene dry-run created v1.0.1 tag"

PATH="${bin_dir}:${PATH}" bash "${repo_dir}/scripts/release.sh" --skip-hygiene 1.0.1 "${notes_file}" \
  >"${tmp_dir}/skip.stdout" 2>"${tmp_dir}/skip.stderr" \
  || fail "--skip-hygiene should allow release.sh to proceed"

grep -q 'merged-branch' "${tmp_dir}/skip.stderr" || fail "skip-hygiene run did not report orphan branch"
git -C "${repo_dir}" rev-parse -q --verify refs/tags/v1.0.1 >/dev/null \
  || fail "release.sh did not create v1.0.1 tag during skip-hygiene run"

exit 0
