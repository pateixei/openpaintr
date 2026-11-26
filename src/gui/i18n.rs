use std::{
    borrow::Cow,
    cell::Cell,
    collections::BTreeMap,
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
    if let Ok(cwd) = std::env::current_dir() {
        catalogs.scan(&cwd.join("translations"));
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        catalogs.scan(&directory.join("translations"));
    }
    catalogs
});

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
            let Ok(data) = std::fs::read(&path) else {
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
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".config")))
            .map(|path| path.join("openpaintr").join("language.json"))
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    pub fn save(self) -> io::Result<()> {
        self.save_at(&Self::settings_path()?)
    }

    fn load_at(path: &Path) -> io::Result<String> {
        let bytes = std::fs::read(path)?;
        serde_json::from_slice(&bytes).map_err(io::Error::other)
    }

    fn save_at(self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path.parent().expect("settings file has a directory"))?;
        super::dialogs::atomic_write_bytes(path, &serde_json::to_vec(self.0)?, true)
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
    const KEYS: [&str; 14] = [
        "blend.normal",
        "blend.multiply",
        "blend.additive",
        "blend.color-burn",
        "blend.color-dodge",
        "blend.reflect",
        "blend.glow",
        "blend.overlay",
        "blend.difference",
        "blend.negation",
        "blend.lighten",
        "blend.darken",
        "blend.screen",
        "blend.xor",
    ];
    logical_text(KEYS.get(usize::from(index)).copied().unwrap_or(KEYS[0]))
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

    #[test]
    fn bundled_catalogs_have_all_keys_and_matching_parameters() {
        let catalogs = Catalogs::bundled();
        let english = &catalogs.0[ENGLISH];
        assert!(!english.is_empty());
        for catalog in catalogs.0.values() {
            assert!(english.keys().eq(catalog.keys()));
            for (key, message) in catalog {
                assert!(!message.trim().is_empty(), "{key}");
                assert_eq!(placeholders(message), placeholders(&english[key]), "{key}");
                assert!(!placeholders(message).contains(&usize::MAX), "{key}");
            }
        }
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
