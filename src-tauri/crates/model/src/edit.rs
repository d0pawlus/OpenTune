// SPDX-License-Identifier: GPL-3.0-or-later
//! Internal undo/redo edit record for [`Tune`](crate::Tune).
//!
//! Not part of the frozen M2 seam — this is an implementation detail of
//! `Tune`'s undo/redo stack, kept crate-private until a later task needs to
//! expose it.

/// A single recorded byte-level change, sufficient to undo/redo a `Tune::set`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Edit {
    /// The page the edit applies to.
    pub(crate) page: u16,
    /// Byte offset within the page where `before`/`after` start.
    pub(crate) offset: usize,
    /// The bytes at `offset` before the edit, for undo.
    pub(crate) before: Vec<u8>,
    /// The bytes at `offset` after the edit, for redo.
    pub(crate) after: Vec<u8>,
}
