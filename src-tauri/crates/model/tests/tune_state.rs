// SPDX-License-Identifier: GPL-3.0-or-later
//! Task 4 tests — `Tune` dirty/flash state (sub-step 4.3) and undo/redo
//! (4.4). Scaled-accessor and error-surface tests live in `tune.rs`.

mod common;

use common::{load1, scalar, scalar_on, tune};
use opentune_ini::{Endianness, ScalarType};
use opentune_model::Value;

fn u08(name: &str, offset: usize) -> opentune_ini::ConstantDef {
    scalar(name, ScalarType::U08, offset, 1.0, 0.0, 255.0)
}

// ---------- 4.3 — dirty + flash state ----------

#[test]
fn load_page_does_not_mark_dirty() {
    let mut t = tune(Endianness::Little, vec![u08("c", 0)]);
    load1(&mut t, &[7]);
    assert!(!t.is_dirty());
    assert!(t.dirty_pages().is_empty());
}

#[test]
fn set_marks_page_dirty() {
    let mut t = tune(Endianness::Little, vec![u08("c", 0)]);
    t.set("c", Value::Scalar(9.0)).unwrap();
    assert!(t.is_dirty());
    assert_eq!(t.dirty_pages(), vec![1]);
}

#[test]
fn dirty_pages_are_sorted_across_pages() {
    let a = u08("a", 0);
    let b = scalar_on(2, "b", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    let mut t = tune(Endianness::Little, vec![a, b]);
    t.set("b", Value::Scalar(2.0)).unwrap();
    t.set("a", Value::Scalar(1.0)).unwrap();
    assert_eq!(t.dirty_pages(), vec![1, 2]);
}

#[test]
fn mark_burned_clears_dirty() {
    let mut t = tune(Endianness::Little, vec![u08("c", 0)]);
    t.set("c", Value::Scalar(9.0)).unwrap();
    t.mark_burned();
    assert!(!t.is_dirty());
    assert!(t.dirty_pages().is_empty());
}

#[test]
fn set_writing_identical_bytes_records_nothing() {
    let mut t = tune(Endianness::Little, vec![u08("c", 0)]);
    load1(&mut t, &[50]);
    t.set("c", Value::Scalar(50.0)).unwrap();
    assert!(!t.is_dirty());
    assert!(!t.undo());
}

// ---------- 4.4 — undo/redo ----------

#[test]
fn undo_restores_prior_bytes_and_value() {
    let mut t = tune(Endianness::Little, vec![u08("c", 0)]);
    load1(&mut t, &[10]);
    t.set("c", Value::Scalar(20.0)).unwrap();
    assert_eq!(t.page_bytes(1)[0], 20);
    assert!(t.undo());
    assert_eq!(t.page_bytes(1)[0], 10);
    assert_eq!(t.get("c").unwrap(), Value::Scalar(10.0));
}

#[test]
fn undo_on_empty_stack_returns_false() {
    let mut t = tune(Endianness::Little, vec![u08("c", 0)]);
    assert!(!t.undo());
    assert!(!t.redo());
}

#[test]
fn redo_reapplies_the_undone_edit() {
    let mut t = tune(Endianness::Little, vec![u08("c", 0)]);
    t.set("c", Value::Scalar(20.0)).unwrap();
    assert!(t.undo());
    assert!(t.redo());
    assert_eq!(t.page_bytes(1)[0], 20);
    assert!(!t.redo());
}

#[test]
fn fresh_set_clears_the_redo_stack() {
    let mut t = tune(Endianness::Little, vec![u08("c", 0)]);
    t.set("c", Value::Scalar(20.0)).unwrap();
    assert!(t.undo());
    t.set("c", Value::Scalar(30.0)).unwrap();
    assert!(!t.redo());
    assert_eq!(t.page_bytes(1)[0], 30);
}

#[test]
fn undo_across_pages_pops_in_reverse_order() {
    let a = u08("a", 0);
    let b = scalar_on(2, "b", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    let mut t = tune(Endianness::Little, vec![a, b]);
    t.set("a", Value::Scalar(1.0)).unwrap();
    t.set("b", Value::Scalar(2.0)).unwrap();
    assert!(t.undo()); // undoes b
    assert_eq!(t.page_bytes(2)[0], 0);
    assert_eq!(t.page_bytes(1)[0], 1);
    assert!(t.undo()); // undoes a
    assert_eq!(t.page_bytes(1)[0], 0);
}

#[test]
fn undo_after_burn_re_marks_dirty() {
    let mut t = tune(Endianness::Little, vec![u08("c", 0)]);
    t.set("c", Value::Scalar(20.0)).unwrap();
    t.mark_burned();
    assert!(!t.is_dirty());
    assert!(t.undo()); // RAM != flash again
    assert!(t.is_dirty());
    assert_eq!(t.dirty_pages(), vec![1]);
}

#[test]
fn load_page_clears_undo_history() {
    let mut t = tune(Endianness::Little, vec![u08("c", 0)]);
    t.set("c", Value::Scalar(20.0)).unwrap();
    load1(&mut t, &[99]); // prior edits reference stale bytes
    assert!(!t.undo());
    assert!(!t.redo());
}
