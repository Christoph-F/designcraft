//! File > Export Imposed PDF…: sheet, margins, gap, rotation, pages, copies and marks, with a
//! live preview of the layout that `file.imposeLayout` computes.

use serde_json::{Value, json};

use crate::DesignApp;
use crate::dialogs::Dialog;

/// Sheet presets (portrait, millimetres).
const SHEETS: &[(&str, f64, f64)] = &[
    ("SRA3", 320.0, 450.0),
    ("SRA4", 225.0, 320.0),
    ("A3", 297.0, 420.0),
    ("A4", 210.0, 297.0),
    ("B2", 500.0, 707.0),
    ("Tabloid", 279.4, 431.8),
    ("Letter", 215.9, 279.4),
];

pub(crate) fn defaults() -> Value {
    json!({
        "sheet": "SRA3", "w": "320", "h": "450", "landscape": false,
        "margin": "10", "gapx": "", "gapy": "", "rotate": "auto", "center": true,
        "pages": "", "copies": "1", "marks": "sheet", "bleed": true,
    })
}

fn mm(d: &Dialog, k: &str) -> Option<f64> {
    d.s(k).trim().trim_end_matches("mm").trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0)
}

/// The `file.imposeLayout` / `file.exportImposed` parameters the fields describe.
pub(crate) fn params(d: &Dialog) -> Result<Value, String> {
    let pt = designcraft_geom::units::PT_PER_MM;
    let (w, h) = (mm(d, "w").filter(|v| *v > 0.0), mm(d, "h").filter(|v| *v > 0.0));
    let (Some(w), Some(h)) = (w, h) else { return Err("Sheet width and height must be positive numbers".into()) };
    let sheet = if d.b("landscape") { [w.max(h), w.min(h)] } else { [w.min(h), w.max(h)] };
    let margin = mm(d, "margin").ok_or("Margins must be a number")?;
    let m = margin * pt;
    let mut p = json!({
        "sheet": [sheet[0] * pt, sheet[1] * pt],
        "margins": [m, m, m, m],
        "rotate": d.s("rotate"),
        "center": d.b("center"),
        "marks": d.s("marks"),
        "bleed": d.b("bleed"),
    });
    let (gx, gy) = (d.s("gapx"), d.s("gapy"));
    if !gx.trim().is_empty() || !gy.trim().is_empty() {
        let x = mm(d, "gapx").ok_or("Gap must be a number")?;
        let y = mm(d, "gapy").ok_or("Gap must be a number")?;
        p["gap"] = json!([x * pt, y * pt]);
    }
    let pages = d.s("pages");
    if !pages.trim().is_empty() && pages != "all" {
        p["pages"] = json!(pages);
    }
    let copies = d.s("copies");
    let copies: u64 = copies.trim().parse().ok().filter(|c| *c >= 1).ok_or("Copies must be a whole number of at least 1")?;
    p["copies"] = json!(copies);
    Ok(p)
}

pub(crate) fn body(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| crate::i18n::tr(&lang, s);
    egui::Grid::new("impose_fields").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, tr("Sheet:"));
        ui.horizontal(|ui| {
            let cur = d.s("sheet");
            craft_ui::ComboBox::from_id_salt("impose_sheet").selected_text(crate::rtl::widget(ui, &cur)).width(120.0).show_ui(ui, |ui| {
                for &(name, w, h) in SHEETS {
                    if ui.selectable_label(cur == name, crate::rtl::widget(ui, name)).clicked() {
                        d.fields.insert("sheet".into(), json!(name));
                        d.fields.insert("w".into(), json!(format!("{w}")));
                        d.fields.insert("h".into(), json!(format!("{h}")));
                    }
                }
                if ui.selectable_label(cur == "Custom", crate::rtl::widget(ui, tr("Custom"))).clicked() {
                    d.fields.insert("sheet".into(), json!("Custom"));
                }
            });
            for key in ["w", "h"] {
                let before = d.s(key);
                text_field(ui, d, key, 56.0);
                if d.s(key) != before {
                    d.fields.insert("sheet".into(), json!("Custom"));
                }
                if key == "w" {
                    crate::rtl::label(ui, "×");
                }
            }
            crate::rtl::label(ui, "mm");
        });
        ui.end_row();

        crate::rtl::label(ui, tr("Orientation:"));
        ui.horizontal(|ui| {
            let landscape = d.b("landscape");
            if ui.radio(!landscape, crate::rtl::widget(ui, tr("Portrait"))).clicked() {
                d.fields.insert("landscape".into(), json!(false));
            }
            if ui.radio(landscape, crate::rtl::widget(ui, tr("Landscape"))).clicked() {
                d.fields.insert("landscape".into(), json!(true));
            }
        });
        ui.end_row();

        crate::rtl::label(ui, tr("Margins:"));
        ui.horizontal(|ui| {
            text_field(ui, d, "margin", 56.0);
            crate::rtl::label(ui, "mm");
        });
        ui.end_row();

        crate::rtl::label(ui, tr("Gap:"));
        ui.horizontal(|ui| {
            text_field(ui, d, "gapx", 56.0);
            text_field(ui, d, "gapy", 56.0);
            crate::rtl::label(ui, egui::RichText::new(tr("mm (empty: twice the bleed)")).small());
        });
        ui.end_row();

        crate::rtl::label(ui, tr("Rotate:"));
        ui.horizontal(|ui| {
            for (v, label) in [("auto", "Auto"), ("off", "Off"), ("on", "On")] {
                if ui.radio(d.s("rotate") == v, crate::rtl::widget(ui, tr(label))).clicked() {
                    d.fields.insert("rotate".into(), json!(v));
                }
            }
        });
        ui.end_row();

        crate::rtl::label(ui, tr("Pages:"));
        text_field(ui, d, "pages", 120.0);
        ui.end_row();

        crate::rtl::label(ui, tr("Copies:"));
        text_field(ui, d, "copies", 56.0);
        ui.end_row();

        crate::rtl::label(ui, tr("Marks:"));
        ui.horizontal(|ui| {
            for (v, label) in [("none", "None"), ("sheet", "Sheet"), ("each", "Each Page")] {
                if ui.radio(d.s("marks") == v, crate::rtl::widget(ui, tr(label))).clicked() {
                    d.fields.insert("marks".into(), json!(v));
                }
            }
        });
        ui.end_row();
    });
    check(ui, d, "center", tr("Center on Sheet"));
    check(ui, d, "bleed", tr("Use Document Bleed"));
    ui.add_space(6.0);
    preview(app, ui, d, &lang);
}

fn text_field(ui: &mut egui::Ui, d: &mut Dialog, key: &str, w: f32) {
    let mut s = d.s(key);
    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(w)).changed() {
        d.fields.insert(key.into(), Value::String(s));
    }
}

fn check(ui: &mut egui::Ui, d: &mut Dialog, key: &str, label: &str) {
    let mut b = d.b(key);
    if ui.checkbox(&mut b, crate::rtl::widget(ui, label)).changed() {
        d.fields.insert(key.into(), Value::Bool(b));
    }
}

fn preview(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog, lang: &str) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let layout = params(d).and_then(|p| app.run("file.imposeLayout", p));
    let area = 260.0_f32;
    let (frame, _) = ui.allocate_exact_size(egui::vec2(area, area), egui::Sense::hover());
    let painter = ui.painter_at(frame);
    painter.rect_filled(frame, 0.0, t.panel_darker);
    let r = match &layout {
        Ok(r) => r,
        Err(e) => {
            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, e)).color(t.text_dim));
            return;
        }
    };
    let (sw, sh) = match params(d) {
        Ok(p) => (p["sheet"][0].as_f64().unwrap_or(1.0), p["sheet"][1].as_f64().unwrap_or(1.0)),
        Err(_) => return,
    };
    let scale = (f64::from(area - 8.0) / sw.max(1.0)).min(f64::from(area - 8.0) / sh.max(1.0)) as f32;
    let origin = frame.center() - egui::vec2(sw as f32 * scale, sh as f32 * scale) / 2.0;
    let map = |x: f64, y: f64| origin + egui::vec2(x as f32 * scale, y as f32 * scale);
    let sheet = egui::Rect::from_min_max(map(0.0, 0.0), map(sw, sh));
    painter.rect_filled(sheet, 0.0, egui::Color32::WHITE);
    painter.rect_stroke(sheet, 0.0, egui::Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
    let rect = |v: &Value| -> Option<egui::Rect> {
        let a = v.as_array()?;
        let n = |i: usize| a.get(i).and_then(Value::as_f64);
        Some(egui::Rect::from_min_max(map(n(0)?, n(1)?), map(n(0)? + n(2)?, n(1)? + n(3)?)))
    };
    let cells = r["cells"].as_array().map(Vec::as_slice).unwrap_or_default();
    for c in cells {
        if let Some(clip) = rect(&c["clip"]) {
            painter.rect_filled(clip, 0.0, t.accent.gamma_multiply(0.18));
        }
    }
    for (i, c) in cells.iter().enumerate() {
        if let Some(trim) = rect(&c["trim"]) {
            painter.rect_filled(trim, 0.0, t.accent.gamma_multiply(0.35));
            painter.rect_stroke(trim, 0.0, egui::Stroke::new(1.0, t.accent_strong), egui::StrokeKind::Inside);
            if cells.len() <= 64 {
                painter.text(trim.center(), egui::Align2::CENTER_CENTER, (i + 1).to_string(), egui::FontId::proportional(11.0), t.text_strong);
            }
        }
    }
    for m in r["marks"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let n = |i: usize| m.get(i).and_then(Value::as_f64).unwrap_or(0.0);
        painter.line_segment([map(n(0), n(1)), map(n(2), n(3))], egui::Stroke::new(1.0, egui::Color32::BLACK));
    }
    let per = r["perSheet"].as_u64().unwrap_or(0);
    let sheets = r["sheets"].as_u64().unwrap_or(0);
    let pages = r["placements"].as_u64().unwrap_or(0);
    let line = crate::i18n::tr(lang, "{per} per sheet · {sheets} sheets ({pages} pages)")
        .replace("{per}", &per.to_string())
        .replace("{sheets}", &sheets.to_string())
        .replace("{pages}", &pages.to_string());
    crate::rtl::label(ui, line);
}
