//! Relative links in the repository's Markdown files point to existing files
//! and headings.

use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
};

fn markdown_files(directory: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if path.is_dir() {
            if name != "target" && !name.starts_with('.') {
                markdown_files(&path, files)?;
            }
        } else if name.ends_with(".md") {
            files.push(path);
        }
    }
    Ok(())
}

/// Lines outside fenced code blocks.
fn prose(text: &str) -> impl Iterator<Item = &str> {
    let mut fenced = false;
    text.lines().filter(move |line| {
        let fence = line.trim_start().starts_with("```");
        fenced ^= fence;
        !fenced && !fence
    })
}

/// GitHub's anchor for a heading.
fn anchor(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

fn anchors(text: &str) -> HashSet<String> {
    prose(text)
        .filter(|line| line.starts_with('#'))
        .map(|line| anchor(line.trim_start_matches('#')))
        .collect()
}

#[test]
fn relative_markdown_links_resolve() -> io::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    markdown_files(root, &mut files)?;
    assert!(files.iter().any(|file| file.ends_with("README.md")));

    let mut broken = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file)?;
        for line in prose(&text) {
            for rest in line.split("](").skip(1) {
                let Some((link, _)) = rest.split_once(')') else {
                    continue;
                };
                if link.contains("://") || link.starts_with("mailto:") {
                    continue;
                }
                let (path, fragment) = link.split_once('#').unwrap_or((link, ""));
                let target = if path.is_empty() {
                    file.clone()
                } else {
                    file.parent().unwrap_or(root).join(path)
                };
                let valid = target.exists()
                    && (fragment.is_empty()
                        || target.extension().is_none_or(|extension| extension != "md")
                        || anchors(&fs::read_to_string(&target)?).contains(fragment));
                if !valid {
                    let name = file.strip_prefix(root).unwrap_or(file);
                    broken.push(format!("{}: {link}", name.display()));
                }
            }
        }
    }
    assert!(broken.is_empty(), "broken links:\n{}", broken.join("\n"));
    Ok(())
}
