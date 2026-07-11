// SPDX-License-Identifier: GPL-3.0-or-later
//! The simulator's hidden "true VE" ground-truth surface, and the decode of
//! the *table* VE the engine currently holds in page memory — together these
//! close the M4 auto-tune demo's loop (locked decision 11, task-9 brief):
//!
//! ```text
//! afr = afr_target × true_ve / current_ve
//! ```
//!
//! Where the loaded `veTable` is wrong, `current_ve` differs from
//! [`true_ve`] and the simulated "measured" AFR drifts away from
//! `afrTarget` — exactly the error surface the auto-tune demo (Task 12)
//! must flatten back out. Correcting a cell to
//! `VE_new = VE_old × afr/target = VE_old × true/current` converges to
//! `true_ve` in a single step, by construction.
//!
//! **Written fresh** (no port): `true_ve` is a made-up-for-the-demo affine
//! surface (§ADR-0006 doesn't apply — there is no reference implementation
//! of a "made up ground truth" to port). [`VeContext::current_ve`]'s
//! bilinear lookup **deliberately duplicates** the segment+fraction shape
//! `analysis::grid::TableGrid::lookup` will implement in Task 11 (still a
//! stub returning `None` as of this writing, per `crates/analysis/src/grid.rs`)
//! rather than taking a same-milestone dependency on a crate this one has no
//! other reason to depend on (`opentune-simulator`'s `Cargo.toml` doesn't
//! list `opentune-analysis`, and adding it just for a stub would be a
//! backwards layering anyway) — task-9-brief's own call: "~30 deliberately
//! duplicated lines, the M3 '30 trivial lines over a dependency' precedent".
//! If/when `analysis::grid` stabilizes, this could fold onto it; until then
//! the two copies are intentionally independent.
//!
//! Byte layout convention (this module's own choice — the INI grammar's
//! `[RxC]` shape syntax doesn't attach row/col to a physical axis): a
//! `zBins` array decodes in **row-major, row = Y bins (load), col = X bins
//! (rpm)** order — `ve[row * cols + col]`, i.e. exactly
//! [`VeContext::current_ve`]'s `ve[y * rpm_bins.len() + x]` indexing. This
//! matches the real Speeduino/TunerStudio veTable convention (each row is
//! one load/MAP bin across all RPM columns).

use crate::memory::MemoryImage;
use crate::och_codec;
use opentune_ini::{ConstantDef, ConstantKind, Definition, Endianness, Number, ScalarType};

/// The simulator's hidden "true VE" surface — what the engine actually
/// needs, regardless of what the loaded `veTable` says. Deterministic and
/// cell-dependent: lean error grows with load and rpm (pinned formula,
/// task-9 brief): `40 + 25·(load/100) + 15·(rpm/6000)`, clamped `20..110`.
pub(crate) fn true_ve(rpm: f64, load_kpa: f64) -> f64 {
    let unclamped = 40.0 + 25.0 * (load_kpa / 100.0) + 15.0 * (rpm / 6000.0);
    unclamped.clamp(20.0, 110.0)
}

/// Decoded `veTable` context: physical axis bins + physical cell values,
/// refreshed from the memory image each engine tick (see [`ve_context`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VeContext {
    pub(crate) rpm_bins: Vec<f64>,
    pub(crate) load_bins: Vec<f64>,
    /// Row-major, row = `load_bins` index, col = `rpm_bins` index (module
    /// doc comment) — `ve[y * rpm_bins.len() + x]`.
    pub(crate) ve: Vec<f64>,
}

impl VeContext {
    /// Bilinear current-VE lookup, clamped to the bin range; `None` when the
    /// bins are empty or `ve`'s length doesn't match `rpm_bins.len() *
    /// load_bins.len()` (shape mismatch — never index out of bounds).
    pub(crate) fn current_ve(&self, rpm: f64, load_kpa: f64) -> Option<f64> {
        let nx = self.rpm_bins.len();
        let ny = self.load_bins.len();
        if nx == 0 || ny == 0 || self.ve.len() != nx * ny {
            return None;
        }
        let (xi, xf) = segment(&self.rpm_bins, rpm);
        let (yi, yf) = segment(&self.load_bins, load_kpa);
        let xi2 = (xi + 1).min(nx - 1);
        let yi2 = (yi + 1).min(ny - 1);
        let cell = |x: usize, y: usize| self.ve[y * nx + x];
        let top = cell(xi, yi) + (cell(xi2, yi) - cell(xi, yi)) * xf;
        let bottom = cell(xi, yi2) + (cell(xi2, yi2) - cell(xi, yi2)) * xf;
        Some(top + (bottom - top) * yf)
    }
}

/// Locate `v`'s bracketing segment in ascending `bins`: the lower index and
/// the fractional position within that segment. Out-of-range `v` clamps to
/// the first/last segment with fraction 0.0/1.0 (never extrapolates).
fn segment(bins: &[f64], v: f64) -> (usize, f64) {
    let last = bins.len() - 1;
    if last == 0 || v <= bins[0] {
        return (0, 0.0);
    }
    if v >= bins[last] {
        return (last - 1, 1.0);
    }
    for i in 0..last {
        if v <= bins[i + 1] {
            let span = bins[i + 1] - bins[i];
            let frac = if span > 0.0 {
                (v - bins[i]) / span
            } else {
                0.0
            };
            return (i, frac);
        }
    }
    (last - 1, 1.0) // unreachable given the bounds checks above; graceful fallback.
}

/// Resolve + decode the VE table the INI's `[VeAnalyze]` map points at:
/// `def.ve_analyze.maps[0].table` → its `[TableEditor]` `TableDef` → the
/// `x_bins`/`y_bins`/`z` constants it names → their raw bytes in `memory`,
/// decoded into physical values. `None` when the INI declares no
/// `[VeAnalyze]`, its map's table isn't declared under `[TableEditor]`, any
/// of its `xBins`/`yBins`/`zBins` names isn't a known constant, or the
/// decode itself fails (unknown constant kind, an `Expr` scale/translate
/// not resolvable here, or a page read shorter than the array's declared
/// shape) — fail-open at every step, never a panic. `crate::ecu` calls this
/// once per engine tick, straight off its retained [`Definition`]; the
/// lookups involved are small linear scans, cheap enough to redo every tick
/// rather than caching a resolved binding.
pub(crate) fn ve_context(def: &Definition, memory: &MemoryImage) -> Option<VeContext> {
    let map = def.ve_analyze.as_ref()?.maps.first()?;
    let table = def.table(&map.table)?;
    let endian = def.comms.endianness;
    Some(VeContext {
        rpm_bins: decode_array(def.constant(&table.x_bins)?, endian, memory)?,
        load_bins: decode_array(def.constant(&table.y_bins)?, endian, memory)?,
        ve: decode_array(def.constant(&table.z)?, endian, memory)?,
    })
}

/// Decode one array-kind constant's current raw bytes into physical values,
/// preserving the array's declared row-major order (module doc comment).
/// `physical = raw * scale + translate` per element (`ConstantDef::scale`/
/// `translate`); a `Number::Expr` scale/translate can't be resolved here
/// (no expression evaluator in this crate) and fails the whole array open
/// (`None`) rather than guessing.
fn decode_array(
    constant: &ConstantDef,
    endian: Endianness,
    memory: &MemoryImage,
) -> Option<Vec<f64>> {
    let ConstantKind::Array { elem, shape } = &constant.kind else {
        return None;
    };
    let scale = literal(&constant.scale)?;
    let translate = literal(&constant.translate)?;
    let width = och_codec::width(*elem);
    let count = shape.rows * shape.cols;
    let offset = u16::try_from(constant.offset).ok()?;
    let byte_len = u16::try_from(count * width).ok()?;
    let raw = memory.read(constant.page, offset, byte_len);
    if raw.len() != count * width {
        return None;
    }
    Some(
        raw.chunks_exact(width)
            .map(|chunk| decode_scalar(chunk, *elem, endian) * scale + translate)
            .collect(),
    )
}

fn literal(n: &Number) -> Option<f64> {
    match n {
        Number::Lit(v) => Some(*v),
        Number::Expr(_) => None,
    }
}

/// Decode one scalar element's raw bytes into its unscaled numeric value
/// (the inverse of `och_codec`'s scalar write path).
fn decode_scalar(bytes: &[u8], elem: ScalarType, endian: Endianness) -> f64 {
    match elem {
        ScalarType::U08 => f64::from(bytes[0]),
        ScalarType::S08 => f64::from(bytes[0] as i8),
        ScalarType::U16 => f64::from(read_u16(bytes, endian)),
        ScalarType::S16 => f64::from(read_u16(bytes, endian) as i16),
        ScalarType::U32 => f64::from(read_u32(bytes, endian)),
        ScalarType::S32 => f64::from(read_u32(bytes, endian) as i32),
        ScalarType::F32 => f64::from(match endian {
            Endianness::Little => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            Endianness::Big => f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        }),
    }
}

fn read_u16(bytes: &[u8], endian: Endianness) -> u16 {
    let arr = [bytes[0], bytes[1]];
    match endian {
        Endianness::Little => u16::from_le_bytes(arr),
        Endianness::Big => u16::from_be_bytes(arr),
    }
}

fn read_u32(bytes: &[u8], endian: Endianness) -> u32 {
    let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
    match endian {
        Endianness::Little => u32::from_le_bytes(arr),
        Endianness::Big => u32::from_be_bytes(arr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentune_ini::parse_definition;

    // ── 9.2: true_ve + VeContext::current_ve (hand-built, no Definition) ────

    #[test]
    fn true_ve_matches_the_pinned_formula_before_clamping() {
        assert_eq!(true_ve(6000.0, 100.0), 80.0);
        assert_eq!(true_ve(800.0, 30.0), 49.5);
    }

    #[test]
    fn true_ve_clamps_to_the_documented_range() {
        assert_eq!(true_ve(0.0, 0.0), 40.0, "within range already, no clamping");
        assert_eq!(
            true_ve(60_000.0, 100.0),
            110.0,
            "must clamp at the high end"
        );
        assert_eq!(true_ve(0.0, -1_000.0), 20.0, "must clamp at the low end");
    }

    fn hand_built_context() -> VeContext {
        VeContext {
            rpm_bins: vec![1000.0, 2000.0],
            load_bins: vec![20.0, 40.0],
            ve: vec![50.0, 60.0, 70.0, 80.0],
        }
    }

    #[test]
    fn current_ve_bilinear_interpolates_between_the_four_corners() {
        let ctx = hand_built_context();
        assert_eq!(ctx.current_ve(1500.0, 30.0), Some(65.0));
    }

    #[test]
    fn current_ve_clamps_out_of_range_operating_points_to_the_edge() {
        let ctx = hand_built_context();
        assert_eq!(ctx.current_ve(500.0, 10.0), Some(50.0));
    }

    #[test]
    fn current_ve_is_none_on_a_wrong_length_ve_array() {
        let ctx = VeContext {
            rpm_bins: vec![1000.0, 2000.0],
            load_bins: vec![20.0, 40.0],
            ve: vec![50.0, 60.0, 70.0], // should be 4 (2×2)
        };
        assert_eq!(ctx.current_ve(1500.0, 30.0), None);
    }

    // ── 9.3: ve_context (real Definition + MemoryImage) ─────────────────────

    /// A minimal INI: one page holding a 2×2 `veTable` + its bins, bound
    /// through `[TableEditor]`/`[VeAnalyze]` — just enough for [`ve_context`]
    /// to have something real to resolve against, independent of the full
    /// sample INI (kept separate so this unit test doesn't churn every time
    /// the sample INI changes).
    const VE_TEST_INI: &str = r#"
[MegaTune]
   signature            = "speeduino 202504-dev"
   queryCommand         = "Q"
   versionInfo          = "S"
   blockReadTimeout     = 2000
   blockingFactor       = 121
   endianness           = little
   ochGetCommand        = "A"
   pageReadCommand      = "p%2i%2o%2c"
   pageValueWrite       = "M%2i%2o%2c%v"
   burnCommand          = "b%2i"

[Constants]
    endianness      = little
    nPages          = 1
    pageSize        = 8

page = 1
      veTable      = array, U08, 0, [2x2], "%",   1.0,   0.0, 0.0, 255.0, 0
      rpmBins      = array, U08, 4, [2],   "RPM", 100.0, 0.0, 0.0, 255.0, 0
      fuelLoadBins = array, U08, 6, [2],   "kPa", 1.0,   0.0, 0.0, 255.0, 0

[TableEditor]
   table = veTable1Tbl, veTable1Map, "VE Table", 1
      xBins = rpmBins, rpm
      yBins = fuelLoadBins, fuelLoad
      zBins = veTable

[VeAnalyze]
   veAnalyzeMap = veTable1Tbl, afrTable1Tbl, afr, egoCorrection
"#;

    fn ve_test_definition() -> Definition {
        parse_definition(VE_TEST_INI).expect("VE_TEST_INI must parse")
    }

    #[test]
    fn ve_context_decodes_bins_and_cells_from_memory() {
        let def = ve_test_definition();
        assert!(
            def.diagnostics.is_empty(),
            "test fixture must parse clean: {:?}",
            def.diagnostics
        );
        let mut memory = MemoryImage::new(&def.pages);
        memory.write(1, 0, &[50, 60, 70, 80]); // veTable, row-major (load × rpm)
        memory.write(1, 4, &[10, 20]); // rpmBins raw ×100 → 1000, 2000
        memory.write(1, 6, &[20, 40]); // fuelLoadBins raw ×1 → 20, 40

        let ctx = ve_context(&def, &memory).expect("VeAnalyze binding must resolve");
        assert_eq!(ctx.rpm_bins, vec![1000.0, 2000.0]);
        assert_eq!(ctx.load_bins, vec![20.0, 40.0]);
        assert_eq!(ctx.ve, vec![50.0, 60.0, 70.0, 80.0]);
        assert_eq!(ctx.current_ve(1500.0, 30.0), Some(65.0));
    }

    #[test]
    fn ve_context_is_none_without_a_ve_analyze_section() {
        let ini = VE_TEST_INI.replace("[VeAnalyze]", "[NotVeAnalyze]");
        let def = parse_definition(&ini).expect("modified fixture must still parse");
        let memory = MemoryImage::new(&def.pages);
        assert!(ve_context(&def, &memory).is_none());
    }

    #[test]
    fn ve_context_fails_open_on_an_unresolvable_expr_scale() {
        // A `veTable` scale that is an expression (no evaluator reachable
        // from this crate) must fail the whole decode open, not guess.
        let ini = VE_TEST_INI.replacen(
            "veTable      = array, U08, 0, [2x2], \"%\",   1.0,   0.0, 0.0, 255.0, 0",
            "veTable      = array, U08, 0, [2x2], \"%\", { veScaleExpr }, 0.0, 0.0, 255.0, 0",
            1,
        );
        let def = parse_definition(&ini).expect("modified fixture must still parse");
        let memory = MemoryImage::new(&def.pages);
        assert!(ve_context(&def, &memory).is_none());
    }
}
