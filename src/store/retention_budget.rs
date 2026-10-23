// Retention deadlines for SQLite admission, query execution, and JSON parsing.
// Keeps connection guards scoped to one operation; never waits on the Store mutex.
use super::Store;
use anyhow::Result;
use rusqlite::{Connection, ffi};
use std::{
    io::Read,
    sync::TryLockError,
    time::{Duration, Instant},
};

pub(super) struct Deadline {
    end: Instant,
    #[cfg(test)]
    pub(super) before_parse: Option<Box<dyn Fn()>>,
}

impl Deadline {
    pub(super) fn new(budget: Duration) -> Self {
        Self {
            end: Instant::now() + budget,
            #[cfg(test)]
            before_parse: None,
        }
    }

    pub(super) fn expired(&self) -> bool {
        Instant::now() >= self.end
    }

    pub(super) fn parse(&self, raw: &str) -> Option<serde_json::Value> {
        #[cfg(test)]
        if let Some(callback) = &self.before_parse {
            callback();
        }
        if self.expired() {
            return None;
        }
        serde_json::from_reader(TimedReader {
            remaining: raw.as_bytes(),
            deadline: self,
            until_check: 0,
        })
        .ok()
    }

    pub(super) fn key(&self, raw: &str) -> Option<String> {
        serde_json::from_reader(TimedReader {
            remaining: raw.as_bytes(),
            deadline: self,
            until_check: 0,
        })
        .ok()
    }

    pub(super) fn utf8(&self, bytes: Vec<u8>) -> Option<String> {
        let mut start = 0;
        while start < bytes.len() {
            if self.expired() {
                return None;
            }
            let end = (start + 4096).min(bytes.len());
            match std::str::from_utf8(&bytes[start..end]) {
                Ok(_) => start = end,
                Err(error) if error.error_len().is_none() && end < bytes.len() => {
                    start += error.valid_up_to();
                }
                Err(_) => return None,
            }
        }
        // Every byte was validated in bounded chunks, including split UTF-8 characters.
        Some(unsafe { String::from_utf8_unchecked(bytes) })
    }

    pub(super) fn copy(&self, output: &mut String, text: &str) -> Option<()> {
        let mut offset = 0;
        while offset < text.len() {
            if self.expired() {
                return None;
            }
            let end = text.floor_char_boundary((offset + 4096).min(text.len()));
            output.push_str(&text[offset..end]);
            offset = end;
        }
        Some(())
    }
}

pub(super) fn with_conn<T>(
    store: &Store,
    deadline: &Deadline,
    operation: impl FnOnce(&Connection) -> Result<T>,
) -> Result<Option<T>> {
    if deadline.expired() {
        return Ok(None);
    }
    let conn = match store.conn.try_lock() {
        Ok(conn) => conn,
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Poisoned(_)) => anyhow::bail!("Store mutex poisoned during retention"),
    };
    if deadline.expired() {
        return Ok(None);
    }
    let guard = SqlDeadline::install(&conn, deadline)?;
    let result = operation(&conn);
    drop(guard);
    match result {
        Err(error)
            if deadline.expired()
                && error
                    .downcast_ref::<rusqlite::Error>()
                    .and_then(|e| e.sqlite_error_code())
                    == Some(ffi::ErrorCode::OperationInterrupted) =>
        {
            Ok(None)
        }
        other => other.map(Some),
    }
}

struct SqlDeadline<'a> {
    conn: &'a Connection,
    _deadline: &'a Deadline,
    timeout: Duration,
}

impl<'a> SqlDeadline<'a> {
    fn install(conn: &'a Connection, deadline: &'a Deadline) -> Result<Self> {
        let timeout: u64 = conn.query_row("PRAGMA busy_timeout", [], |row| row.get(0))?;
        conn.busy_timeout(Duration::ZERO)?;
        // SQLite calls synchronously while the borrowed deadline and connection are alive.
        unsafe {
            ffi::sqlite3_progress_handler(
                conn.handle(),
                100,
                Some(expired),
                (deadline as *const Deadline).cast_mut().cast(),
            );
        }
        Ok(Self {
            conn,
            _deadline: deadline,
            timeout: Duration::from_millis(timeout),
        })
    }
}

impl Drop for SqlDeadline<'_> {
    fn drop(&mut self) {
        // Clear the callback before its borrowed context can go out of scope.
        unsafe {
            ffi::sqlite3_progress_handler(self.conn.handle(), 0, None, std::ptr::null_mut());
        }
        let _ = self.conn.busy_timeout(self.timeout);
    }
}

unsafe extern "C" fn expired(context: *mut std::ffi::c_void) -> std::ffi::c_int {
    // SqlDeadline keeps this reference alive until the callback is removed.
    i32::from(unsafe { &*context.cast::<Deadline>() }.expired())
}

struct TimedReader<'a> {
    remaining: &'a [u8],
    deadline: &'a Deadline,
    until_check: usize,
}

impl Read for TimedReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.until_check == 0 {
            if self.deadline.expired() {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            self.until_check = 4096;
        }
        let count = buffer.len().min(self.remaining.len()).min(self.until_check);
        buffer[..count].copy_from_slice(&self.remaining[..count]);
        self.remaining = &self.remaining[count..];
        self.until_check -= count;
        Ok(count)
    }
}
