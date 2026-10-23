// Incremental reads for oversized event metadata, with no long-lived blob handle.
// Every 64 KiB chunk releases the Store mutex and checks the shared deadline.
use super::{
    Candidate, Store,
    budget::{Deadline, with_conn},
};
use anyhow::Result;
use rusqlite::{Connection, ffi};

pub(super) const CHUNK_BYTES: usize = 64 * 1024;

pub(super) fn load(store: &Store, row: &mut Candidate, deadline: &Deadline) -> Result<bool> {
    if row.metadata.is_some() || !row.eligible || row.metadata_bytes == 0 {
        return Ok(!deadline.expired());
    }
    let mut bytes = Vec::new();
    while bytes.len() < row.metadata_bytes {
        let offset = bytes.len();
        let mut chunk = vec![0; CHUNK_BYTES.min(row.metadata_bytes - offset)];
        let result = with_conn(store, deadline, |conn| {
            read_chunk(conn, row, offset, &mut chunk)
        })?;
        if result != Some(true) {
            return Ok(false);
        }
        bytes.extend_from_slice(&chunk);
    }
    row.metadata = deadline.utf8(bytes);
    Ok(!deadline.expired())
}

fn read_chunk(
    conn: &Connection,
    row: &Candidate,
    offset: usize,
    buffer: &mut [u8],
) -> Result<bool> {
    let mut blob = std::ptr::null_mut();
    // The read-only blob is closed before returning, including on size/read errors.
    let opened = unsafe {
        ffi::sqlite3_blob_open(
            conn.handle(),
            c"main".as_ptr(),
            c"events".as_ptr(),
            c"metadata".as_ptr(),
            row.id,
            0,
            &mut blob,
        )
    };
    if opened != ffi::SQLITE_OK {
        return Err(sqlite_error(opened).into());
    }
    let same_size = unsafe { ffi::sqlite3_blob_bytes(blob) } as usize == row.metadata_bytes;
    let result = if same_size {
        unsafe {
            ffi::sqlite3_blob_read(
                blob,
                buffer.as_mut_ptr().cast(),
                buffer.len() as i32,
                offset as i32,
            )
        }
    } else {
        ffi::SQLITE_OK
    };
    let closed = unsafe { ffi::sqlite3_blob_close(blob) };
    if result != ffi::SQLITE_OK {
        return Err(sqlite_error(result).into());
    }
    if closed != ffi::SQLITE_OK {
        return Err(sqlite_error(closed).into());
    }
    Ok(same_size)
}

fn sqlite_error(code: i32) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(ffi::Error::new(code), None)
}
