//! Already-applied agent edits. Review decisions never overwrite later manual edits.
use similar::{DiffTag, TextDiff};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

pub struct Review {
    pub before: String,
    pub current: String,
    pub hunks: Vec<Hunk>,
    pub active: usize,
    pub expanded: bool,
}

pub struct Hunk {
    /// Byte range in the current, editable document (not the original snapshot).
    pub range: Range<usize>,
    pub original: String,
    pub replacement: String,
    pub old_line: usize,
    pub resolved: bool,
    pub protected: bool,
}

fn offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for line in text.split_inclusive('\n') {
        offsets.push(offsets.last().unwrap() + line.len());
    }
    offsets
}

impl Review {
    pub fn new(before: String, current: String) -> Self {
        let old = offsets(&before);
        let new = offsets(&current);
        let diff = TextDiff::from_lines(&before, &current);
        let hunks = diff
            .grouped_ops(0)
            .into_iter()
            .filter_map(|ops| {
                let first = ops.first()?;
                let last = ops.last()?;
                let a = first.old_range().start..last.old_range().end;
                let b = first.new_range().start..last.new_range().end;
                Some(Hunk {
                    range: new[b.start]..new[b.end],
                    original: before[old[a.start]..old[a.end]].into(),
                    replacement: current[new[b.start]..new[b.end]].into(),
                    old_line: a.start,
                    resolved: false,
                    protected: false,
                })
            })
            .collect();
        Self {
            before,
            current,
            hunks,
            active: 0,
            expanded: true,
        }
    }

    pub fn pending(&self) -> usize {
        self.hunks.iter().filter(|h| !h.resolved).count()
    }

    /// Context leaves the last selected change active. Deletions have an empty
    /// live range, so their anchor is selectable even at the end of the file.
    pub fn select_at(&mut self, cursor: usize) {
        if let Some(index) = self.hunks.iter().position(|h| {
            !h.resolved
                && ((h.range.is_empty() && cursor == h.range.start) || h.range.contains(&cursor))
        }) {
            self.active = index;
        }
    }

    pub fn advance(&mut self) {
        if let Some(index) = self.hunks.iter().position(|h| !h.resolved) {
            self.active = index;
        }
    }

    /// Shift untouched ranges; touching a change protects it from review Undo.
    pub fn sync(&mut self, text: &str) {
        if self.current == text {
            return;
        }
        let a = self.current.graphemes(true).collect::<Vec<_>>();
        let b = text.graphemes(true).collect::<Vec<_>>();
        let byte_offsets = |parts: &[&str]| {
            let mut out = vec![0];
            for part in parts {
                out.push(out.last().unwrap() + part.len());
            }
            out
        };
        let old = byte_offsets(&a);
        let new = byte_offsets(&b);
        let diff = TextDiff::from_slices(&a, &b);
        for hunk in &mut self.hunks {
            if hunk.resolved {
                continue;
            }
            let mut shift = 0isize;
            for op in diff.ops() {
                if op.tag() == DiffTag::Equal {
                    continue;
                }
                let r = op.old_range();
                let s = op.new_range();
                let start = old[r.start];
                let end = old[r.end];
                // Boundary insertions are conservatively considered manual modifications.
                let touches = if start == end {
                    start >= hunk.range.start && start <= hunk.range.end
                } else if hunk.range.is_empty() {
                    start <= hunk.range.start && end >= hunk.range.start
                } else {
                    start < hunk.range.end && end > hunk.range.start
                };
                if touches {
                    hunk.protected = true;
                    hunk.resolved = true;
                    break;
                }
                if end <= hunk.range.start {
                    shift += (new[s.end] - new[s.start]) as isize - (end - start) as isize;
                }
            }
            if !hunk.resolved {
                hunk.range = hunk.range.start.saturating_add_signed(shift)
                    ..hunk.range.end.saturating_add_signed(shift);
            }
        }
        self.current = text.into();
        if self.hunks.get(self.active).is_some_and(|h| h.resolved) {
            self.advance();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tracks_unicode_edits_before_hunks_and_protects_overlap() {
        let mut review = Review::new("start\nold\nend\n".into(), "start\nnew\nend\n".into());
        assert_eq!(review.pending(), 1);
        review.sync("界👩‍💻start\nnew\nend\n");
        let h = &review.hunks[0];
        assert_eq!(&review.current[h.range.clone()], "new\n");
        assert!(!h.protected);
        review.sync("界👩‍💻start\nnew manual\nend\n");
        assert_eq!(review.pending(), 0);
        assert!(review.hunks[0].protected);
    }
    #[test]
    fn cursor_selects_pending_ranges_and_empty_deletion_anchors() {
        let mut review = Review::new(
            "old\ncontext\ndeleted\nlast\n".into(),
            "界👩‍💻\ncontext\nlast\n".into(),
        );
        assert_eq!(review.hunks.len(), 2);
        let deletion = review.hunks[1].range.start;
        review.select_at(deletion);
        assert_eq!(review.active, 1);
        review.select_at("界👩‍💻\n".len());
        assert_eq!(review.active, 1, "context retains the selected change");
        review.select_at("界".len());
        assert_eq!(review.active, 0);
        review.hunks[1].resolved = true;
        review.select_at(deletion);
        assert_eq!(review.active, 0, "resolved changes cannot be selected");

        let mut eof = Review::new("context\ndeleted".into(), "context\n".into());
        eof.select_at(eof.current.len());
        assert_eq!(eof.active, 0);
        assert!(eof.hunks[0].range.is_empty());
    }

    #[test]
    fn deletions_and_boundary_insertions_are_protected() {
        let mut review = Review::new("a\nb\nc\n".into(), "a\nc\n".into());
        assert!(review.hunks[0].range.is_empty());
        review.sync("a\nmanual\nc\n");
        assert!(review.hunks[0].protected);
    }
}
