// Thread-local fake keychain for API-route launch tests.
// Real security invocation and secret representation are unchanged in production.
use super::*;
use std::cell::RefCell;
thread_local! {
    static FAKE: RefCell<Option<(KeychainItem, bool)>> = const { RefCell::new(None) };
}

pub(crate) struct Guard;
impl Guard {
    pub(crate) fn set(item: KeychainItem, readable: bool) -> Self {
        FAKE.with(|fake| *fake.borrow_mut() = Some((item, readable)));
        Self
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        FAKE.with(|fake| *fake.borrow_mut() = None);
    }
}

pub(super) fn read(item: &KeychainItem) -> Option<Result<ApiKey>> {
    FAKE.with(|fake| {
        fake.borrow().as_ref().map(|(expected, readable)| {
            if item != expected || !readable {
                bail!("keychain item {}/{} is not readable", item.service, item.account);
            }
            Ok(ApiKey("synthetic-route-test-value".into()))
        })
    })
}
