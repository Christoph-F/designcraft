//! File > Export Imposed PDF: copies of document pages tiled onto press sheets (N-up), with
//! sheet-level or per-cell cut marks. Geometry comes from [`crate::impose`].

use super::common::{tf, to_path};
use super::images::ImageContentCache;
use super::metadata::file_info_metadata;
use super::pipeline::{MARK_LEN, print_color};
use super::*;
use crate::impose::{ImposeOptions, Imposition, impose_grid};

/// Most page placements one export accepts.
pub const MAX_PLACEMENTS: usize = 100_000;

/// Which cut marks to draw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkStyle {
    None,
    /// Short lines in the sheet margins at every cut position.
    #[default]
    Sheet,
    /// Corner crop marks around every cell.
    Each,
}

#[derive(Clone, Debug)]
pub struct ImposedOptions {
    pub layout: ImposeOptions,
    /// Zero-based pages in order; empty means every page.
    pub pages: Vec<usize>,
    /// Consecutive copies of each page (at least 1).
    pub copies: usize,
    pub marks: MarkStyle,
    /// Mark line weight (points).
    pub mark_weight: f64,
    /// Distance of marks from the clip box (points).
    pub mark_offset: f64,
    pub title: Option<String>,
}

impl ImposedOptions {
    pub fn new(layout: ImposeOptions) -> Self {
        Self { layout, pages: Vec::new(), copies: 1, marks: MarkStyle::Sheet, mark_weight: 0.25, mark_offset: 6.0, title: None }
    }
}

type Seg = [(f64, f64); 2];

/// Mark segments (sheet coordinates, y down) that stay clear of every cell's clip box.
pub fn mark_segments(imp: &Imposition, sheet: [f64; 2], style: MarkStyle, offset: f64, weight: f64) -> Vec<Seg> {
    let mut out: Vec<Seg> = Vec::new();
    let off = offset.max(0.0);
    let half = weight.max(0.05) / 2.0;
    let clear = |a: (f64, f64), b: (f64, f64)| {
        let (x0, x1) = (a.0.min(b.0) - half, a.0.max(b.0) + half);
        let (y0, y1) = (a.1.min(b.1) - half, a.1.max(b.1) + half);
        imp.cells.iter().all(|c| x1 <= c.clip.x0 || x0 >= c.clip.x1 || y1 <= c.clip.y0 || y0 >= c.clip.y1)
    };
    let inside = |p: (f64, f64)| p.0 >= 0.0 && p.0 <= sheet[0] && p.1 >= 0.0 && p.1 <= sheet[1];
    match style {
        MarkStyle::None => {}
        MarkStyle::Sheet => {
            let (xs, ys) = imp.cut_lines();
            let top = imp.cells.iter().map(|c| c.clip.y0).fold(f64::INFINITY, f64::min) - off;
            let bottom = imp.cells.iter().map(|c| c.clip.y1).fold(f64::NEG_INFINITY, f64::max) + off;
            let left = imp.cells.iter().map(|c| c.clip.x0).fold(f64::INFINITY, f64::min) - off;
            let right = imp.cells.iter().map(|c| c.clip.x1).fold(f64::NEG_INFINITY, f64::max) + off;
            let mut push = |a: (f64, f64), b: (f64, f64)| {
                if inside(a) && inside(b) && clear(a, b) {
                    out.push([a, b]);
                }
            };
            for &x in &xs {
                if top > 0.0 {
                    push((x, (top - MARK_LEN).max(0.0)), (x, top));
                }
                if bottom < sheet[1] {
                    push((x, bottom), (x, (bottom + MARK_LEN).min(sheet[1])));
                }
            }
            for &y in &ys {
                if left > 0.0 {
                    push(((left - MARK_LEN).max(0.0), y), (left, y));
                }
                if right < sheet[0] {
                    push((right, y), ((right + MARK_LEN).min(sheet[0]), y));
                }
            }
        }
        MarkStyle::Each => {
            for c in &imp.cells {
                let (t, k) = (c.trim, c.clip);
                for (x, kx, sx) in [(t.x0, k.x0, -1.0), (t.x1, k.x1, 1.0)] {
                    for (y, ky, sy) in [(t.y0, k.y0, -1.0), (t.y1, k.y1, 1.0)] {
                        // A gutter narrower than a full mark gets a shorter one rather than none.
                        for len in [MARK_LEN, 12.0, 9.0, 6.0, 4.0] {
                            let h = ((kx + sx * off, y), (kx + sx * (off + len), y));
                            if inside(h.0) && inside(h.1) && clear(h.0, h.1) {
                                out.push([h.0, h.1]);
                                break;
                            }
                        }
                        for len in [MARK_LEN, 12.0, 9.0, 6.0, 4.0] {
                            let (vs, ve) = (ky + sy * off, ky + sy * (off + len));
                            let v = ((x, vs), (x, ve));
                            if inside(v.0) && inside(v.1) && clear(v.0, v.1) {
                                out.push([v.0, v.1]);
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// An imposed PDF: the selected pages tiled onto sheets, row by row. The grid is laid out for
/// the first selected page's size; other pages are placed at the same trim origin.
pub fn export_imposed(doc: &Document, cache: &Cache, opts: &ImposedOptions) -> Result<ExportReport> {
    write::guard(|| export_imposed_inner(doc, cache, opts))
}

fn export_imposed_inner(doc: &Document, cache: &Cache, opts: &ImposedOptions) -> Result<ExportReport> {
    let n = doc.page_count();
    if n == 0 {
        return Err(PdfError::NoPages);
    }
    let pages: Vec<usize> = if opts.pages.is_empty() { (0..n).collect() } else { opts.pages.clone() };
    if pages.iter().any(|&p| p >= n) {
        return Err(PdfError::Write("page out of range".into()));
    }
    let copies = opts.copies.max(1);
    let total = pages
        .len()
        .checked_mul(copies)
        .filter(|&t| t <= MAX_PLACEMENTS)
        .ok_or_else(|| PdfError::Write(format!("more than {MAX_PLACEMENTS} placements")))?;
    let first = pages.first().and_then(|&p| doc.page_loc(p)).ok_or(PdfError::NoPages)?;
    let (pw, ph) = {
        let p = doc.spreads.get(first.0).and_then(|s| s.pages.get(first.1)).ok_or(PdfError::NoPages)?;
        (p.width, p.height)
    };
    let imp = impose_grid([pw, ph], &opts.layout).map_err(|e| PdfError::Write(e.to_string()))?;
    let per = imp.per_sheet();
    if per == 0 {
        return Err(PdfError::Write("the page does not fit on the sheet".into()));
    }
    let sheets = imp.sheets_for(total);

    let pdf_opts = PdfOptions { title: opts.title.clone(), ..PdfOptions::default() };
    let mut warnings = Vec::new();
    let (base, cmyk_icc) = if print_color(doc, &pdf_opts) {
        let (settings, icc, warning) = crate::pdfx::print_settings_and_profile(false)?;
        warnings.extend(warning);
        (settings, Some(icc))
    } else {
        (krilla::SerializeSettings::default(), None)
    };
    let settings = krilla::SerializeSettings { compress_content_streams: true, ..base };
    let metadata = file_info_metadata(doc, opts.title.as_deref(), None);
    let mut writer = Writer::new(settings, metadata);
    let pdf = writer.document_mut();
    let mut ex = Exporter {
        drawing_items: Vec::new(),
        inks: crate::inks::ink_set(doc),
        doc,
        cache,
        composed: HashMap::new(),
        opts: &pdf_opts,
        clip: Rect::ZERO,
        warnings,
        images: HashMap::new(),
        outputs: HashMap::new(),
        images_by_content: ImageContentCache::default(),
        #[cfg(test)]
        image_conversions: 0,
        pdfs: HashMap::new(),
        variants: HashMap::new(),
        has_optional_content: false,
        #[cfg(test)]
        pdf_preparations: 0,
        layer_error: None,
        version: PdfVersion::Pdf17,
        too_new: Vec::new(),
        svgs: HashMap::new(),
        fonts: HashMap::new(),
        reverse_cmaps: HashMap::new(),
        rgb_only: false,
        tags: Vec::new(),
        story_tags: HashMap::new(),
        tag_story: None,
        text_area: None,
        para_tags: HashMap::new(),
    };
    let size = Size::from_wh(opts.layout.sheet[0] as f32, opts.layout.sheet[1] as f32).ok_or(PdfError::NoPages)?;
    let marks = mark_segments(&imp, opts.layout.sheet, opts.marks, opts.mark_offset, opts.mark_weight);
    let mut bp = BezPath::new();
    for [a, b] in &marks {
        bp.move_to(designcraft_geom::Point::new(a.0, a.1));
        bp.line_to(designcraft_geom::Point::new(b.0, b.1));
    }
    let bleed = opts.layout.bleed;
    for sheet in 0..sheets {
        let mut page = pdf.start_page_with(PageSettings::new(size));
        let mut s = page.surface();
        #[cfg(all(test, not(target_arch = "wasm32")))]
        unwind_tests::inject(&mut s);
        for (slot, cell) in imp.cells.iter().enumerate() {
            let idx = sheet.saturating_mul(per).saturating_add(slot);
            if idx >= total {
                break;
            }
            let abs = pages.get(idx / copies).copied().unwrap_or(0);
            let Some((si, pi)) = doc.page_loc(abs) else { continue };
            let Some(p) = doc.spreads.get(si).and_then(|sp| sp.pages.get(pi)) else { continue };
            let (t, k) = (cell.trim, cell.clip);
            // Page space (origin at the spread, y down) to sheet space; a rotated page is
            // turned 90 degrees clockwise.
            let m = if imp.rotated { Affine::new([0.0, 1.0, -1.0, 0.0, t.x0 + p.height, t.y0 - p.x]) } else { Affine::translate((t.x0 - p.x, t.y0)) };
            if let Some(path) = to_path(&k.to_path(0.1)) {
                s.push_clip_path(&path, &FillRule::NonZero);
                s.push_transform(&tf(m));
                ex.clip = Rect::new(p.x - bleed[3], -bleed[0], p.x + p.width + bleed[1], p.height + bleed[2]);
                ex.spread(&mut s, si);
                s.pop();
                s.pop();
            }
        }
        if let Some(p) = to_path(&bp) {
            s.set_fill(None);
            s.set_stroke(Some(Stroke {
                paint: registration(1.0, ex.rgb_only).into(),
                width: opts.mark_weight.max(0.05) as f32,
                miter_limit: 4.0,
                line_cap: krilla::paint::LineCap::Butt,
                line_join: krilla::paint::LineJoin::Miter,
                opacity: NormalizedF32::ONE,
                dash: None,
            }));
            s.draw_path(&p);
            s.set_stroke(None);
        }
        s.finish();
        page.finish();
    }
    ex.check_placed_versions()?;
    if let Some(error) = ex.layer_error {
        return Err(error);
    }
    let mut bytes = writer.finish().map_err(PdfError::from)?;
    if ex.has_optional_content {
        bytes = crate::optional_content::register(&bytes)?;
    }
    let mut warnings = ex.warnings;
    bytes = crate::default_cmyk::attach(bytes, cmyk_icc.as_deref(), Standard::None, &mut warnings)?;
    warnings.dedup();
    Ok(ExportReport { bytes, pages: sheets, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impose::Rotate;

    fn layout(sheet: [f64; 2]) -> ImposeOptions {
        ImposeOptions { sheet, margins: [20.0; 4], gap: [0.0; 2], bleed: [0.0; 4], rotate: Rotate::Off, center: true }
    }

    fn doc(pages: usize) -> Document {
        Document::new(&designcraft_doc::build::NewDocument { pages, facing_pages: false, ..designcraft_doc::build::NewDocument::default() })
    }

    #[test]
    fn sheets_match_placements() {
        let d = doc(5);
        let p = &d.spreads[0].pages[0];
        let (w, h) = (p.width, p.height);
        let sheet = [w * 2.0 + 40.0, h * 2.0 + 40.0];
        let r = export_imposed(&d, &Cache::new(), &ImposedOptions::new(layout(sheet))).unwrap();
        assert_eq!(r.pages, 2, "5 pages at 4 per sheet");
        let mut o = ImposedOptions::new(layout(sheet));
        o.copies = 4;
        o.pages = vec![0];
        assert_eq!(export_imposed(&d, &Cache::new(), &o).unwrap().pages, 1);
    }

    #[test]
    fn nothing_fits_is_an_error() {
        let d = doc(1);
        assert!(export_imposed(&d, &Cache::new(), &ImposedOptions::new(layout([100.0, 100.0]))).is_err());
        let mut o = ImposedOptions::new(layout([5000.0, 5000.0]));
        o.copies = MAX_PLACEMENTS + 1;
        assert!(export_imposed(&d, &Cache::new(), &o).is_err());
    }

    #[test]
    fn marks_avoid_cells() {
        let o = ImposeOptions { sheet: [400.0, 400.0], margins: [40.0; 4], gap: [20.0; 2], bleed: [0.0; 4], rotate: Rotate::Off, center: true };
        let imp = impose_grid([100.0, 100.0], &o).unwrap();
        for style in [MarkStyle::Sheet, MarkStyle::Each] {
            let segs = mark_segments(&imp, o.sheet, style, 6.0, 0.25);
            assert!(!segs.is_empty());
            for [a, b] in segs {
                for c in &imp.cells {
                    let hit = a.0.max(b.0) > c.clip.x0 && a.0.min(b.0) < c.clip.x1 && a.1.max(b.1) > c.clip.y0 && a.1.min(b.1) < c.clip.y1;
                    assert!(!hit, "{a:?}-{b:?} enters {c:?}");
                }
            }
        }
    }
}
