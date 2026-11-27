use std::{
    fs,
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::{Serialize, de::DeserializeOwned};

pub fn config_file(name: &str) -> io::Result<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Ok(xdg.join("openpaintr").join(name));
    }

    #[cfg(windows)]
    let config_root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(|path| PathBuf::from(path).join(".config"))
                .filter(|path| path.is_absolute())
        });

    #[cfg(not(windows))]
    let config_root = std::env::var_os("HOME")
        .map(|path| PathBuf::from(path).join(".config"))
        .filter(|path| path.is_absolute());

    if let Some(root) = config_root {
        return Ok(root.join("openpaintr").join(name));
    }

    exe_adjacent(name)
}

pub fn exe_adjacent(name: &str) -> io::Result<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .ok_or_else(|| io::ErrorKind::NotFound.into())
}

pub fn ensure_parent_dir(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn atomic_write_bytes(path: &Path, bytes: &[u8], replace: bool) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let mut file = tempfile::NamedTempFile::new_in(parent.unwrap_or(Path::new(".")))?;
    {
        let mut output = BufWriter::new(file.as_file_mut());
        output.write_all(bytes)?;
        output.flush()?;
    }
    file.as_file().sync_all()?;
    if replace {
        file.persist(path)?;
    } else {
        file.persist_noclobber(path)?;
    }
    Ok(())
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

/// Output options for [`write_json_atomic`].
#[derive(Clone, Copy, Debug)]
pub struct JsonWrite {
    pub pretty: bool,
    pub trailing_newline: bool,
    /// Overwrite an existing file; when false the write fails if it exists.
    pub replace: bool,
}

impl JsonWrite {
    /// Indented JSON that replaces any existing file.
    pub const PRETTY: Self = Self {
        pretty: true,
        trailing_newline: false,
        replace: true,
    };
    /// Single-line JSON that replaces any existing file.
    pub const COMPACT: Self = Self {
        pretty: false,
        ..Self::PRETTY
    };
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T, options: JsonWrite) -> io::Result<()> {
    ensure_parent_dir(path)?;
    let mut bytes = if options.pretty {
        serde_json::to_vec_pretty(value)
    } else {
        serde_json::to_vec(value)
    }
    .map_err(io::Error::other)?;
    if options.trailing_newline {
        bytes.push(b'\n');
    }
    atomic_write_bytes(path, &bytes, options.replace)
}
