---
name: release-changelog
description: Write the changelog entry for a new version and tag the release. Use when the user asks to release, tag, cut or publish a version (e.g. "release v1.4.0", "tag a new version", "bump to 1.4.0"), or to write or update the changelog or patch notes. Updates CHANGELOG.md and every translated CHANGELOG.<language>.md, which PyAppify shows as update notes in the user's language.
---

# Release changelog

PyAppify shows a version's update notes from the changelogs at the root of
this repository, in the language the user picked:

| File | Language |
|---|---|
| `CHANGELOG.md` | English, and the fallback for every other language |
| `CHANGELOG.zh-CN.md` | Simplified Chinese |
| `CHANGELOG.zh-TW.md` | Traditional Chinese |
| `CHANGELOG.es.md` | Spanish |
| `CHANGELOG.ja.md` | Japanese |
| `CHANGELOG.ko.md` | Korean |

Format, which PyAppify parses:

```markdown
# Changelog

## v1.4.0 (2026-11-12)
- Added a dark theme
- Fixed a crash when a folder name
  contained emoji

## v1.3.0 (2026-10-01)
- ...
```

- `## <tag>` starts a version. The first word must be the git tag (`v1.4.0`;
  `[v1.4.0]` or `1.4.0` also match). Anything after it, like a date, is ignored.
- `- ` starts a note; an indented line continues the previous note.
- Other lines (the title, intro text, `### Added` sub-headings) are ignored.
- Newest version first.
- A version missing from a translation shows its English notes. A tag with no
  entry at all shows its commit messages instead.

## Steps

1. **Pick the version.** Use the one the user gave. Otherwise list the tags
   (`git tag --sort=-v:refname | head`) and propose the next one: patch for
   fixes only, minor for features, major for breaking changes. Confirm it.

2. **Collect the changes** since the previous tag:

   ```bash
   git log --no-merges --format='%h %s%n%b' <previous-tag>..HEAD
   ```

   Read the diffs of commits whose messages are unclear (`git show --stat <sha>`).

3. **Write the English entry** at the top of `CHANGELOG.md`, below the title
   (create the file with a `# Changelog` title if it does not exist):
   - Write for the app's users, not developers: what changed for them, in plain
     words. "Fixed the window opening off-screen", not "fix: clamp geometry in
     MainWindow.restore()".
   - Leave out internal-only changes (refactors, CI, tests, dependency bumps
     without a visible effect).
   - One line per note, usually 3 to 8 notes. Start with a verb (Added, Fixed,
     Improved, Removed).

4. **Translate the entry** into every `CHANGELOG.<language>.md` that already
   exists, at the top of each file, under the same `## <tag>` header. Keep
   product names, UI labels the app does not translate, and code as they are.
   Do not create new language files unless the user asks.

5. **Show the user** the English entry and the translations, and wait for
   approval or edits.

6. **Commit and tag** once approved:

   ```bash
   git add CHANGELOG*.md
   git commit -m "Release <tag>"
   git tag <tag>
   ```

   The notes must be in the tagged commit: PyAppify reads the changelogs from
   the version being installed.

7. **Push only when the user asks**: `git push` and `git push origin <tag>`.

## Fixing notes after a release

The tagged commit cannot change. Fix or translate the old entry in a normal
commit instead: PyAppify reads the changelogs from the version being installed,
so anyone updating across that version to the next release sees the corrected
entry.
