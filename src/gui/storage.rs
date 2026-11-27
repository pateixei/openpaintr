use std::{
    ffi::OsString,
    fmt, fs,
    io::{self, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Serialize, de::DeserializeOwned};

/// Keeps configuration, state, and user translations in one directory.
const DATA_DIR_VARIABLE: &str = "OPENPAINTR_DATA_DIR";
/// A file with this name beside the executable selects portable mode.
const PORTABLE_MARKER: &str = "portable";
/// The data directory of portable mode, beside the executable.
const PORTABLE_DIRECTORY: &str = "data";

#[derive(Clone, Copy)]
enum UserDirectory {
    Config,
    State,
}

/// The directory that holds every user file, if one replaces the XDG
/// locations: `OPENPAINTR_DATA_DIR`, or `data/` beside a portable executable.
pub fn data_dir(env: impl Fn(&str) -> Option<OsString>, exe_dir: Option<&Path>) -> Option<PathBuf> {
    // Relative values would depend on the working directory.
    env(DATA_DIR_VARIABLE)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            exe_dir
                .filter(|dir| dir.join(PORTABLE_MARKER).is_file())
                .map(|dir| dir.join(PORTABLE_DIRECTORY))
        })
}

fn user_file(
    env: impl Fn(&str) -> Option<OsString>,
    exe_dir: Option<&Path>,
    directory: UserDirectory,
    name: &str,
) -> io::Result<PathBuf> {
    // No fallback: if the data directory is not writable, saving reports it.
    if let Some(data) = data_dir(&env, exe_dir) {
        return Ok(data.join(name));
    }
    let absolute = |variable: &str| {
        env(variable)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let (xdg_variable, home_default) = match directory {
        UserDirectory::Config => ("XDG_CONFIG_HOME", ".config"),
        UserDirectory::State => ("XDG_STATE_HOME", ".local/state"),
    };
    let root = absolute(xdg_variable).or_else(|| {
        #[cfg(windows)]
        if let Some(root) = absolute("APPDATA") {
            return Some(root);
        }
        let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        absolute(home).map(|home| home.join(home_default))
    });
    match root {
        Some(root) => Ok(root.join("openpaintr").join(name)),
        None => exe_dir
            .map(|dir| dir.join(name))
            .ok_or_else(|| io::ErrorKind::NotFound.into()),
    }
}

fn environment(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
}

/// Path of a settings file.
pub fn config_file(name: &str) -> io::Result<PathBuf> {
    user_file(
        environment,
        exe_dir().as_deref(),
        UserDirectory::Config,
        name,
    )
}

/// Path of a file that records state, such as the recent files.
pub fn state_file(name: &str) -> io::Result<PathBuf> {
    user_file(
        environment,
        exe_dir().as_deref(),
        UserDirectory::State,
        name,
    )
}

/// Where a user file lives and where to read it from.
#[derive(Debug)]
pub struct UserFile {
    pub path: PathBuf,
    /// `path`, or the legacy file when it could not be migrated.
    pub read_from: PathBuf,
    pub migration_error: Option<io::Error>,
}

/// Copies `legacy`, a file that earlier versions kept beside the executable, to
/// `path` unless `path` already exists. The legacy file is left untouched.
pub fn migrate(path: PathBuf, legacy: Option<PathBuf>, limit: u64) -> UserFile {
    let mut file = UserFile {
        read_from: path.clone(),
        path,
        migration_error: None,
    };
    // An unreadable destination counts as existing: never replace it.
    if !matches!(file.path.try_exists(), Ok(false)) {
        return file;
    }
    let Some(legacy) = legacy.filter(|legacy| matches!(legacy.try_exists(), Ok(true))) else {
        return file;
    };
    let copied = read_capped(&legacy, limit).and_then(|bytes| {
        ensure_parent_dir(&file.path)?;
        atomic_write_bytes(&file.path, &bytes, false)
    });
    if let Err(error) = copied {
        // Keep the customizations visible in this session.
        file.read_from = legacy;
        file.migration_error = Some(error);
    }
    file
}

pub fn exe_adjacent(name: &str) -> Option<PathBuf> {
    exe_dir().map(|dir| dir.join(name))
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
pub const JSON_LIMIT: u64 = 1024 * 1024;

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

    fn variables<'a>(list: &'a [(&'a str, &'a Path)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        |name| {
            list.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.as_os_str().to_owned())
        }
    }

    #[test]
    fn user_files_follow_xdg_directories() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let exe = temp.path().join("bin");
        let home = temp.path().join("home");
        let config = temp.path().join("config");
        let state = temp.path().join("state");
        let find = |list: &[(&str, &Path)], directory| {
            user_file(variables(list), Some(&exe), directory, "file")
        };

        let xdg = [
            ("HOME", home.as_path()),
            ("XDG_CONFIG_HOME", config.as_path()),
            ("XDG_STATE_HOME", state.as_path()),
        ];
        assert_eq!(
            find(&xdg, UserDirectory::Config)?,
            config.join("openpaintr/file")
        );
        assert_eq!(
            find(&xdg, UserDirectory::State)?,
            state.join("openpaintr/file")
        );
        // Unset or relative XDG variables fall back to the home directory.
        let relative = Path::new("relative");
        for list in [
            vec![("HOME", home.as_path())],
            vec![
                ("HOME", home.as_path()),
                ("XDG_CONFIG_HOME", relative),
                ("XDG_STATE_HOME", relative),
                (DATA_DIR_VARIABLE, relative),
            ],
        ] {
            assert_eq!(
                find(&list, UserDirectory::Config)?,
                home.join(".config/openpaintr/file")
            );
            assert_eq!(
                find(&list, UserDirectory::State)?,
                home.join(".local/state/openpaintr/file")
            );
        }
        Ok(())
    }

    #[test]
    fn portable_mode_and_the_data_directory_variable_hold_every_file() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let exe = temp.path().join("app");
        let custom = temp.path().join("custom");
        let home = temp.path().join("home");
        fs::create_dir(&exe)?;
        let plain = [("HOME", home.as_path())];
        assert_eq!(data_dir(variables(&plain), Some(&exe)), None);

        fs::write(exe.join(PORTABLE_MARKER), "")?;
        let redirected = [
            ("HOME", home.as_path()),
            (DATA_DIR_VARIABLE, custom.as_path()),
        ];
        for (list, base) in [
            (&plain[..], exe.join("data")),
            (&redirected[..], custom.clone()),
        ] {
            assert_eq!(data_dir(variables(list), Some(&exe)), Some(base.clone()));
            for directory in [UserDirectory::Config, UserDirectory::State] {
                let path = user_file(variables(list), Some(&exe), directory, "file")?;
                assert_eq!(path, base.join("file"));
            }
        }
        // The variable works without portable mode too.
        fs::remove_file(exe.join(PORTABLE_MARKER))?;
        assert_eq!(data_dir(variables(&redirected), Some(&exe)), Some(custom));
        Ok(())
    }

    #[test]
    fn migration_copies_old_files_once_and_keeps_them() -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt as _;

        let temp = tempfile::tempdir()?;
        let beside = temp.path().join("bin");
        fs::create_dir(&beside)?;
        let legacy = beside.join("old.json");
        fs::write(&legacy, "old")?;
        // The executable directory may be read-only.
        fs::set_permissions(&beside, fs::Permissions::from_mode(0o500))?;
        let new = temp.path().join("config/openpaintr/new.json");

        let file = migrate(new.clone(), Some(legacy.clone()), 16);
        assert_eq!(file.read_from, new);
        assert!(file.migration_error.is_none());
        assert_eq!(fs::read_to_string(&new)?, "old");
        assert_eq!(fs::read_to_string(&legacy)?, "old");

        // An existing file is never replaced.
        fs::write(&new, "changed")?;
        migrate(new.clone(), Some(legacy.clone()), 16);
        assert_eq!(fs::read_to_string(&new)?, "changed");

        // Nothing to migrate.
        let absent = temp.path().join("absent.json");
        let file = migrate(absent.clone(), Some(beside.join("none")), 16);
        assert_eq!(file.read_from, absent);
        assert!(file.migration_error.is_none() && !absent.exists());

        // When the copy fails, the old file is still read.
        let blocked = beside.join("data/new.json");
        for file in [
            migrate(blocked, Some(legacy.clone()), 16),
            migrate(absent.clone(), Some(legacy.clone()), 2),
        ] {
            assert_eq!(file.read_from, legacy);
            assert!(file.migration_error.is_some());
        }
        assert!(!absent.exists());
        fs::set_permissions(&beside, fs::Permissions::from_mode(0o700))?;
        Ok(())
    }

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
