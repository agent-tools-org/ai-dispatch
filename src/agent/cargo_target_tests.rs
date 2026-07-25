// Tests for cargo target seed mechanics.
// Exports: none. Deps: cargo target helpers and tempfile.

use super::*;

#[test]
fn temp_seed_target_is_unique_within_process() {
    let target = PathBuf::from("/tmp/cache/feat-shared");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let barrier = std::sync::Arc::clone(&barrier);
        let target = target.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            temp_seed_target(&target)
        }));
    }

    let mut paths = handles
        .into_iter()
        .map(|handle| handle.join().expect("thread should return temp seed target"))
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    assert_eq!(paths.len(), 8);
}

#[test]
fn seed_skips_when_clone_probe_is_unavailable() {
    let probe_guard = CloneSeedGuard::unavailable("clone unavailable: forced");
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let source = temp.path().join("_base");
    let target = temp.path().join("feat-cache");
    std::fs::create_dir_all(&source).expect("source dir should be created");

    let outcome = seed_branch_target_from_source(&source, &target);

    assert_eq!(
        outcome,
        BranchTargetSeedOutcome::Skipped {
            target: target.to_string_lossy().into_owned(),
            reason: "clone unavailable: forced".to_string(),
        }
    );
    assert!(!target.exists());
    drop(probe_guard);
}
