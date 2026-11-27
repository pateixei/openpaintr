use std::{
    fmt, fs,
    io::{self, BufWriter, Read, Write},
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

/// Opens a file for reading without blocking, even when the path is a FIFO.
/// Callers check the open file's type before reading it.
pub fn open_nonblocking(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        // `O_NONBLOCK` on x86-64 and AArch64.
        options.custom_flags(0o4000);
    }
    options.open(path)
}

/// Largest settings, language, shortcut, or recent-file list that is read.
const JSON_LIMIT: u64 = 1024 * 1024;

/// A rejected read, translated only when displayed: catalogs are themselves
/// loaded through [`read_capped`], before any translation is available.
#[derive(Debug)]
struct Rejected(&'static str);

impl fmt::Display for Rejected {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(super::i18n::text(self.0))
    }
}

impl std::error::Error for Rejected {}

/// Reads a regular file of at most `max` bytes.
pub fn read_capped(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    let file = open_nonblocking(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            Rejected("error.not-a-regular-file"),
        ));
    }
    let mut bytes = Vec::new();
    // Read one byte past the cap: the file can grow after it is opened.
    file.take(max.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            Rejected("error.file-too-large"),
        ));
    }
    Ok(bytes)
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
    let bytes = read_capped(path, JSON_LIMIT)?;
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

pub fn write_json_atomic<T: Serialize>(
    path: &Path,
    value: &T,
    options: JsonWrite,
) -> io::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_are_capped_and_limited_to_regular_files() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("data");
        fs::write(&path, [b'a'; 8])?;
        assert_eq!(read_capped(&path, 8)?, [b'a'; 8]);
        let error = read_capped(&path, 7).expect_err("oversized");
        assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);
        assert_eq!(error.to_string(), "The file is too large.");

        let pipe = temp.path().join("pipe");
        let status = std::process::Command::new("mkfifo").arg(&pipe).status()?;
        assert!(status.success());
        // A device that never ends must not be read either.
        for path in [pipe.as_path(), temp.path(), Path::new("/dev/zero")] {
            let error = read_capped(path, 8).expect_err("not a regular file");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{path:?}");
            assert_eq!(error.to_string(), "The path is not a regular file.");
        }
        Ok(())
    }

    #[test]
    fn json_files_over_the_limit_are_rejected() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("data.json");
        let json = |size: u64| format!("\"{}\"", "a".repeat(size as usize - 2));
        fs::write(&path, json(JSON_LIMIT))?;
        assert!(read_json::<String>(&path).is_ok());
        fs::write(&path, json(JSON_LIMIT + 1))?;
        let error = read_json::<String>(&path).expect_err("oversized");
        assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);
        Ok(())
    }
}
