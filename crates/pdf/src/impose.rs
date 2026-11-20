//! N-up imposition geometry for press sheets.
//!
//! Pure math, no PDF output: [`impose_grid`] lays copies of one page size out on a sheet and
//! returns, for every cell, the trim box and the clip box (trim plus bleed, limited by the
//! neighbouring cells and the sheet edge). Units are points; coordinates are y down with the
//! origin at the sheet's top-left corner.

use designcraft_geom::Rect;

/// Tolerance (points) used when comparing lengths.
const TOL: f64 = 1e-6;

/// Largest grid this module lays out. Larger requests fail with [`ImposeError::TooManyCells`]
/// before any cell is allocated.
pub const MAX_CELLS: usize = 10_000;

/// Whether the pages are turned 90 degrees clockwise on the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotate {
    /// Use whichever orientation places more cells; a tie keeps the pages upright.
    Auto,
    /// Pages upright.
    Off,
    /// Pages turned 90 degrees clockwise.
    On,
}

/// Imposition parameters. All lengths are points.
#[derive(Debug, Clone, PartialEq)]
pub struct ImposeOptions {
    /// Sheet width and height.
    pub sheet: [f64; 2],
    /// Keep-free border of the sheet: top, right, bottom, left.
    pub margins: [f64; 4],
    /// Horizontal gap between columns, vertical gap between rows (between trim boxes).
    pub gap: [f64; 2],
    /// Page bleed: top, right, bottom, left, in the page's own orientation.
    pub bleed: [f64; 4],
    /// Orientation of the pages on the sheet.
    pub rotate: Rotate,
    /// Centre the grid in the usable area; otherwise place it at the top-left of that area.
    pub center: bool,
}

/// One copy of the page on the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// Trim box of the page on the sheet.
    pub trim: Rect,
    /// Trim box expanded by the bleed, limited toward neighbours and the sheet edge.
    pub clip: Rect,
    /// Zero-based column.
    pub column: u32,
    /// Zero-based row.
    pub row: u32,
}

/// The grid laid out on one sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Imposition {
    /// Number of columns.
    pub columns: u32,
    /// Number of rows.
    pub rows: u32,
    /// Whether the pages are turned 90 degrees clockwise.
    pub rotated: bool,
    /// Cells in row-major order (row 0 left to right, then row 1, ...).
    pub cells: Vec<Cell>,
}

/// Why an imposition could not be laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImposeError {
    /// A value is not finite, negative where it must not be, or zero where it must be positive.
    /// The payload names the offending parameter.
    InvalidValue(&'static str),
    /// The page does not fit on the usable area in the requested orientation.
    DoesNotFit,
    /// The grid would hold more than [`MAX_CELLS`] cells.
    TooManyCells,
}

impl std::fmt::Display for ImposeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidValue(name) => write!(f, "invalid imposition value: {name}"),
            Self::DoesNotFit => write!(f, "the page does not fit on the sheet"),
            Self::TooManyCells => write!(f, "the imposition would exceed {MAX_CELLS} cells"),
        }
    }
}

impl std::error::Error for ImposeError {}

impl Imposition {
    /// Number of copies placed on one sheet.
    pub fn per_sheet(&self) -> usize {
        self.cells.len()
    }

    /// Number of sheets needed for `copies` copies (rounded up). Zero copies need no sheets.
    pub fn sheets_for(&self, copies: usize) -> usize {
        let per = self.per_sheet();
        if per == 0 {
            return 0;
        }
        copies.div_ceil(per)
    }

    /// Distinct cut positions: the x values of all trim left and right edges, and the y values
    /// of all trim top and bottom edges, each sorted and merged within 1e-6 points.
    pub fn cut_lines(&self) -> (Vec<f64>, Vec<f64>) {
        let mut xs = Vec::with_capacity(self.cells.len() * 2);
        let mut ys = Vec::with_capacity(self.cells.len() * 2);
        for cell in &self.cells {
            xs.push(cell.trim.x0);
            xs.push(cell.trim.x1);
            ys.push(cell.trim.y0);
            ys.push(cell.trim.y1);
        }
        (sorted_unique(xs), sorted_unique(ys))
    }
}

fn sorted_unique(mut values: Vec<f64>) -> Vec<f64> {
    values.sort_by(f64::total_cmp);
    let mut out: Vec<f64> = Vec::with_capacity(values.len());
    for v in values {
        match out.last() {
            Some(&last) if (v - last).abs() <= TOL => {}
            _ => out.push(v),
        }
    }
    out
}

/// Usable width and height of the sheet inside its margins.
fn usable_size(opts: &ImposeOptions) -> Result<[f64; 2], ImposeError> {
    let m = opts.margins;
    let w = opts.sheet[0] - m[3] - m[1];
    let h = opts.sheet[1] - m[0] - m[2];
    if !(w.is_finite() && w > 0.0 && h.is_finite() && h > 0.0) {
        return Err(ImposeError::InvalidValue("margins"));
    }
    Ok([w, h])
}

fn validate(page: [f64; 2], opts: &ImposeOptions) -> Result<(), ImposeError> {
    let all_finite = |values: &[f64]| values.iter().all(|v| v.is_finite());
    if !all_finite(&page) {
        return Err(ImposeError::InvalidValue("page"));
    }
    if !all_finite(&opts.sheet) {
        return Err(ImposeError::InvalidValue("sheet"));
    }
    if !all_finite(&opts.margins) {
        return Err(ImposeError::InvalidValue("margins"));
    }
    if !all_finite(&opts.gap) {
        return Err(ImposeError::InvalidValue("gap"));
    }
    if !all_finite(&opts.bleed) {
        return Err(ImposeError::InvalidValue("bleed"));
    }
    if !(page[0] > 0.0 && page[1] > 0.0) {
        return Err(ImposeError::InvalidValue("page"));
    }
    if !(opts.sheet[0] > 0.0 && opts.sheet[1] > 0.0) {
        return Err(ImposeError::InvalidValue("sheet"));
    }
    if opts.margins.iter().any(|v| *v < 0.0) {
        return Err(ImposeError::InvalidValue("margins"));
    }
    if opts.gap.iter().any(|v| *v < 0.0) {
        return Err(ImposeError::InvalidValue("gap"));
    }
    if opts.bleed.iter().any(|v| *v < 0.0) {
        return Err(ImposeError::InvalidValue("bleed"));
    }
    usable_size(opts)?;
    Ok(())
}

/// Grid dimensions for one orientation, validated against the cell limit.
#[derive(Debug, Clone, Copy)]
struct Grid {
    cols: usize,
    rows: usize,
    cell_w: f64,
    cell_h: f64,
}

/// Largest count of cells of `cell` length that fit in `usable` with `gap` between them.
fn count_fitting(usable: f64, cell: f64, gap: f64) -> f64 {
    ((usable + gap + TOL) / (cell + gap)).floor()
}

fn grid_for(page: [f64; 2], opts: &ImposeOptions, rotated: bool) -> Result<Grid, ImposeError> {
    let (cell_w, cell_h) = if rotated { (page[1], page[0]) } else { (page[0], page[1]) };
    let [usable_w, usable_h] = usable_size(opts)?;
    let cols = count_fitting(usable_w, cell_w, opts.gap[0]);
    let rows = count_fitting(usable_h, cell_h, opts.gap[1]);
    if !(cols >= 1.0 && rows >= 1.0) {
        return Err(ImposeError::DoesNotFit);
    }
    // Both counts are at least 1 here, so the product bounds each factor; an infinite count
    // makes the product infinite and is rejected too.
    let cells = cols * rows;
    if cells.is_nan() || cells > MAX_CELLS as f64 {
        return Err(ImposeError::TooManyCells);
    }
    Ok(Grid { cols: cols as usize, rows: rows as usize, cell_w, cell_h })
}

/// Places the grid in one orientation and computes every cell's trim and clip boxes.
fn build(opts: &ImposeOptions, rotated: bool, grid: Grid) -> Result<Imposition, ImposeError> {
    let sheet = opts.sheet;
    let m = opts.margins;
    let gap = opts.gap;
    let b = opts.bleed;
    // Bleed in the cell's own orientation. Turning the page 90 degrees clockwise moves the page's
    // left edge to the top, its top to the right, its right to the bottom and its bottom to the left.
    let (bleed_top, bleed_right, bleed_bottom, bleed_left) = if rotated { (b[3], b[0], b[1], b[2]) } else { (b[0], b[1], b[2], b[3]) };

    let [usable_w, usable_h] = usable_size(opts)?;
    let (cols_f, rows_f) = (grid.cols as f64, grid.rows as f64);
    let grid_w = cols_f * grid.cell_w + (cols_f - 1.0) * gap[0];
    let grid_h = rows_f * grid.cell_h + (rows_f - 1.0) * gap[1];
    let (origin_x, origin_y) =
        if opts.center { (m[3] + (usable_w - grid_w).max(0.0) / 2.0, m[0] + (usable_h - grid_h).max(0.0) / 2.0) } else { (m[3], m[0]) };

    let mut cells = Vec::with_capacity(grid.cols * grid.rows);
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let x = origin_x + col as f64 * (grid.cell_w + gap[0]);
            let y = origin_y + row as f64 * (grid.cell_h + gap[1]);
            let right = x + grid.cell_w;
            let bottom = y + grid.cell_h;

            // Toward a neighbour: half the gap. Toward the sheet edge: up to the edge itself.
            let left_ext = if col > 0 { bleed_left.min(gap[0] / 2.0) } else { bleed_left.min(x.max(0.0)) };
            let right_ext = if col + 1 < grid.cols { bleed_right.min(gap[0] / 2.0) } else { bleed_right.min((sheet[0] - right).max(0.0)) };
            let top_ext = if row > 0 { bleed_top.min(gap[1] / 2.0) } else { bleed_top.min(y.max(0.0)) };
            let bottom_ext = if row + 1 < grid.rows { bleed_bottom.min(gap[1] / 2.0) } else { bleed_bottom.min((sheet[1] - bottom).max(0.0)) };

            let trim = Rect::new(x, y, right, bottom);
            let clip = Rect::new(x - left_ext, y - top_ext, right + right_ext, bottom + bottom_ext);
            let finite = [trim.x0, trim.y0, trim.x1, trim.y1, clip.x0, clip.y0, clip.x1, clip.y1].iter().all(|v| v.is_finite());
            if !finite {
                return Err(ImposeError::InvalidValue("sheet"));
            }
            let column = u32::try_from(col).map_err(|_| ImposeError::TooManyCells)?;
            let row_index = u32::try_from(row).map_err(|_| ImposeError::TooManyCells)?;
            cells.push(Cell { trim, clip, column, row: row_index });
        }
    }

    Ok(Imposition {
        columns: u32::try_from(grid.cols).map_err(|_| ImposeError::TooManyCells)?,
        rows: u32::try_from(grid.rows).map_err(|_| ImposeError::TooManyCells)?,
        rotated,
        cells,
    })
}

/// Lays copies of a page of size `page` (width, height) out on a press sheet.
pub fn impose_grid(page: [f64; 2], opts: &ImposeOptions) -> Result<Imposition, ImposeError> {
    validate(page, opts)?;
    let (rotated, grid) = match opts.rotate {
        Rotate::Off => (false, grid_for(page, opts, false)?),
        Rotate::On => (true, grid_for(page, opts, true)?),
        Rotate::Auto => {
            let upright = grid_for(page, opts, false);
            let turned = grid_for(page, opts, true);
            match (upright, turned) {
                (Ok(a), Ok(b)) => {
                    if b.cols * b.rows > a.cols * a.rows {
                        (true, b)
                    } else {
                        (false, a)
                    }
                }
                (Err(ImposeError::DoesNotFit), Ok(b)) => (true, b),
                (Ok(a), Err(ImposeError::DoesNotFit)) => (false, a),
                (Err(e), Err(f)) => return Err(if e == ImposeError::DoesNotFit { f } else { e }),
                (Err(e), Ok(_)) | (Ok(_), Err(e)) => return Err(e),
            }
        }
    };
    build(opts, rotated, grid)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MM: f64 = 72.0 / 25.4;
    const A4: [f64; 2] = [210.0 * MM, 297.0 * MM];
    const PAGE: [f64; 2] = [88.0 * MM, 60.0 * MM];

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn opts(sheet: [f64; 2], margins: [f64; 4], gap: [f64; 2], bleed: [f64; 4], rotate: Rotate, center: bool) -> ImposeOptions {
        ImposeOptions { sheet, margins, gap, bleed, rotate, center }
    }

    fn cell_at(imp: &Imposition, column: u32, row: u32) -> Option<&Cell> {
        imp.cells.iter().find(|c| c.column == column && c.row == row)
    }

    #[test]
    fn plain_layout_orientations() {
        let base = |rotate| opts(A4, [0.0; 4], [0.0; 2], [0.0; 4], rotate, false);

        let auto = impose_grid(PAGE, &base(Rotate::Auto)).expect("auto");
        assert_eq!(auto.cells.len(), 9);
        assert!(auto.rotated);
        assert_eq!((auto.columns, auto.rows), (3, 3));

        let off = impose_grid(PAGE, &base(Rotate::Off)).expect("off");
        assert_eq!(off.cells.len(), 8);
        assert!(!off.rotated);
        assert_eq!((off.columns, off.rows), (2, 4));

        let on = impose_grid(PAGE, &base(Rotate::On)).expect("on");
        assert_eq!(on.cells.len(), 9);
        assert!(on.rotated);
    }

    #[test]
    fn margins_gap_and_bleed_change_the_count() {
        let o = |rotate| opts(A4, [5.0 * MM; 4], [6.0 * MM; 2], [3.0 * MM; 4], rotate, false);
        let off = impose_grid(PAGE, &o(Rotate::Off)).expect("off");
        assert_eq!(off.cells.len(), 8);
        assert_eq!((off.columns, off.rows), (2, 4));
        let auto = impose_grid(PAGE, &o(Rotate::Auto)).expect("auto");
        assert_eq!(auto.cells.len(), 9);
        assert!(auto.rotated);
        assert_eq!((auto.columns, auto.rows), (3, 3));
    }

    #[test]
    fn clip_is_limited_by_gap_and_sheet_edge() {
        let o = |margin: f64, gap: f64| opts(A4, [margin * MM; 4], [gap * MM; 2], [3.0 * MM; 4], Rotate::Off, false);

        // Gap 6 mm: an inner edge reaches the full 3 mm bleed (half the gap is 3 mm).
        let wide = impose_grid(PAGE, &o(5.0, 6.0)).expect("wide gap");
        let first = cell_at(&wide, 0, 0).expect("cell 0,0");
        let second = cell_at(&wide, 1, 0).expect("cell 1,0");
        assert!(close(first.clip.x1 - first.trim.x1, 3.0 * MM));
        assert!(close(second.trim.x0 - second.clip.x0, 3.0 * MM));

        // Gap 2 mm: the inner edge stops at half the gap, 1 mm.
        let narrow = impose_grid(PAGE, &o(5.0, 2.0)).expect("narrow gap");
        let first = cell_at(&narrow, 0, 0).expect("cell 0,0");
        assert!(close(first.clip.x1 - first.trim.x1, 1.0 * MM));

        // Margin 5 mm: the outer edge takes the full 3 mm.
        assert!(close(first.trim.x0 - first.clip.x0, 3.0 * MM));

        // Margin 0 without centring: the outer clip stops at the sheet edge.
        let flush = impose_grid(PAGE, &o(0.0, 6.0)).expect("flush");
        let first = cell_at(&flush, 0, 0).expect("cell 0,0");
        assert!(close(first.trim.x0, 0.0));
        assert!(close(first.clip.x0, 0.0));
    }

    #[test]
    fn rotated_bleed_maps_page_sides_to_cell_sides() {
        // Asymmetric bleed [top, right, bottom, left] = [1, 2, 3, 4] pt. Large gaps and margins
        // keep every interior clip unlimited, so the bleed shows at full size.
        let o = |rotate| opts([500.0, 500.0], [30.0; 4], [50.0; 2], [1.0, 2.0, 3.0, 4.0], rotate, false);
        let page = [100.0, 60.0];

        let extents = |imp: &Imposition| {
            let cell = cell_at(imp, 1, 1).expect("interior cell");
            [cell.trim.y0 - cell.clip.y0, cell.clip.x1 - cell.trim.x1, cell.clip.y1 - cell.trim.y1, cell.trim.x0 - cell.clip.x0]
        };

        let upright = impose_grid(page, &o(Rotate::Off)).expect("upright");
        let got = extents(&upright);
        for (g, want) in got.iter().zip([1.0, 2.0, 3.0, 4.0]) {
            assert!(close(*g, want), "upright extents {got:?}");
        }

        let turned = impose_grid(page, &o(Rotate::On)).expect("turned");
        let got = extents(&turned);
        for (g, want) in got.iter().zip([4.0, 1.0, 2.0, 3.0]) {
            assert!(close(*g, want), "turned extents {got:?}");
        }
    }

    #[test]
    fn centering_splits_leftover_space_equally() {
        let imp = impose_grid(PAGE, &opts(A4, [0.0; 4], [0.0; 2], [0.0; 4], Rotate::Off, true)).expect("centred");
        let first = cell_at(&imp, 0, 0).expect("first");
        let last = imp.cells.last().expect("last");
        let left = first.trim.x0;
        let right = A4[0] - last.trim.x1;
        assert!(left > 0.0);
        assert!(close(left, right));
    }

    #[test]
    fn sheets_for_rounds_up() {
        let auto = impose_grid(PAGE, &opts(A4, [0.0; 4], [0.0; 2], [0.0; 4], Rotate::Auto, false)).expect("auto");
        assert_eq!(auto.per_sheet(), 9);
        assert_eq!(auto.sheets_for(150), 17);
        assert_eq!(auto.sheets_for(0), 0);

        let off = impose_grid(PAGE, &opts(A4, [0.0; 4], [0.0; 2], [0.0; 4], Rotate::Off, false)).expect("off");
        assert_eq!(off.per_sheet(), 8);
        assert_eq!(off.sheets_for(150), 19);
    }

    #[test]
    fn cut_lines_are_deduplicated() {
        let square = [90.0, 90.0];
        let sheet = [300.0, 300.0];

        let tight = impose_grid(square, &opts(sheet, [0.0; 4], [0.0; 2], [0.0; 4], Rotate::Off, false)).expect("tight");
        assert_eq!((tight.columns, tight.rows), (3, 3));
        let (xs, ys) = tight.cut_lines();
        assert_eq!(xs.len(), 4);
        assert_eq!(ys.len(), 4);

        let spaced = impose_grid(square, &opts(sheet, [0.0; 4], [6.0; 2], [0.0; 4], Rotate::Off, false)).expect("spaced");
        let (xs, ys) = spaced.cut_lines();
        assert_eq!(xs.len(), 6);
        assert_eq!(ys.len(), 6);
    }

    #[test]
    fn invalid_input_is_reported() {
        let base = || opts(A4, [0.0; 4], [0.0; 2], [0.0; 4], Rotate::Off, false);

        // Page larger than the sheet in either orientation.
        let big_page = [300.0 * MM, 100.0 * MM];
        assert_eq!(impose_grid(big_page, &base()), Err(ImposeError::DoesNotFit));
        let auto = opts(A4, [0.0; 4], [0.0; 2], [0.0; 4], Rotate::Auto, false);
        assert_eq!(impose_grid(big_page, &auto), Err(ImposeError::DoesNotFit));

        // NaN in any field.
        assert!(matches!(impose_grid([f64::NAN, 10.0], &base()), Err(ImposeError::InvalidValue(_))));
        let mut o = base();
        o.sheet[1] = f64::NAN;
        assert!(matches!(impose_grid(PAGE, &o), Err(ImposeError::InvalidValue(_))));
        let mut o = base();
        o.margins[2] = f64::NAN;
        assert!(matches!(impose_grid(PAGE, &o), Err(ImposeError::InvalidValue(_))));
        let mut o = base();
        o.bleed[0] = f64::INFINITY;
        assert!(matches!(impose_grid(PAGE, &o), Err(ImposeError::InvalidValue(_))));

        // Negative gap.
        let mut o = base();
        o.gap[0] = -1.0;
        assert_eq!(impose_grid(PAGE, &o), Err(ImposeError::InvalidValue("gap")));

        // Zero page.
        assert_eq!(impose_grid([0.0, 10.0], &base()), Err(ImposeError::InvalidValue("page")));
    }

    #[test]
    fn huge_grids_are_refused_before_allocation() {
        let o = opts([14400.0, 14400.0], [0.0; 4], [0.0; 2], [0.0; 4], Rotate::Off, false);
        assert_eq!(impose_grid([0.001, 0.001], &o), Err(ImposeError::TooManyCells));
        let auto = opts([14400.0, 14400.0], [0.0; 4], [0.0; 2], [0.0; 4], Rotate::Auto, false);
        assert_eq!(impose_grid([0.001, 0.001], &auto), Err(ImposeError::TooManyCells));
    }
}
