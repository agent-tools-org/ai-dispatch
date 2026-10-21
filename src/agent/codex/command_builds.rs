// Test-only native Codex command construction counter.
// Measures delegated builds without launching a CLI or examining synthetic files.
thread_local! {
    static BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn record() {
    BUILDS.with(|builds| builds.set(builds.get() + 1));
}

pub(crate) fn count() -> usize {
    BUILDS.with(std::cell::Cell::get)
}
