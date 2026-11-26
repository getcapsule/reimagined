use std::path::{Path, PathBuf};

#[cfg(windows)]
pub(super) fn encode(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(windows)]
pub(super) fn decode(bytes: Vec<u8>) -> rusqlite::Result<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    if !bytes.len().is_multiple_of(2) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let wide: Vec<_> = bytes
        .chunks_exact(2)
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .collect();
    Ok(OsString::from_wide(&wide).into())
}

#[cfg(unix)]
pub(super) fn encode(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
pub(super) fn decode(bytes: Vec<u8>) -> rusqlite::Result<PathBuf> {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    Ok(OsString::from_vec(bytes).into())
}
