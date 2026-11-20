//! File > Export Imposed PDF and its layout query.

use designcraft_geom::units::PT_PER_MM;
use designcraft_pdf::impose::{ImposeOptions, Rotate, impose_grid};
use designcraft_pdf::{ImposedOptions, MarkStyle};

use super::pdf::pages_param;
use super::{EngineError, Result, Session, Value, bad, json, str_param};

fn num_list<const N: usize>(id: &str, p: &Value, key: &str) -> Result<Option<[f64; N]>> {
    let Some(v) = p.get(key).filter(|v| !v.is_null()) else { return Ok(None) };
    let a = v.as_array().filter(|a| a.len() == N).ok_or_else(|| bad(id, format!("`{key}` must be {N} numbers")))?;
    let mut out = [0.0; N];
    for (o, x) in out.iter_mut().zip(a) {
        *o = x.as_f64().filter(|n| n.is_finite() && *n >= 0.0).ok_or_else(|| bad(id, format!("`{key}` must be non-negative numbers")))?;
    }
    Ok(Some(out))
}

fn bool_param(id: &str, p: &Value, key: &str, default: bool) -> Result<bool> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(v) => Err(bad(id, format!("bad `{key}`: {v}"))),
    }
}

/// The options and the page size the layout is for.
fn parse(s: &mut Session, id: &str, p: &Value) -> Result<(ImposedOptions, [f64; 2])> {
    let st = s.doc()?;
    let d = &st.doc;
    let n = d.page_count();
    let pages = pages_param(id, p, n)?.unwrap_or_else(|| (0..n).collect());
    let first = pages.first().copied().ok_or_else(|| bad(id, "no pages"))?;
    let (si, pi) = d.page_loc(first).ok_or_else(|| bad(id, "no such page"))?;
    let page = d.spreads.get(si).and_then(|sp| sp.pages.get(pi)).ok_or_else(|| bad(id, "no such page"))?;
    let page_size = [page.width, page.height];
    let sheet = num_list::<2>(id, p, "sheet")?.filter(|v| v[0] > 0.0 && v[1] > 0.0).ok_or_else(|| bad(id, "`sheet` [w, h] in points is required"))?;
    let margins = num_list::<4>(id, p, "margins")?.unwrap_or([PT_PER_MM * 10.0; 4]);
    let use_bleed = bool_param(id, p, "bleed", true)?;
    // Document bleed is [top, bottom, inside, outside]; pages are placed as right-hand pages
    // (inside = left, outside = right), so facing-page documents bleed the same on every cell.
    let db = d.settings.bleed.map(|v| v.max(0.0));
    let bleed = if use_bleed { [db[0], db[3], db[1], db[2]] } else { [0.0; 4] };
    let gap = num_list::<2>(id, p, "gap")?.unwrap_or_else(|| {
        let g = 2.0 * bleed.iter().copied().fold(0.0, f64::max);
        [g, g]
    });
    let rotate = match str_param(p, "rotate").unwrap_or("auto") {
        "auto" => Rotate::Auto,
        "off" => Rotate::Off,
        "on" => Rotate::On,
        r => return Err(bad(id, format!("unknown rotate `{r}` (auto, off, on)"))),
    };
    let center = bool_param(id, p, "center", true)?;
    let copies = match p.get("copies") {
        None | Some(Value::Null) => 1,
        Some(v) => v
            .as_u64()
            .and_then(|c| usize::try_from(c).ok())
            .filter(|c| *c >= 1)
            .ok_or_else(|| bad(id, "`copies` must be a whole number of at least 1"))?,
    };
    let marks = match str_param(p, "marks").unwrap_or("sheet") {
        "none" => MarkStyle::None,
        "sheet" => MarkStyle::Sheet,
        "each" => MarkStyle::Each,
        r => return Err(bad(id, format!("unknown marks `{r}` (none, sheet, each)"))),
    };
    let mut o = ImposedOptions::new(ImposeOptions { sheet, margins, gap, bleed, rotate, center });
    o.pages = pages;
    o.copies = copies;
    o.marks = marks;
    Ok((o, page_size))
}

pub(super) fn export_imposed(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.exportImposed";
    let (opts, _) = parse(s, ID, p)?;
    let st = s.doc()?;
    let r = designcraft_pdf::export_imposed(&crate::cmd::interchange::with_pdf_boxes(&st.doc), &s.cache, &opts)
        .map_err(|e| EngineError::Other(e.to_string()))?;
    match str_param(p, "path") {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            crate::io::write_file(path, &r.bytes).map_err(EngineError::Other)?;
            Ok(json!({"path": path, "bytes": r.bytes.len(), "sheets": r.pages, "warnings": r.warnings}))
        }
        None => Ok(json!({"base64": crate::cmd::file::base64_encode(&r.bytes), "bytes": r.bytes.len(), "sheets": r.pages, "warnings": r.warnings})),
    }
}

pub(super) fn impose_layout(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.imposeLayout";
    let (opts, page) = parse(s, ID, p)?;
    let imp = impose_grid(page, &opts.layout).map_err(|e| bad(ID, e.to_string()))?;
    let total = opts.pages.len().saturating_mul(opts.copies);
    let rect = |r: designcraft_geom::Rect| json!([r.x0, r.y0, r.width(), r.height()]);
    let cells: Vec<Value> = imp.cells.iter().map(|c| json!({"trim": rect(c.trim), "clip": rect(c.clip), "column": c.column, "row": c.row})).collect();
    let segs: Vec<Value> = designcraft_pdf::mark_segments(&imp, opts.layout.sheet, opts.marks, opts.mark_offset, opts.mark_weight)
        .iter()
        .map(|[a, b]| json!([a.0, a.1, b.0, b.1]))
        .collect();
    Ok(json!({
        "columns": imp.columns, "rows": imp.rows, "rotated": imp.rotated,
        "perSheet": imp.per_sheet(), "sheets": imp.sheets_for(total), "placements": total,
        "cells": cells, "marks": segs,
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn layout_and_export() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 5})).unwrap();
        // Letter pages (612 x 792) on a 1300 x 1700 sheet, no bleed: 2 x 2.
        let q = json!({"sheet": [1300, 1700], "margins": [20, 20, 20, 20], "bleed": false, "rotate": "off"});
        let l = s.execute("file.imposeLayout", &q).unwrap();
        assert_eq!((l["columns"].as_u64(), l["rows"].as_u64(), l["perSheet"].as_u64(), l["sheets"].as_u64()), (Some(2), Some(2), Some(4), Some(2)));
        assert_eq!(l["cells"].as_array().unwrap().len(), 4);
        let mut e = q.clone();
        e["pages"] = json!("1-2");
        e["copies"] = json!(2);
        e["marks"] = json!("each");
        let r = s.execute("file.exportImposed", &e).unwrap();
        assert_eq!(r["sheets"], 1);
        assert!(r["bytes"].as_u64().unwrap() > 100);
    }

    #[test]
    fn strict_params() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let ok = json!({"sheet": [1300, 1700]});
        for bad in [
            json!({}),
            json!({"sheet": [0, 10]}),
            json!({"sheet": [1300, 1700], "rotate": "sideways"}),
            json!({"sheet": [1300, 1700], "marks": "all"}),
            json!({"sheet": [1300, 1700], "copies": 0}),
            json!({"sheet": [1300, 1700], "margins": [1, 2]}),
            json!({"sheet": [1300, 1700], "gap": [-1, 0]}),
            json!({"sheet": [1300, 1700], "pages": "9"}),
        ] {
            assert!(s.execute("file.imposeLayout", &bad).is_err(), "{bad}");
            assert!(s.execute("file.exportImposed", &bad).is_err(), "{bad}");
        }
        assert!(s.execute("file.imposeLayout", &ok).is_ok());
        // Nothing fits.
        assert!(s.execute("file.exportImposed", &json!({"sheet": [300, 300]})).is_err());
    }
}
