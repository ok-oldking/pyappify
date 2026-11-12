//! Patch notes from human-written changelog files, one per language.
//!
//! An app keeps `CHANGELOG.md` (English) at the root of its repository and may
//! add `CHANGELOG.<language>.md` for each launcher language, for example
//! `CHANGELOG.zh-CN.md` or `CHANGELOG.ja.md`:
//!
//! ```markdown
//! # Changelog
//!
//! ## v1.3.0 (2026-11-12)
//! - Added a dark theme
//! - Fixed a crash when a folder name
//!   contained emoji
//!
//! ## v1.2.0
//! - First public release
//! ```
//!
//! `## <version>` starts a version; `[v1.3.0]`, `1.3.0` and trailing text such
//! as a date also match the tag `v1.3.0`. `- ` or `* ` starts a note and an
//! indented line continues it. Everything else, including `### Added` style
//! sub-headings, is ignored.
//!
//! The changelogs are read from the version being installed. When that version
//! has an entry in the reader's language or in English, the notes are the
//! entries of every version tag the update brings in, each in the reader's
//! language or else English. Otherwise, as for any untagged commit, they are
//! commit messages. pyappify-python reads the same files the same way.

use std::collections::{HashMap, HashSet};

pub const ENGLISH: &str = "en";
pub const CHANGELOG_FILE: &str = "CHANGELOG.md";

/// The launcher's languages, as the changelog file names spell them.
pub const LANGUAGES: [&str; 6] = ["en", "zh-CN", "zh-TW", "es", "ja", "ko"];

pub type Changelog = HashMap<String, Vec<String>>;

/// The file holding `language`'s notes: `CHANGELOG.md` or `CHANGELOG.<language>.md`.
pub fn changelog_path(language: &str) -> String {
    if language.is_empty() || language == ENGLISH {
        CHANGELOG_FILE.to_string()
    } else {
        format!("CHANGELOG.{language}.md")
    }
}

/// `[v1.3.0]` and `1.3.0` both name the tag `v1.3.0`: compare without them.
pub fn version_key(version: &str) -> String {
    let version = version.trim().trim_matches(|c| c == '[' || c == ']').trim();
    let mut chars = version.chars();
    match (chars.next(), chars.next()) {
        (Some('v' | 'V'), Some(next)) if next.is_ascii_digit() => version[1..].to_string(),
        _ => version.to_string(),
    }
}

/// `{version key: [note, ...]}` for each `## <version>` section of `text`.
pub fn parse_changelog(text: &str) -> Changelog {
    let mut notes = Changelog::new();
    let mut current: Option<String> = None;
    for raw_line in text.trim_start_matches('\u{feff}').lines() {
        let line = raw_line.trim();
        if let Some(header) = raw_line.strip_prefix("## ") {
            current = header.split_whitespace().next().map(|word| {
                let key = version_key(word);
                notes.entry(key.clone()).or_default();
                key
            });
            continue;
        }
        if raw_line.starts_with("# ") {
            current = None;
            continue;
        }
        let Some(version) = current.as_ref() else {
            continue;
        };
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let entry = notes.entry(version.clone()).or_default();
        if let Some(note) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            entry.push(note.trim().to_string());
        } else if raw_line.starts_with([' ', '\t']) {
            if let Some(last) = entry.last_mut() {
                last.push(' ');
                last.push_str(line);
            }
        }
    }
    notes
}

/// Whether `version` is in the localized changelog or the English one.
pub fn has_entry(localized: &Changelog, english: &Changelog, version: &str) -> bool {
    let key = version_key(version);
    localized.contains_key(&key) || english.contains_key(&key)
}

/// The notes of `versions` (newest first), localized where possible, else English.
/// Repeated notes are shown once.
pub fn notes_for_versions(
    localized: &Changelog,
    english: &Changelog,
    versions: &[String],
) -> Vec<String> {
    let mut notes = Vec::new();
    let mut seen = HashSet::new();
    for version in versions {
        let key = version_key(version);
        let entry = localized
            .get(&key)
            .filter(|entry| !entry.is_empty())
            .or_else(|| english.get(&key))
            .map(Vec::as_slice)
            .unwrap_or_default();
        for note in entry {
            if !note.is_empty() && seen.insert(note.clone()) {
                notes.push(note.clone());
            }
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENGLISH_CHANGELOG: &str = "\
# Changelog

## v1.2.0 (2026-11-12)
### Added
- Dark theme
- Faster duplicate
  detection

## [1.1.0] - 2026-10-01
- English 1.1.0 note

# Footer
- not a note
";

    const JAPANESE_CHANGELOG: &str = "\u{feff}## v1.2.0\n- ダークテーマ\n- 重複検出の高速化\n";

    #[test]
    fn parses_versions_notes_and_continuations() {
        let notes = parse_changelog(ENGLISH_CHANGELOG);
        assert_eq!(notes.len(), 2);
        assert_eq!(
            notes["1.2.0"],
            vec!["Dark theme", "Faster duplicate detection"]
        );
        assert_eq!(notes["1.1.0"], vec!["English 1.1.0 note"]);
    }

    #[test]
    fn version_keys_and_paths() {
        assert_eq!(version_key("[v1.2.0]"), "1.2.0");
        assert_eq!(version_key("version-2"), "version-2");
        assert_eq!(changelog_path("en"), "CHANGELOG.md");
        assert_eq!(changelog_path("zh-CN"), "CHANGELOG.zh-CN.md");
    }

    #[test]
    fn missing_translation_falls_back_to_english_per_version() {
        let english = parse_changelog(ENGLISH_CHANGELOG);
        let japanese = parse_changelog(JAPANESE_CHANGELOG);
        let versions = ["v1.2.0", "v1.1.0", "v1.0.0"].map(String::from);
        assert_eq!(
            notes_for_versions(&japanese, &english, &versions),
            vec!["ダークテーマ", "重複検出の高速化", "English 1.1.0 note"]
        );
        assert!(has_entry(&japanese, &english, "v1.1.0"));
        assert!(!has_entry(&japanese, &english, "v1.0.0"));
    }
}
