use std::{
    borrow::Cow,
    cell::Cell,
    collections::BTreeMap,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use eframe::egui::{FontData, FontDefinitions};
use unicode_bidi::BidiInfo;

const ENGLISH: &str = "en-us";
const BUILTIN: [(&str, &str); 10] = [
    (ENGLISH, include_str!("../../translations/en-us.json")),
    ("fr-fr", include_str!("../../translations/fr-fr.json")),
    ("es-es", include_str!("../../translations/es-es.json")),
    ("de-de", include_str!("../../translations/de-de.json")),
    ("pt-br", include_str!("../../translations/pt-br.json")),
    ("it-it", include_str!("../../translations/it-it.json")),
    ("ru-ru", include_str!("../../translations/ru-ru.json")),
    ("ar-sa", include_str!("../../translations/ar-sa.json")),
    ("zh-cn", include_str!("../../translations/zh-cn.json")),
    ("ja-jp", include_str!("../../translations/ja-jp.json")),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Locale(&'static str);

type Catalog = BTreeMap<String, String>;

struct Catalogs(BTreeMap<String, Catalog>);

static CATALOGS: LazyLock<Catalogs> = LazyLock::new(|| {
    let mut catalogs = Catalogs::bundled();
    let env = |name: &str| std::env::var_os(name);
    let exe_dir = super::storage::exe_dir();
    let data_dir = super::storage::data_dir(env, exe_dir.as_deref());
    let beside_executable = exe_dir.map(|dir| dir.join("translations"));
    for directory in search_directories(env, beside_executable, data_dir) {
        catalogs.scan(&directory);
    }
    catalogs
});

/// Largest translation file that is loaded.
const CATALOG_LIMIT: u64 = 1024 * 1024;

/// Directories that may override or add catalogs, later ones winning: system
/// data directories, `translations/` beside the executable, then the user's
/// data directory, which `data_dir` replaces when set. The working directory is
/// never searched, so relative paths in the environment are ignored.
fn search_directories(
    env: impl Fn(&str) -> Option<OsString>,
    beside_executable: Option<PathBuf>,
    data_dir: Option<PathBuf>,
) -> Vec<PathBuf> {
    let absolute = |path: PathBuf| path.is_absolute().then_some(path);
    let translations = |base: PathBuf| base.join("openpaintr").join("translations");

    let mut system: Vec<PathBuf> = env("XDG_DATA_DIRS").map_or_else(Vec::new, |list| {
        std::env::split_paths(&list).filter_map(absolute).collect()
    });
    if system.is_empty() {
        system = ["/usr/local/share", "/usr/share"].map(PathBuf::from).into();
    }
    // XDG lists the most important directory first.
    let mut directories: Vec<_> = system.into_iter().rev().map(translations).collect();

    directories.extend(beside_executable);

    if let Some(data) = data_dir {
        directories.push(data.join("translations"));
        return directories;
    }
    let user = env("XDG_DATA_HOME")
        .map(PathBuf::from)
        .and_then(absolute)
        .or_else(|| {
            env("HOME")
                .map(PathBuf::from)
                .and_then(absolute)
                .map(|home| home.join(".local").join("share"))
        });
    directories.extend(user.map(translations));
    directories
}

static VISUAL_LABELS: LazyLock<BTreeMap<&'static str, String>> = LazyLock::new(|| {
    CATALOGS
        .0
        .values()
        .flat_map(Catalog::values)
        .filter_map(|value| match display(value) {
            Cow::Owned(visual) => Some((value.as_str(), visual)),
            Cow::Borrowed(_) => None,
        })
        .collect()
});

thread_local! {
    static ACTIVE_LOCALE: Cell<Locale> = const { Cell::new(Locale(ENGLISH)) };
}

impl Catalogs {
    fn bundled() -> Self {
        Self(
            BUILTIN
                .into_iter()
                .map(|(code, data)| {
                    (
                        code.to_owned(),
                        serde_json::from_str(data).expect("valid bundled catalog"),
                    )
                })
                .collect(),
        )
    }

    fn scan(&mut self, directory: &Path) {
        let Ok(files) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in files.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Some(code) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|code| valid_code(code))
            else {
                continue;
            };
            let Ok(data) = super::storage::read_capped(&path, CATALOG_LIMIT) else {
                continue;
            };
            let Ok(mut catalog) = serde_json::from_slice::<Catalog>(&data) else {
                continue;
            };
            catalog.retain(|key, value| {
                !value.trim().is_empty()
                    && self.0[ENGLISH]
                        .get(key)
                        .is_some_and(|base| placeholders(value) == placeholders(base))
            });
            if !catalog.is_empty() {
                self.0.entry(code.to_owned()).or_default().extend(catalog);
            }
        }
    }

    fn code(&self, code: &str) -> Option<&str> {
        self.0.get_key_value(code).map(|(code, _)| code.as_str())
    }

    fn system_code(&self, name: &str) -> &str {
        let tag = name
            .split(['.', '@'])
            .next()
            .unwrap_or("")
            .replace('_', "-")
            .to_ascii_lowercase();
        if let Some(code) = self.code(&tag) {
            return code;
        }
        let language = tag.split('-').next().unwrap_or("");
        self.code(language)
            .or_else(|| {
                BUILTIN
                    .iter()
                    .find(|(code, _)| code.split('-').next() == Some(language))
                    .map(|(code, _)| *code)
            })
            .or_else(|| {
                self.0
                    .keys()
                    .find(|code| code.split('-').next() == Some(language))
                    .map(String::as_str)
            })
            .unwrap_or(ENGLISH)
    }

    fn lookup<'a>(&'a self, locale: &str, key: &'a str) -> &'a str {
        self.0
            .get(locale)
            .and_then(|catalog| catalog.get(key))
            .or_else(|| self.0[ENGLISH].get(key))
            .map_or(key, String::as_str)
    }
}

fn valid_code(code: &str) -> bool {
    let mut parts = code.split('-');
    parts.next().is_some_and(|language| {
        (2..=3).contains(&language.len()) && language.bytes().all(|byte| byte.is_ascii_lowercase())
    }) && parts.all(|part| {
        (2..=8).contains(&part.len())
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    })
}

fn placeholders(message: &str) -> Vec<usize> {
    let mut slots: Vec<_> = message
        .split('{')
        .skip(1)
        .map(|part| {
            part.split_once('}')
                .and_then(|(slot, _)| slot.parse().ok())
                .unwrap_or(usize::MAX)
        })
        .collect();
    slots.sort_unstable();
    slots
}

impl Default for Locale {
    fn default() -> Self {
        Self(ENGLISH)
    }
}

impl Locale {
    pub fn all() -> impl Iterator<Item = Self> {
        CATALOGS.0.keys().map(|code| Self(code))
    }

    pub fn name(self) -> &'static str {
        let name = CATALOGS.0[self.0]
            .get("language-name")
            .map_or(self.0, String::as_str);
        visual_label(name)
    }

    #[cfg_attr(test, allow(dead_code))]
    pub fn prefers_simplified_chinese(self) -> bool {
        self.0 == "zh" || self.0.starts_with("zh-hans") || self.0 == "zh-cn"
    }

    pub fn load() -> Self {
        if let Ok(path) = Self::settings_path()
            && let Ok(code) = Self::load_at(&path)
            && let Some(code) = CATALOGS.code(&code)
        {
            return Self(code);
        }
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|variable| std::env::var(variable).ok().filter(|name| !name.is_empty()))
            .map_or_else(Self::default, |name| Self(CATALOGS.system_code(&name)))
    }

    fn settings_path() -> io::Result<PathBuf> {
        super::storage::config_file("language.json")
    }

    pub fn save(self) -> io::Result<()> {
        self.save_at(&Self::settings_path()?)
    }

    fn load_at(path: &Path) -> io::Result<String> {
        super::storage::read_json(path)
    }

    fn save_at(self, path: &Path) -> io::Result<()> {
        super::storage::write_json_atomic(path, &self.0, super::storage::JsonWrite::COMPACT)
    }

    // Render helpers use the active frame's locale, not a shared app preference.
    pub fn with<R>(self, render: impl FnOnce() -> R) -> R {
        struct Restore(Locale);

        impl Drop for Restore {
            fn drop(&mut self) {
                ACTIVE_LOCALE.set(self.0);
            }
        }

        let _restore = Restore(ACTIVE_LOCALE.replace(self));
        render()
    }
}

pub fn text(key: &str) -> &str {
    visual_label(logical_text(key))
}

pub fn logical_text(key: &str) -> &str {
    CATALOGS.lookup(ACTIVE_LOCALE.get().0, key)
}

fn visual_label(label: &str) -> &str {
    VISUAL_LABELS.get(label).map_or(label, String::as_str)
}

pub fn format(key: &str, args: &[&dyn std::fmt::Display]) -> String {
    display(&format_logical(key, args)).into_owned()
}

pub fn format_logical(key: &str, args: &[&dyn std::fmt::Display]) -> String {
    let template = logical_text(key);
    let mut rendered = String::with_capacity(template.len());
    let mut remaining = template;
    while let Some((prefix, rest)) = remaining.split_once('{') {
        rendered.push_str(prefix);
        if let Some((slot, tail)) = rest.split_once('}')
            && let Ok(index) = slot.parse::<usize>()
            && let Some(value) = args.get(index)
        {
            use std::fmt::Write as _;
            let _ = write!(rendered, "{value}");
            remaining = tail;
        } else {
            rendered.push('{');
            remaining = rest;
        }
    }
    rendered.push_str(remaining);
    rendered
}

/// egui 0.33 lays glyphs out in storage order. Shape and reorder a copy for display.
pub fn display(input: &str) -> Cow<'_, str> {
    let has_arabic = input.chars().any(|c| matches!(c, '\u{0600}'..='\u{08FF}'));
    if !has_arabic && !input.chars().any(|c| matches!(c, '\u{0590}'..='\u{05FF}')) {
        return Cow::Borrowed(input);
    }
    let logical = if has_arabic {
        Cow::Owned(arabic_reshaper::arabic_reshape(input))
    } else {
        Cow::Borrowed(input)
    };
    let mut visual = String::with_capacity(input.len());
    for (index, line) in logical.split('\n').enumerate() {
        if index > 0 {
            visual.push('\n');
        }
        let bidi = BidiInfo::new(line, None);
        for paragraph in &bidi.paragraphs {
            let (levels, runs) = bidi.visual_runs(paragraph, paragraph.range.clone());
            for run in runs {
                if levels[run.start].is_rtl() {
                    visual.extend(line[run].chars().rev().map(mirror_bracket));
                } else {
                    visual.push_str(&line[run]);
                }
            }
        }
    }
    Cow::Owned(visual)
}

fn mirror_bracket(character: char) -> char {
    match character {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        other => other,
    }
}

#[cfg_attr(test, allow(dead_code))]
pub fn fonts(locale: Locale) -> FontDefinitions {
    const ARABIC: (&str, &[u8]) = (
        "noto-sans-arabic",
        include_bytes!("../../assets/fonts/NotoSansArabic-Regular.ttf"),
    );
    const CHINESE: (&str, &[u8]) = (
        "noto-sans-sc",
        include_bytes!("../../assets/fonts/NotoSansSC-Regular.ttf"),
    );
    const JAPANESE: (&str, &[u8]) = (
        "noto-sans-jp",
        include_bytes!("../../assets/fonts/NotoSansJP-Regular.ttf"),
    );
    let mut definitions = FontDefinitions::default();
    let ordered = if locale.prefers_simplified_chinese() {
        [ARABIC, CHINESE, JAPANESE]
    } else {
        [ARABIC, JAPANESE, CHINESE]
    };
    for (name, data) in ordered {
        definitions
            .font_data
            .insert(name.to_owned(), FontData::from_static(data).into());
        for family in definitions.families.values_mut() {
            family.push(name.to_owned());
        }
    }
    definitions
}

pub fn blend_name(index: u8) -> &'static str {
    visual_label(blend_name_logical(index))
}

pub fn blend_name_logical(index: u8) -> &'static str {
    let keys = &openpaintr::BLEND_MODE_KEYS;
    logical_text(keys.get(usize::from(index)).copied().unwrap_or(keys[0]))
}

pub fn error_message(error: &io::Error) -> String {
    error
        .get_ref()
        .and_then(|source| source.downcast_ref::<openpaintr::Diagnostic>())
        .map_or_else(
            || error.to_string(),
            |diagnostic| text(diagnostic.key()).to_owned(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn describe_placeholders(message: &str) -> String {
        let slots: Vec<_> = placeholders(message)
            .into_iter()
            .map(|slot| match slot {
                usize::MAX => "{invalid}".to_owned(),
                slot => format!("{{{slot}}}"),
            })
            .collect();
        format!("[{}]", slots.join(", "))
    }

    #[test]
    fn bundled_catalogs_have_all_keys_and_matching_parameters() {
        let catalogs: BTreeMap<&str, Catalog> = BUILTIN
            .into_iter()
            .map(|(code, data)| {
                let catalog = serde_json::from_str(data)
                    .unwrap_or_else(|error| panic!("{code}.json: invalid catalog: {error}"));
                (code, catalog)
            })
            .collect();
        assert_eq!(catalogs.len(), BUILTIN.len(), "repeated catalog code");
        let english = &catalogs[ENGLISH];
        assert!(!english.is_empty(), "{ENGLISH}.json: no keys");

        let mut problems = Vec::new();
        for (code, catalog) in &catalogs {
            let mut report = |key: &str, problem: String| {
                problems.push(format!("{code}.json: key \"{key}\": {problem}"));
            };
            for key in english.keys().filter(|key| !catalog.contains_key(*key)) {
                report(key, "missing".to_owned());
            }
            for (key, message) in catalog {
                let Some(base) = english.get(key) else {
                    report(key, format!("not in {ENGLISH}.json"));
                    continue;
                };
                if message.trim().is_empty() {
                    report(key, "empty value".to_owned());
                    continue;
                }
                let slots = placeholders(message);
                if slots != placeholders(base) || slots.contains(&usize::MAX) {
                    report(
                        key,
                        format!(
                            "placeholders {} do not match {} in {ENGLISH}.json",
                            describe_placeholders(message),
                            describe_placeholders(base),
                        ),
                    );
                }
            }
        }
        assert!(
            problems.is_empty(),
            "translation catalog problems:\n{}",
            problems.join("\n")
        );
    }

    #[test]
    fn bundled_fonts_cover_the_four_additional_scripts() {
        use eframe::egui::{Context, FontFamily, FontId, RawInput};

        let context = Context::default();
        for code in ["ru-ru", "ar-sa", "zh-cn", "ja-jp"] {
            let locale = Locale(code);
            context.set_fonts(fonts(locale));
            let _ = context.run(RawInput::default(), |context| {
                context.fonts_mut(|fonts| {
                    let font = FontId::new(13.0, FontFamily::Proportional);
                    for (key, message) in &CATALOGS.0[code] {
                        let message = display(message);
                        for character in message.chars().filter(|c| !c.is_control()) {
                            assert!(
                                fonts.has_glyph(&font, character),
                                "{code} {key}: missing glyph U+{:04X}",
                                character as u32,
                            );
                        }
                    }
                });
            });
        }
    }

    #[test]
    fn arabic_shaping_preserves_shortcuts_and_line_breaks() {
        let logical = "حفظ (Ctrl+Shift+B)\nالمستند";
        let visual = display(logical);
        assert_ne!(visual, logical);
        assert!(visual.contains("(Ctrl+Shift+B)"));
        assert_eq!(visual.lines().count(), 2);
        assert!(visual.chars().any(|c| matches!(c, '\u{FE70}'..='\u{FEFF}')));
        Locale("ar-sa").with(|| {
            let title = format_logical("layer-number", &[&3]);
            assert_eq!(title, "طبقة 3");
            assert_ne!(display(&title), title);
        });
    }

    #[test]
    fn discovers_new_catalogs_and_falls_back_per_message() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        std::fs::write(
            temp.path().join("nl-nl.json"),
            r#"{"language-name":"Nederlands","file":"Nederlandse bestanden","save-changes":"Wrong placeholder {1}"}"#,
        )?;
        std::fs::write(
            temp.path().join("invalid_name.json"),
            r#"{"file":"Ignored"}"#,
        )?;
        std::fs::write(temp.path().join("tr-tr.json"), b"invalid JSON")?;
        std::fs::write(
            temp.path().join("fr-fr.json"),
            r#"{"file":"Fichier personnalisé"}"#,
        )?;
        let mut catalogs = Catalogs::bundled();
        catalogs.scan(temp.path());
        assert_eq!(catalogs.system_code("nl_NL.UTF-8"), "nl-nl");
        assert_eq!(catalogs.system_code("pt_BR.UTF-8"), "pt-br");
        assert_eq!(catalogs.system_code("es_MX"), "es-es");
        assert_eq!(catalogs.system_code("C"), ENGLISH);
        assert_eq!(catalogs.lookup("nl-nl", "file"), "Nederlandse bestanden");
        assert_eq!(
            catalogs.lookup("nl-nl", "new"),
            catalogs.lookup(ENGLISH, "new")
        );
        assert_eq!(
            catalogs.lookup("nl-nl", "save-changes"),
            catalogs.lookup(ENGLISH, "save-changes")
        );
        assert_eq!(catalogs.lookup("fr-fr", "file"), "Fichier personnalisé");
        assert_eq!(catalogs.lookup("fr-fr", "new"), "Nouveau");
        assert!(catalogs.code("tr-tr").is_none());
        assert!(catalogs.code("invalid_name").is_none());
        Ok(())
    }

    fn environment<'a>(
        variables: &'a [(&'a str, &'a str)],
    ) -> impl Fn(&str) -> Option<OsString> + 'a {
        |name| {
            variables
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.into())
        }
    }

    #[test]
    fn searches_only_data_directories_and_the_executable_directory() {
        let paths = |list: &[&str]| list.iter().map(PathBuf::from).collect::<Vec<_>>();
        assert_eq!(
            search_directories(
                environment(&[("HOME", "/home/user")]),
                Some("/opt/app/translations".into()),
                None,
            ),
            paths(&[
                "/usr/share/openpaintr/translations",
                "/usr/local/share/openpaintr/translations",
                "/opt/app/translations",
                "/home/user/.local/share/openpaintr/translations",
            ]),
        );
        assert_eq!(
            search_directories(
                environment(&[
                    ("XDG_DATA_DIRS", "/first:relative::/second"),
                    ("XDG_DATA_HOME", "/data"),
                    ("HOME", "/home/user"),
                ]),
                None,
                None,
            ),
            paths(&[
                "/second/openpaintr/translations",
                "/first/openpaintr/translations",
                "/data/openpaintr/translations",
            ]),
        );
        // Relative values would reach into the working directory.
        let relative = search_directories(
            environment(&[
                ("XDG_DATA_DIRS", ".:here"),
                ("XDG_DATA_HOME", "."),
                ("HOME", "."),
            ]),
            None,
            None,
        );
        assert!(relative.iter().all(|path| path.starts_with("/usr")));
        // A data directory replaces the user's one.
        let portable = search_directories(
            environment(&[("HOME", "/home/user")]),
            None,
            Some("/media/app/data".into()),
        );
        assert_eq!(
            portable.last(),
            Some(&PathBuf::from("/media/app/data/translations"))
        );
        assert!(!portable.iter().any(|path| path.starts_with("/home")));
    }

    #[test]
    fn later_search_directories_override_earlier_ones() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_str().expect("UTF-8 temporary path");
        let system = format!("{root}/high:{root}/low");
        let user = format!("{root}/user");
        let beside = temp.path().join("beside");
        let directories = search_directories(
            environment(&[("XDG_DATA_DIRS", &system), ("XDG_DATA_HOME", &user)]),
            Some(beside),
            None,
        );
        // Each directory defines its own key and every key of the ones before it.
        let keys = ["file", "new", "open", "save"];
        for (rank, directory) in directories.iter().enumerate() {
            std::fs::create_dir_all(directory)?;
            let entries: Vec<_> = keys[rank..]
                .iter()
                .map(|key| format!(r#""{key}":"rank {rank}""#))
                .collect();
            std::fs::write(
                directory.join("pt-br.json"),
                format!("{{{}}}", entries.join(",")),
            )?;
        }
        let mut catalogs = Catalogs::bundled();
        for directory in &directories {
            catalogs.scan(directory);
        }
        // low < high < beside the executable < user.
        assert!(directories[0].starts_with(temp.path().join("low")));
        assert!(directories[3].starts_with(&user));
        for (rank, key) in keys.iter().enumerate() {
            assert_eq!(catalogs.lookup("pt-br", key), format!("rank {rank}"));
        }
        Ok(())
    }

    #[test]
    fn skips_oversized_and_non_regular_catalogs_without_blocking() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let catalog = r#"{"file":"Loaded"}"#;
        let padded = |size: u64| {
            let padding = " ".repeat(size as usize - catalog.len());
            format!("{catalog}{padding}")
        };
        std::fs::write(temp.path().join("aa-aa.json"), padded(CATALOG_LIMIT))?;
        std::fs::write(temp.path().join("bb-bb.json"), padded(CATALOG_LIMIT + 1))?;
        std::fs::create_dir(temp.path().join("cc-cc.json"))?;
        let status = std::process::Command::new("mkfifo")
            .arg(temp.path().join("xx-xx.json"))
            .status()?;
        assert!(status.success());

        let mut catalogs = Catalogs::bundled();
        catalogs.scan(temp.path());
        assert_eq!(catalogs.lookup("aa-aa", "file"), "Loaded");
        for code in ["bb-bb", "cc-cc", "xx-xx"] {
            assert!(catalogs.code(code).is_none(), "{code}");
        }
        Ok(())
    }

    #[test]
    fn saves_and_replaces_a_locale_preference() -> io::Result<()> {
        let temp = tempfile::tempdir()?;
        let preference = temp.path().join("openpaintr").join("language.json");
        assert!(Locale::load_at(&preference).is_err());
        for (code, _) in BUILTIN {
            Locale(code).save_at(&preference)?;
            assert_eq!(Locale::load_at(&preference)?, code);
        }
        std::fs::write(&preference, b"invalid JSON")?;
        assert!(Locale::load_at(&preference).is_err());
        Ok(())
    }
}
