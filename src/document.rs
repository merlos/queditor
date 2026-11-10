//! Document operations use UTF-8 byte offsets at grapheme boundaries.
use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
}

#[derive(Debug, Default)]
pub struct Document {
    pub text: String,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub path: Option<PathBuf>,
    pub revision: u64,
    saved: String,
    disk_baseline: Option<String>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Recovery {
    pub text: String,
    pub cursor: usize,
    pub path: Option<PathBuf>,
    pub saved: String,
    pub disk_baseline: Option<String>,
}

impl Document {
    pub fn recovery(&self) -> Recovery {
        Recovery {
            text: self.text.clone(),
            cursor: self.cursor,
            path: self.path.clone(),
            saved: self.saved.clone(),
            disk_baseline: self.disk_baseline.clone(),
        }
    }

    pub fn from_recovery(r: Recovery) -> Result<Self> {
        if r.text.len() > MAX_FILE_BYTES as usize
            || r.saved.len() > MAX_FILE_BYTES as usize
            || r.disk_baseline
                .as_ref()
                .is_some_and(|s| s.len() > MAX_FILE_BYTES as usize)
        {
            bail!("Recovery exceeds document size limit");
        }
        let mut d = Self {
            text: r.text,
            saved: r.saved,
            disk_baseline: r.disk_baseline,
            path: r.path,
            ..Self::default()
        };
        d.move_to(r.cursor, false);
        Ok(d)
    }

    pub fn open(path: &Path) -> Result<Self> {
        let path = absolute(path)?;
        if !path.exists() {
            bail!("File does not exist: {}", path.display());
        }
        let text = read_text(&path)?;
        Ok(Self {
            saved: text.clone(),
            disk_baseline: Some(text.clone()),
            text,
            path: Some(path),
            ..Self::default()
        })
    }

    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }

    pub fn selection(&self) -> Option<std::ops::Range<usize>> {
        self.anchor
            .filter(|a| *a != self.cursor)
            .map(|a| a.min(self.cursor)..a.max(self.cursor))
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|r| &self.text[r])
    }

    /// One-based inclusive line range covered by the current selection.
    pub fn selected_lines(&self) -> Option<std::ops::RangeInclusive<usize>> {
        let range = self.selection()?;
        let bytes = self.text.as_bytes();
        let start = bytes[..range.start]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1;
        // A trailing newline belongs to the line it terminates, not the next line.
        let end_offset = if bytes.get(range.end.saturating_sub(1)) == Some(&b'\n') {
            range.end.saturating_sub(1)
        } else {
            range.end
        };
        let end = bytes[..end_offset]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1;
        Some(start..=end)
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.text = s.text;
        self.cursor = s.cursor;
        self.anchor = s.anchor;
        self.revision += 1;
    }

    pub fn replace(&mut self, range: std::ops::Range<usize>, replacement: &str) {
        if self.text[range.clone()] == *replacement {
            return;
        }
        self.undo.push(self.snapshot());
        // Bound snapshot memory until an incremental undo representation is introduced.
        while self.undo.len() > 100
            || self.undo.iter().map(|s| s.text.len()).sum::<usize>() > 32 * 1024 * 1024
        {
            self.undo.remove(0);
        }
        self.redo.clear();
        let end = range.start + replacement.len();
        self.text.replace_range(range, replacement);
        self.cursor = boundary_at_or_after(&self.text, end);
        self.anchor = None;
        self.revision += 1;
    }

    pub fn insert(&mut self, text: &str) {
        let text = normalize_newlines(text, self.text.contains("\r\n"));
        let range = self.selection().unwrap_or(self.cursor..self.cursor);
        self.replace(range, &text);
    }

    pub fn backspace(&mut self) {
        if let Some(r) = self.selection() {
            self.replace(r, "");
        } else if self.cursor > 0 {
            self.replace(previous(&self.text, self.cursor)..self.cursor, "");
        }
    }

    pub fn delete(&mut self) {
        if let Some(r) = self.selection() {
            self.replace(r, "");
        } else if self.cursor < self.text.len() {
            self.replace(self.cursor..next(&self.text, self.cursor), "");
        }
    }

    /// Delete the active selection, or from the cursor to the end of the current line.
    pub fn kill_line(&mut self) {
        if let Some(range) = self.selection() {
            self.replace(range, "");
            return;
        }
        let row = self.line_column().0;
        let end = self.line_start(row) + self.line(row).len();
        if self.cursor < end {
            self.replace(self.cursor..end, "");
        }
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(s);
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(s);
        }
    }

    pub fn move_to(&mut self, pos: usize, selecting: bool) {
        if selecting {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = boundary_at_or_after(&self.text, pos.min(self.text.len()));
    }

    pub fn left(&mut self, selecting: bool) {
        let p = if !selecting {
            self.selection().map(|r| r.start)
        } else {
            None
        }
        .unwrap_or_else(|| previous(&self.text, self.cursor));
        self.move_to(p, selecting);
    }

    pub fn right(&mut self, selecting: bool) {
        let p = if !selecting {
            self.selection().map(|r| r.end)
        } else {
            None
        }
        .unwrap_or_else(|| next(&self.text, self.cursor));
        self.move_to(p, selecting);
    }

    pub fn word_left(&mut self, selecting: bool) {
        let text = &self.text[..self.cursor];
        let pos = text
            .split_word_bound_indices()
            .rev()
            .find(|(_, s)| !s.chars().all(char::is_whitespace))
            .map_or(0, |(i, _)| i);
        self.move_to(pos, selecting);
    }

    pub fn word_right(&mut self, selecting: bool) {
        let text = &self.text[self.cursor..];
        let mut pos = text.len();
        let mut found_word = false;
        for (i, s) in text.split_word_bound_indices() {
            if found_word && !s.chars().all(char::is_whitespace) {
                pos = i;
                break;
            }
            if !s.chars().all(char::is_whitespace) {
                found_word = true;
            }
        }
        self.move_to(self.cursor + pos, selecting);
    }

    pub fn line_column(&self) -> (usize, usize) {
        let row = self.text[..self.cursor]
            .bytes()
            .filter(|b| *b == b'\n')
            .count();
        let start = self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
        (row, self.text[start..self.cursor].graphemes(true).count())
    }

    pub fn line_start(&self, row: usize) -> usize {
        if row == 0 {
            return 0;
        }
        self.text
            .match_indices('\n')
            .nth(row - 1)
            .map_or(self.text.len(), |(i, _)| i + 1)
    }

    pub fn line(&self, row: usize) -> &str {
        let start = self.line_start(row);
        self.text[start..]
            .split('\n')
            .next()
            .unwrap_or("")
            .trim_end_matches('\r')
    }

    pub fn position(&self, row: usize, column: usize) -> usize {
        let line = self.line(row);
        self.line_start(row)
            + line
                .grapheme_indices(true)
                .nth(column)
                .map_or(line.len(), |(i, _)| i)
    }

    pub fn vertical(&mut self, delta: isize, selecting: bool) {
        let (row, col) = self.line_column();
        let count = self.text.bytes().filter(|b| *b == b'\n').count();
        let row = row.saturating_add_signed(delta).min(count);
        self.move_to(self.position(row, col), selecting);
    }

    pub fn display_column(&self) -> usize {
        let (row, _) = self.line_column();
        display_width(&self.text[self.line_start(row)..self.cursor])
    }

    pub fn position_at_display(&self, row: usize, column: usize) -> usize {
        let mut width = 0;
        for (i, g) in self.line(row).grapheme_indices(true) {
            let w = grapheme_width(g, width);
            if column < width + w {
                return self.line_start(row) + i;
            }
            width += w;
        }
        self.line_start(row) + self.line(row).len()
    }

    pub fn save(&mut self, requested: Option<&Path>) -> Result<()> {
        let path = absolute(
            requested
                .or(self.path.as_deref())
                .context("Use /save <path> for an unnamed document")?,
        )?;
        let same = self.path.as_ref().is_some_and(|p| p == &path);
        if path.exists() {
            if !same {
                bail!("Refusing to overwrite another file: {}", path.display());
            }
            let current = read_text(&path)?;
            if self.disk_baseline.as_deref() != Some(&current) {
                bail!("File changed on disk; save to a new path or reopen it");
            }
        } else if same && self.disk_baseline.is_some() {
            bail!("File was removed on disk; save to a new path");
        }
        let parent = path.parent().context("Invalid save path")?;
        let mut temp =
            tempfile::NamedTempFile::new_in(parent).context("Cannot create save file")?;
        if path.exists() {
            temp.as_file()
                .set_permissions(fs::metadata(&path)?.permissions())?;
        }
        temp.write_all(self.text.as_bytes())?;
        temp.as_file().sync_all()?;
        if same && path.exists() {
            if self.disk_baseline.as_deref() != Some(read_text(&path)?.as_str()) {
                bail!("File changed during save; destination left untouched");
            }
            temp.persist(&path)
                .map_err(|e| e.error)
                .context("Cannot replace destination")?;
        } else {
            // A newly appearing file must not be overwritten between checking and writing.
            temp.persist_noclobber(&path)
                .map_err(|e| e.error)
                .context("Cannot create destination without overwriting")?;
        }
        self.path = Some(path);
        self.saved = self.text.clone();
        self.disk_baseline = Some(self.text.clone());
        Ok(())
    }
}

pub fn absolute(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    // Existing symlinks resolve to their target, rather than being replaced on save.
    if path.exists() {
        Ok(path.canonicalize()?)
    } else {
        let parent = path
            .parent()
            .context("Missing parent directory")?
            .canonicalize()?;
        Ok(parent.join(path.file_name().context("Missing filename")?))
    }
}

pub fn read_text(path: &Path) -> Result<String> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        bail!("Not a regular file");
    }
    if meta.len() > MAX_FILE_BYTES {
        bail!("Files larger than 8 MiB are not supported yet");
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        bail!("File grew beyond the 8 MiB limit");
    }
    if bytes.contains(&0) {
        bail!("Binary files cannot be opened");
    }
    String::from_utf8(bytes).context("Only UTF-8 text is supported")
}

pub fn previous(text: &str, pos: usize) -> usize {
    text[..pos]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(i, _)| i)
}
pub fn next(text: &str, pos: usize) -> usize {
    pos + text[pos..].graphemes(true).next().map_or(0, str::len)
}
fn boundary_at_or_after(text: &str, pos: usize) -> usize {
    text.grapheme_indices(true)
        .map(|(i, _)| i)
        .find(|i| *i >= pos)
        .unwrap_or(text.len())
}
fn normalize_newlines(text: &str, crlf: bool) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    if crlf {
        text.replace('\n', "\r\n")
    } else {
        text
    }
}
pub fn grapheme_width(g: &str, column: usize) -> usize {
    if g == "\t" {
        4 - column % 4
    } else if g.chars().any(|c| c.is_control() && c != '\r' && c != '\n') {
        1
    } else {
        UnicodeWidthStr::width(g)
    }
}
pub fn display_width(text: &str) -> usize {
    text.graphemes(true)
        .fold(0, |w, g| w + grapheme_width(g, w))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grapheme_editing_and_history() {
        let mut d = Document::default();
        d.insert("a👩‍💻e\u{301}");
        d.backspace();
        assert_eq!(d.text, "a👩‍💻");
        d.backspace();
        assert_eq!(d.text, "a");
        d.undo();
        assert_eq!(d.text, "a👩‍💻");
        d.redo();
        assert_eq!(d.text, "a");
    }
    #[test]
    fn selection_and_dirty_baseline() {
        let mut d = Document::default();
        d.insert("hello");
        d.move_to(0, true);
        assert_eq!(d.selected_text(), Some("hello"));
        d.insert("hi");
        assert_eq!(d.text, "hi");
        d.undo();
        d.undo();
        assert!(!d.dirty());
    }

    #[test]
    fn selected_lines_are_one_based_inclusive() {
        let mut d = Document::default();
        d.insert("one\ntwo\nthree");
        d.move_to(0, false);
        d.move_to(7, true);
        assert_eq!(d.selected_text(), Some("one\ntwo"));
        assert_eq!(d.selected_lines(), Some(1..=2));

        d.move_to(0, false);
        d.move_to(4, true);
        assert_eq!(d.selected_text(), Some("one\n"));
        assert_eq!(d.selected_lines(), Some(1..=1));
    }

    #[test]
    fn kill_line_is_undoable_and_preserves_newline() {
        let mut d = Document::default();
        d.insert("one two\nthree");
        d.move_to(4, false);
        d.kill_line();
        assert_eq!(d.text, "one \nthree");
        assert_eq!(d.cursor, 4);
        d.undo();
        assert_eq!(d.text, "one two\nthree");

        d.move_to(4, false);
        d.move_to(7, true);
        d.kill_line();
        assert_eq!(d.text, "one \nthree");
    }
    #[test]
    fn preserves_crlf_and_final_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("text.md");
        fs::write(&path, "one\r\ntwo").unwrap();
        let mut d = Document::open(&path).unwrap();
        d.move_to(d.text.len(), false);
        d.insert("\nthree");
        d.save(None).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "one\r\ntwo\r\nthree");
        assert!(!d.dirty());
        d.undo();
        assert!(d.dirty());
    }
    #[test]
    fn external_changes_are_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("text");
        fs::write(&path, "old").unwrap();
        let mut d = Document::open(&path).unwrap();
        d.insert("new");
        fs::write(&path, "external").unwrap();
        assert!(d.save(None).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "external");
    }
    #[test]
    fn display_positions_include_tabs_and_wide_characters() {
        let mut d = Document::default();
        d.insert("\t界a");
        assert_eq!(d.display_column(), 7);
        assert_eq!(d.position_at_display(0, 4), 1);
        assert_eq!(d.position_at_display(0, 6), 4);
    }
    #[test]
    fn combining_insert_keeps_cursor_on_boundary() {
        let mut d = Document::default();
        d.insert("a");
        d.insert("\u{301}");
        d.left(false);
        assert_eq!(d.cursor, 0);
        d.delete();
        assert!(d.text.is_empty());
    }

    #[test]
    fn rejects_binary_and_unrelated_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("binary");
        fs::write(&binary, [0, 1, 2]).unwrap();
        assert!(Document::open(&binary).is_err());
        let target = dir.path().join("existing");
        fs::write(&target, "user content").unwrap();
        let mut d = Document::default();
        d.insert("replacement");
        assert!(d.save(Some(&target)).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "user content");
    }

    #[cfg(unix)]
    #[test]
    fn saving_through_symlink_preserves_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let link = dir.path().join("link");
        fs::write(&target, "old").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let mut d = Document::open(&link).unwrap();
        d.replace(0..d.text.len(), "new");
        d.save(None).unwrap();
        assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(target).unwrap(), "new");
    }

    #[test]
    fn unicode_edit_sequences_preserve_boundaries_and_undo() {
        let mut d = Document::default();
        let pieces = ["a", "界", "👩‍💻", "e\u{301}", "\n", "\t", "🇪🇸", "\u{301}"];
        let mut seed = 12345_u64;
        for _ in 0..300 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            match seed % 6 {
                0 => d.left(false),
                1 => d.right(false),
                2 => d.vertical(if seed & 256 == 0 { -1 } else { 1 }, false),
                action => {
                    let before = d.text.clone();
                    let revision = d.revision;
                    match action {
                        3 => d.insert(pieces[(seed >> 8) as usize % pieces.len()]),
                        4 => d.backspace(),
                        _ => d.delete(),
                    }
                    if revision != d.revision {
                        let after = d.text.clone();
                        d.undo();
                        assert_eq!(d.text, before);
                        d.redo();
                        assert_eq!(d.text, after);
                    }
                }
            }
            assert!(
                d.cursor == d.text.len()
                    || d.text.grapheme_indices(true).any(|(i, _)| i == d.cursor)
            );
        }
    }
}
