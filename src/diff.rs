//! Diff computation and state management for agent edits.
//!
//! When an agent writes to a file, we:
//! 1. Snapshot the current buffer state (via RenderSnapshot)
//! 2. Apply the new content to the buffer
//! 3. Compute line-level hunks between old and new
//! 4. Store DiffState for rendering ghost lines and accept/reject

use crate::buffer::RenderSnapshot;
use imara_diff::{Algorithm, Diff, InternedInput};
use std::ops::Range;

/// Status of a diff hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkStatus {
    /// Hunk is pending review.
    Pending,
}

/// A single diff hunk representing a contiguous change.
#[derive(Debug, Clone)]
pub struct DiffHunk {
    /// Line range in the old snapshot (deletions).
    pub old_lines: Range<usize>,
    /// Line range in the current buffer (additions).
    pub new_lines: Range<usize>,
    /// Current status of this hunk.
    pub status: HunkStatus,
}

/// State for an active diff review session.
pub struct DiffState {
    /// Snapshot of the document before the agent edit.
    /// Used for rendering ghost lines with proper markdown styling.
    pub old_snapshot: RenderSnapshot,
    /// The hunks representing changes.
    pub hunks: Vec<DiffHunk>,
}

impl DiffState {
    /// Compute diff state from old snapshot and new text.
    pub fn compute(old_snapshot: RenderSnapshot, old_text: &str, new_text: &str) -> Self {
        let hunks = compute_line_hunks(old_text, new_text);
        Self {
            old_snapshot,
            hunks,
        }
    }

    /// Check if there are any pending hunks.
    pub fn has_pending_hunks(&self) -> bool {
        self.hunks.iter().any(|h| h.status == HunkStatus::Pending)
    }

    /// Accept a hunk by index. Since changes are already applied, just remove it.
    pub fn accept_hunk(&mut self, index: usize) {
        if index < self.hunks.len() {
            self.hunks.remove(index);
        }
    }

    /// Accept all pending hunks.
    pub fn accept_all(&mut self) {
        self.hunks.clear();
    }

    /// Get the old line range for a hunk if it exists and overlaps with the given new line.
    /// Returns (old_line_start, old_line_count) for rendering ghost lines.
    pub fn ghost_lines_before(&self, new_line: usize) -> Option<Range<usize>> {
        for hunk in &self.hunks {
            if hunk.status == HunkStatus::Pending && hunk.new_lines.start == new_line {
                if !hunk.old_lines.is_empty() {
                    return Some(hunk.old_lines.clone());
                }
            }
        }
        None
    }

    /// Check if a line in the new buffer is an addition (part of a hunk's new_lines).
    pub fn is_addition(&self, new_line: usize) -> bool {
        self.hunks
            .iter()
            .any(|h| h.status == HunkStatus::Pending && h.new_lines.contains(&new_line))
    }
}

/// Compute line-level hunks between old and new text.
fn compute_line_hunks(old_text: &str, new_text: &str) -> Vec<DiffHunk> {
    let input = InternedInput::new(old_text, new_text);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);

    diff.hunks()
        .map(|hunk| DiffHunk {
            old_lines: hunk.before.start as usize..hunk.before.end as usize,
            new_lines: hunk.after.start as usize..hunk.after.end as usize,
            status: HunkStatus::Pending,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_line_hunks_addition() {
        let old = "line1\nline2\n";
        let new = "line1\ninserted\nline2\n";
        let hunks = compute_line_hunks(old, new);

        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_lines, 1..1); // empty range = pure insertion
        assert_eq!(hunks[0].new_lines, 1..2); // one line inserted
    }

    #[test]
    fn test_compute_line_hunks_deletion() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nline3\n";
        let hunks = compute_line_hunks(old, new);

        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_lines, 1..2); // one line deleted
        assert_eq!(hunks[0].new_lines, 1..1); // empty range = pure deletion
    }

    #[test]
    fn test_compute_line_hunks_modification() {
        let old = "line1\nold content\nline3\n";
        let new = "line1\nnew content\nline3\n";
        let hunks = compute_line_hunks(old, new);

        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_lines, 1..2); // one line removed
        assert_eq!(hunks[0].new_lines, 1..2); // one line added
    }
}
