//! A hook to translate interface text just before it is laid out.
//!
//! Added for ZigZag (not part of upstream epaint): the application registers
//! a function that maps a piece of text to its translation, and every
//! [`LayoutJob`](super::LayoutJob) has each of its sections passed through it.
//! Text being edited (a `TextEdit`, which lays out with
//! `keep_trailing_whitespace`) is left alone, so the text and its cursor stay
//! in step.

use std::sync::RwLock;

use super::{ByteIndex, LayoutJob};

/// Maps a piece of text to its translation, or `None` to keep it.
pub type Translator = fn(&str) -> Option<String>;

static TRANSLATOR: RwLock<Option<Translator>> = RwLock::new(None);

/// Set (or with `None`, clear) the function interface text goes through.
pub fn set_translator(f: Option<Translator>) {
    if let Ok(mut t) = TRANSLATOR.write() {
        *t = f;
    }
}

/// The job with each section's text translated (unchanged when there's no
/// translator, nothing matches, or it's text being edited).
pub(crate) fn apply(job: LayoutJob) -> LayoutJob {
    if job.keep_trailing_whitespace || job.text.is_empty() {
        return job;
    }
    let Some(f) = TRANSLATOR.read().ok().and_then(|t| *t) else {
        return job;
    };
    let mut changed = false;
    let mut pieces: Vec<Option<String>> = Vec::with_capacity(job.sections.len());
    for s in &job.sections {
        let piece = job
            .text
            .get(s.byte_range.start.0..s.byte_range.end.0)
            .and_then(f);
        changed |= piece.is_some();
        pieces.push(piece);
    }
    if !changed {
        return job;
    }
    let mut job = job;
    let mut text = String::with_capacity(job.text.len() + 16);
    for (s, piece) in job.sections.iter_mut().zip(pieces) {
        let start = text.len();
        match piece {
            Some(p) => text.push_str(&p),
            None => text.push_str(
                job.text
                    .get(s.byte_range.start.0..s.byte_range.end.0)
                    .unwrap_or_default(),
            ),
        }
        s.byte_range = ByteIndex(start)..ByteIndex(text.len());
    }
    job.text = text;
    job
}
