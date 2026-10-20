// Thread-local Qwen HOME injection used by model-catalog tests.
// Exports: set_test_qwen_home(), qwen_home_override().
// Deps: std::cell::RefCell, std::path::PathBuf.

use std::cell::RefCell;
use std::path::PathBuf;

thread_local! {
    static TEST_QWEN_HOME: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

pub(crate) fn set_test_qwen_home(home: Option<PathBuf>) {
    TEST_QWEN_HOME.with(|cell| *cell.borrow_mut() = home);
}

pub(crate) fn qwen_home_override() -> Option<PathBuf> {
    TEST_QWEN_HOME.with(|cell| cell.borrow().clone())
}
