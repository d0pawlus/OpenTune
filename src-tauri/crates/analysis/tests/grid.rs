// SPDX-License-Identifier: GPL-3.0-or-later
//! `TableGrid::lookup` — bilinear interpolation + degenerate-bin pinning
//! (M4 Task 10, brief step 10.1).
use opentune_analysis::TableGrid;

fn grid() -> TableGrid {
    TableGrid {
        x_bins: vec![1000.0, 2000.0],
        y_bins: vec![20.0, 40.0],
        z: vec![10.0, 20.0, 30.0, 40.0],
    }
}

#[test]
fn interpolates_bilinearly_at_the_grid_center() {
    let g = grid();
    assert_eq!(g.lookup(1500.0, 30.0), Some(25.0));
}

#[test]
fn returns_the_exact_corner_value() {
    let g = grid();
    assert_eq!(g.lookup(1000.0, 20.0), Some(10.0));
}

#[test]
fn returns_none_below_the_x_axis() {
    let g = grid();
    assert_eq!(g.lookup(999.0, 30.0), None);
}

#[test]
fn returns_none_above_the_y_axis() {
    let g = grid();
    assert_eq!(g.lookup(1500.0, 41.0), None);
}

#[test]
fn duplicate_bins_take_the_t_zero_path_instead_of_dividing_by_zero() {
    let g = TableGrid {
        x_bins: vec![1000.0, 1000.0],
        y_bins: vec![20.0, 40.0],
        z: vec![10.0, 20.0, 30.0, 40.0],
    };
    assert_eq!(g.lookup(1000.0, 20.0), Some(10.0));
}
