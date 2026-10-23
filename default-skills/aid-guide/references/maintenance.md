# Maintenance and historical output

```bash
aid clean --dry-run
aid clean --older-than 30
aid clean --vacuum
```

Run cleanup in dry-run mode first. `clean` retains task records, event detail,
and custody evidence and does not replace `aid gc --task`. It replaces output
copies of at least 4 KiB in old terminal tasks with an "output trimmed" marker.
Only successful command output stored in `metadata.full` is eligible; commands,
provider stdout, accounting, routing, quota, completion, and delivery evidence
remain. Task logs, transcripts, and output files remain available.

`aid clean --older-than 30` uses 30 days since task completion (default: 7).
Trimming commits bounded batches and stops after about 10 seconds; repeat the
same command to continue. `--dry-run` reports eligible rows and payload bytes
without writing. If its scan reaches the budget, the estimate is incomplete.
After task settlement and backup, automatic trimming runs at most daily with a
2-second budget and the default 7-day window. Cadence and cursors share the
existing `update-check.json` state file.

`aid clean --vacuum` compacts after trimming and reports database size before and
after. It refuses while any task is running, waiting, pending, awaiting input, or
stalled, and checks for temporary free disk space up to twice the database size.
VACUUM holds an exclusive database lock; run it during an idle period. Compaction
is never automatic. `--vacuum --dry-run` only describes the compaction.
