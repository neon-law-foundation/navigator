#![allow(clippy::doc_markdown, clippy::similar_names)]
//! Read-modify-write filling of existing fillable PDFs (AcroForm).
//!
//! Distinct from the Typst [`render`](crate::render) path, which only
//! *emits* fresh PDFs and cannot read one. A blank government form (a
//! Nevada SoS articles form, an IRS 990) is loaded, its AcroForm
//! `/Fields` are populated from a `name → value` map, and the modified
//! PDF bytes are written back. Backed by `lopdf` (pure Rust).
//!
//! ## What it does
//!
//! Walks the AcroForm `/Fields` (top-level named fields), and for each
//! requested field name sets its value by field type, then sets
//! `/NeedAppearances true` on the AcroForm so a viewer regenerates the
//! field appearance streams. We do **not** hand-author `/AP` appearance
//! streams — `NeedAppearances` is the pragmatic, viewer-portable choice.
//!
//! - **Text (`Tx`) and choice (`Ch`)** — `/V` is set as a literal
//!   string on the top-level field. A `Tx` parent whose kids are
//!   `/T`-less duplicate widgets (the same value printed on two pages —
//!   the Nevada SoS packets do this) inherits correctly: kids without
//!   their own `/V` render the parent's.
//! - **Checkbox / radio (`Btn`)** — `/V` is set as a *Name* object that
//!   must match one of the field's appearance states (the keys of each
//!   widget's `/AP /N` dictionary; `Off` is always allowed). For a
//!   radio group (parent with `/T`-less kids) the matching kid's `/AS`
//!   is set to the chosen state and every other kid to `Off`; a
//!   kid-less checkbox gets its own `/AS`. On-state names in real
//!   government forms are arbitrary (`Yes`, `1`, `managers`,
//!   `24HOUR Expedite`) — field maps record the exact state, derived
//!   from the vendored bytes, never guessed.
//!
//! ## What it refuses (loud failures, never a silent blank)
//!
//! - **XFA forms** (`/AcroForm /XFA`) — Adobe's XML form layer has no
//!   AcroForm `/V` to set, so filling one would silently produce a
//!   pristine blank that *looks* ready to file. No Rust crate fills
//!   XFA; we [`PdfError::XfaUnsupported`] instead.
//! - **A field name with no match** — a mis-mapped or renamed
//!   government field is a competence trap if dropped silently, so an
//!   unmatched key is [`PdfError::UnmatchedField`], not a no-op.
//! - **A `Btn` value matching no appearance state** — including a
//!   pushbutton (which has no states at all) —
//!   [`PdfError::InvalidChoice`], carrying the allowed states.
//! - **No AcroForm at all** — [`PdfError::NoAcroForm`].
//!
//! Dotted hierarchical `/T` names (kids carrying their own `/T`) remain
//! out of scope; none of the forms we fill use them.

use std::collections::BTreeMap;

use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};

use crate::PdfError;

/// One field's planned mutation, computed during the read phase so the
/// write phase needs no immutable borrows of the document.
enum FillPlan {
    /// Set `/V` to a literal string on the field (text / choice).
    Text { fid: ObjectId, value: String },
    /// Set `/V` to a Name on the field and `/AS` on each listed widget.
    Button {
        fid: ObjectId,
        state: Vec<u8>,
        widget_as: Vec<(ObjectId, Vec<u8>)>,
    },
}

/// Fill an existing fillable PDF's AcroForm fields.
///
/// `fields` maps a field's `/T` name to the value to set. For text and
/// choice fields the value becomes the `/V` string; for checkbox /
/// radio (`Btn`) fields it must be one of the field's appearance-state
/// names (or `Off`). Returns the modified PDF bytes.
///
/// # Errors
///
/// - [`PdfError::Lopdf`] if `blank_pdf` is not a parseable PDF.
/// - [`PdfError::NoAcroForm`] if the document has no AcroForm.
/// - [`PdfError::XfaUnsupported`] if the form is XFA-based.
/// - [`PdfError::UnmatchedField`] if a key in `fields` matches no
///   form field — never silently dropped.
/// - [`PdfError::InvalidChoice`] if a `Btn` value matches none of the
///   field's appearance states — never a silently unchecked box.
pub fn fill_acroform(
    blank_pdf: &[u8],
    fields: &BTreeMap<String, String>,
) -> Result<Vec<u8>, PdfError> {
    let mut doc = Document::load_mem(blank_pdf).map_err(|e| PdfError::Lopdf(e.to_string()))?;

    let (acroform_id, field_ids) = locate_acroform(&doc)?;

    // Build name → field ObjectId for the top-level named fields.
    let mut by_name: BTreeMap<String, ObjectId> = BTreeMap::new();
    for fid in &field_ids {
        if let Ok(dict) = doc.get_object(*fid).and_then(Object::as_dict) {
            if let Ok(name) = dict.get(b"T").and_then(Object::as_str) {
                by_name.insert(String::from_utf8_lossy(name).into_owned(), *fid);
            }
        }
    }

    // Every requested key must match a field — a mis-mapped name is a
    // loud error, not a silent drop (competence guardrail).
    for name in fields.keys() {
        if !by_name.contains_key(name) {
            return Err(PdfError::UnmatchedField(name.clone()));
        }
    }

    // Read phase: plan every mutation while the document is borrowed
    // immutably; validate Btn values against the appearance states.
    let mut plans: Vec<FillPlan> = Vec::with_capacity(fields.len());
    for (name, value) in fields {
        let fid = by_name[name];
        let dict = doc
            .get_object(fid)
            .and_then(Object::as_dict)
            .map_err(|e| PdfError::Lopdf(e.to_string()))?;
        let is_button = dict.get(b"FT").and_then(Object::as_name).ok() == Some(b"Btn");
        if is_button {
            plans.push(plan_button(&doc, fid, dict, name, value)?);
        } else {
            plans.push(FillPlan::Text {
                fid,
                value: value.clone(),
            });
        }
    }

    // Write phase: apply the plans.
    for plan in plans {
        match plan {
            FillPlan::Text { fid, value } => {
                if let Some(Object::Dictionary(dict)) = doc.objects.get_mut(&fid) {
                    dict.set(
                        "V",
                        Object::String(value.into_bytes(), StringFormat::Literal),
                    );
                }
            }
            FillPlan::Button {
                fid,
                state,
                widget_as,
            } => {
                if let Some(Object::Dictionary(dict)) = doc.objects.get_mut(&fid) {
                    dict.set("V", Object::Name(state));
                }
                for (wid, as_state) in widget_as {
                    if let Some(Object::Dictionary(dict)) = doc.objects.get_mut(&wid) {
                        dict.set("AS", Object::Name(as_state));
                    }
                }
            }
        }
    }

    // NeedAppearances=true so viewers regenerate field appearances from
    // the new values rather than showing stale/empty boxes.
    set_need_appearances(&mut doc, acroform_id);

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| PdfError::Lopdf(e.to_string()))?;
    Ok(out)
}

/// Plan a checkbox / radio fill: validate `value` against the field's
/// appearance states and compute the `/AS` to set on each widget.
///
/// A radio group is a parent with `/T`-less kid widgets, each carrying
/// its own on-state in `/AP /N`; a checkbox is a kid-less field whose
/// own `/AP /N` holds the on-state. `Off` is always a valid value (it
/// unchecks). A pushbutton has no states at all, so any value for it is
/// [`PdfError::InvalidChoice`] with an empty allowed list.
fn plan_button(
    doc: &Document,
    fid: ObjectId,
    dict: &Dictionary,
    name: &str,
    value: &str,
) -> Result<FillPlan, PdfError> {
    let kid_ids: Vec<ObjectId> = dict
        .get(b"Kids")
        .and_then(Object::as_array)
        .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
        .unwrap_or_default();

    // (widget id, that widget's on-states) — the field itself when the
    // checkbox has no kids.
    let widgets: Vec<(ObjectId, Vec<String>)> = if kid_ids.is_empty() {
        vec![(fid, appearance_states(doc, dict))]
    } else {
        kid_ids
            .iter()
            .map(|kid| {
                let states = doc
                    .get_object(*kid)
                    .and_then(Object::as_dict)
                    .map(|d| appearance_states(doc, d))
                    .unwrap_or_default();
                (*kid, states)
            })
            .collect()
    };

    let mut allowed: Vec<String> = widgets
        .iter()
        .flat_map(|(_, states)| states.iter().cloned())
        .filter(|s| s != "Off")
        .collect();
    allowed.sort();
    allowed.dedup();

    if value != "Off" && !allowed.iter().any(|s| s == value) {
        return Err(PdfError::InvalidChoice {
            field: name.to_string(),
            value: value.to_string(),
            allowed,
        });
    }

    let widget_as = widgets
        .into_iter()
        .map(|(wid, states)| {
            let as_state = if value != "Off" && states.iter().any(|s| s == value) {
                value.as_bytes().to_vec()
            } else {
                b"Off".to_vec()
            };
            (wid, as_state)
        })
        .collect();

    Ok(FillPlan::Button {
        fid,
        state: value.as_bytes().to_vec(),
        widget_as,
    })
}

/// The appearance-state names of one widget: the keys of its `/AP /N`
/// dictionary (`/AP` and `/N` may each be indirect). Streams behind the
/// keys are irrelevant here — only the names are.
fn appearance_states(doc: &Document, widget: &Dictionary) -> Vec<String> {
    let Some(ap) = resolve_dict(doc, widget.get(b"AP").ok()) else {
        return Vec::new();
    };
    let Some(n) = resolve_dict(doc, ap.get(b"N").ok()) else {
        return Vec::new();
    };
    n.iter()
        .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
        .collect()
}

/// Resolve an object that may be a dictionary or a reference to one.
fn resolve_dict<'a>(doc: &'a Document, obj: Option<&'a Object>) -> Option<&'a Dictionary> {
    match obj? {
        Object::Dictionary(d) => Some(d),
        Object::Reference(id) => doc.get_object(*id).and_then(Object::as_dict).ok(),
        _ => None,
    }
}

/// Resolve the AcroForm and its top-level field ids. The AcroForm is
/// always stored as an indirect object here (the builder writes it that
/// way and real forms do too), so `acroform_id` is its `ObjectId`.
fn locate_acroform(doc: &Document) -> Result<(ObjectId, Vec<ObjectId>), PdfError> {
    let root_id = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|e| PdfError::Lopdf(e.to_string()))?;
    let catalog = doc
        .get_object(root_id)
        .and_then(Object::as_dict)
        .map_err(|e| PdfError::Lopdf(e.to_string()))?;
    let acroform_id = catalog
        .get(b"AcroForm")
        .and_then(Object::as_reference)
        .map_err(|_| PdfError::NoAcroForm)?;
    let acroform = doc
        .get_object(acroform_id)
        .and_then(Object::as_dict)
        .map_err(|_| PdfError::NoAcroForm)?;

    if acroform.has(b"XFA") {
        return Err(PdfError::XfaUnsupported);
    }

    let field_ids = acroform
        .get(b"Fields")
        .and_then(Object::as_array)
        .map_err(|_| PdfError::NoAcroForm)?
        .iter()
        .filter_map(|o| o.as_reference().ok())
        .collect();
    Ok((acroform_id, field_ids))
}

fn set_need_appearances(doc: &mut Document, acroform_id: ObjectId) {
    if let Some(Object::Dictionary(dict)) = doc.objects.get_mut(&acroform_id) {
        dict.set("NeedAppearances", Object::Boolean(true));
    }
}

/// One field in a synthetic fixture form — mirrors the three field
/// shapes the real government packets carry.
#[derive(Debug, Clone)]
pub enum FieldSpec {
    /// A `Tx` text field.
    Text { name: String },
    /// A kid-less `Btn` checkbox whose `/AP /N` carries `on_state` +
    /// `Off` (real on-state names are arbitrary: `Yes`, `1`,
    /// `managers`, …).
    Checkbox { name: String, on_state: String },
    /// A `Btn` radio group: a `/T`-named parent with one `/T`-less kid
    /// widget per option, each kid's `/AP /N` carrying its option +
    /// `Off`.
    Radio { name: String, options: Vec<String> },
}

impl FieldSpec {
    fn text(name: &str) -> Self {
        Self::Text { name: name.into() }
    }
}

/// Build a minimal but genuinely-valid fillable PDF with one text field
/// per name in `field_names` (each `/V` empty). Shorthand for
/// [`blank_acroform_with`] over [`FieldSpec::Text`] entries.
#[must_use]
pub fn blank_acroform(field_names: &[&str]) -> Vec<u8> {
    let specs: Vec<FieldSpec> = field_names.iter().map(|n| FieldSpec::text(n)).collect();
    blank_acroform_with(&specs)
}

/// Append the widget-annotation keys shared by every fixture field.
fn set_widget_keys(field: &mut Dictionary, page_id: ObjectId) {
    field.set("Type", Object::Name(b"Annot".to_vec()));
    field.set("Subtype", Object::Name(b"Widget".to_vec()));
    field.set("P", Object::Reference(page_id));
    field.set(
        "Rect",
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(200),
            Object::Integer(20),
        ]),
    );
}

/// An `/AP` dictionary whose `/N` carries the given state names (the
/// streams behind them are irrelevant to filling, so `Null` stands in).
fn appearance_dict(states: &[&str]) -> Object {
    let mut n = Dictionary::new();
    for state in states {
        n.set(state.as_bytes().to_vec(), Object::Null);
    }
    let mut ap = Dictionary::new();
    ap.set("N", Object::Dictionary(n));
    Object::Dictionary(ap)
}

/// Add one `Tx` text field widget; returns its id.
fn add_text_field(doc: &mut Document, page_id: ObjectId, name: &str) -> ObjectId {
    let mut field = Dictionary::new();
    field.set("FT", Object::Name(b"Tx".to_vec()));
    field.set(
        "T",
        Object::String(name.as_bytes().to_vec(), StringFormat::Literal),
    );
    field.set("V", Object::String(Vec::new(), StringFormat::Literal));
    set_widget_keys(&mut field, page_id);
    doc.add_object(Object::Dictionary(field))
}

/// Add one kid-less `Btn` checkbox with an arbitrary on-state; returns
/// its id.
fn add_checkbox_field(
    doc: &mut Document,
    page_id: ObjectId,
    name: &str,
    on_state: &str,
) -> ObjectId {
    let mut field = Dictionary::new();
    field.set("FT", Object::Name(b"Btn".to_vec()));
    field.set(
        "T",
        Object::String(name.as_bytes().to_vec(), StringFormat::Literal),
    );
    field.set("V", Object::Name(b"Off".to_vec()));
    field.set("AS", Object::Name(b"Off".to_vec()));
    field.set("AP", appearance_dict(&[on_state, "Off"]));
    set_widget_keys(&mut field, page_id);
    doc.add_object(Object::Dictionary(field))
}

/// Add a `Btn` radio group — a `/T`-named parent with one `/T`-less kid
/// widget per option; returns (parent id, kid ids).
fn add_radio_group(
    doc: &mut Document,
    page_id: ObjectId,
    name: &str,
    options: &[String],
) -> (ObjectId, Vec<ObjectId>) {
    let parent_id = doc.new_object_id();
    let mut kid_ids: Vec<ObjectId> = Vec::new();
    for option in options {
        let mut kid = Dictionary::new();
        kid.set("Parent", Object::Reference(parent_id));
        kid.set("AS", Object::Name(b"Off".to_vec()));
        kid.set("AP", appearance_dict(&[option, "Off"]));
        set_widget_keys(&mut kid, page_id);
        kid_ids.push(doc.add_object(Object::Dictionary(kid)));
    }
    let mut parent = Dictionary::new();
    parent.set("FT", Object::Name(b"Btn".to_vec()));
    parent.set(
        "T",
        Object::String(name.as_bytes().to_vec(), StringFormat::Literal),
    );
    // Radio flag (bit 16) + no-toggle-to-off (bit 15), as the real
    // packets set.
    parent.set("Ff", Object::Integer(49152));
    parent.set("V", Object::Name(b"Off".to_vec()));
    parent.set(
        "Kids",
        Object::Array(kid_ids.iter().copied().map(Object::Reference).collect()),
    );
    doc.objects.insert(parent_id, Object::Dictionary(parent));
    (parent_id, kid_ids)
}

/// Build a minimal but genuinely-valid fillable PDF from field specs.
/// Used to produce blank form fixtures for tests and synthetic forms —
/// the same AcroForm structures (text widgets, checkboxes with arbitrary
/// on-states, radio groups with `/T`-less kids) real government forms
/// carry, so [`fill_acroform`] exercises the real path.
#[must_use]
pub fn blank_acroform_with(specs: &[FieldSpec]) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");

    let page_id = doc.new_object_id();
    let pages_id = doc.new_object_id();

    let mut field_ids: Vec<ObjectId> = Vec::new();
    let mut annot_ids: Vec<ObjectId> = Vec::new();
    for spec in specs {
        match spec {
            FieldSpec::Text { name } => {
                let fid = add_text_field(&mut doc, page_id, name);
                field_ids.push(fid);
                annot_ids.push(fid);
            }
            FieldSpec::Checkbox { name, on_state } => {
                let fid = add_checkbox_field(&mut doc, page_id, name, on_state);
                field_ids.push(fid);
                annot_ids.push(fid);
            }
            FieldSpec::Radio { name, options } => {
                let (parent_id, kid_ids) = add_radio_group(&mut doc, page_id, name, options);
                field_ids.push(parent_id);
                annot_ids.extend(kid_ids);
            }
        }
    }

    let mut page = Dictionary::new();
    page.set("Type", Object::Name(b"Page".to_vec()));
    page.set("Parent", Object::Reference(pages_id));
    page.set(
        "MediaBox",
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(612),
            Object::Integer(792),
        ]),
    );
    page.set(
        "Annots",
        Object::Array(annot_ids.iter().copied().map(Object::Reference).collect()),
    );
    doc.objects.insert(page_id, Object::Dictionary(page));

    let mut pages = Dictionary::new();
    pages.set("Type", Object::Name(b"Pages".to_vec()));
    pages.set("Kids", Object::Array(vec![Object::Reference(page_id)]));
    pages.set("Count", Object::Integer(1));
    doc.objects.insert(pages_id, Object::Dictionary(pages));

    let mut acroform = Dictionary::new();
    acroform.set(
        "Fields",
        Object::Array(field_ids.iter().copied().map(Object::Reference).collect()),
    );
    acroform.set(
        "DA",
        Object::String(b"/Helv 0 Tf 0 g".to_vec(), StringFormat::Literal),
    );
    let acroform_id = doc.add_object(Object::Dictionary(acroform));

    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_id));
    catalog.set("AcroForm", Object::Reference(acroform_id));
    let catalog_id = doc.add_object(Object::Dictionary(catalog));

    doc.trailer.set("Root", Object::Reference(catalog_id));

    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save synthetic AcroForm");
    out
}

/// Read back the `/V` value of a top-level field by `/T` name. A test
/// helper for round-trip assertions; returns `None` if the field or its
/// value is absent. A `Btn` field's Name-typed `/V` reads back as the
/// state name string.
#[must_use]
pub fn read_field_value(pdf: &[u8], name: &str) -> Option<String> {
    let doc = Document::load_mem(pdf).ok()?;
    let (_, field_ids) = locate_acroform(&doc).ok()?;
    for fid in field_ids {
        let dict = doc.get_object(fid).and_then(Object::as_dict).ok()?;
        if dict.get(b"T").and_then(Object::as_str).ok() == Some(name.as_bytes()) {
            return match dict.get(b"V").ok()? {
                Object::String(v, _) | Object::Name(v) => {
                    Some(String::from_utf8_lossy(v).into_owned())
                }
                _ => None,
            };
        }
    }
    None
}

/// Read back every top-level field's `/V` in one parse — the bulk
/// sibling of [`read_field_value`] for round-trip assertions over big
/// government packets, where a per-field parse is prohibitively slow.
/// Name-typed `/V` values read back as their state-name strings;
/// fields with no readable `/V` are absent from the map.
///
/// # Errors
///
/// The same parse / locate failures as [`fill_acroform`].
pub fn read_field_values(pdf: &[u8]) -> Result<BTreeMap<String, String>, PdfError> {
    let doc = Document::load_mem(pdf).map_err(|e| PdfError::Lopdf(e.to_string()))?;
    let (_, field_ids) = locate_acroform(&doc)?;
    let mut out = BTreeMap::new();
    for fid in field_ids {
        let Ok(dict) = doc.get_object(fid).and_then(Object::as_dict) else {
            continue;
        };
        let Ok(name) = dict.get(b"T").and_then(Object::as_str) else {
            continue;
        };
        let value = match dict.get(b"V") {
            Ok(Object::String(v, _) | Object::Name(v)) => String::from_utf8_lossy(v).into_owned(),
            _ => continue,
        };
        out.insert(String::from_utf8_lossy(name).into_owned(), value);
    }
    Ok(out)
}

/// The `/T` names of every top-level AcroForm field. Used by field-map
/// guard tests to assert that each mapped name exists in the vendored
/// bytes before a fill is ever attempted in production.
///
/// # Errors
///
/// The same parse / locate failures as [`fill_acroform`].
pub fn field_names(pdf: &[u8]) -> Result<Vec<String>, PdfError> {
    let doc = Document::load_mem(pdf).map_err(|e| PdfError::Lopdf(e.to_string()))?;
    let (_, field_ids) = locate_acroform(&doc)?;
    let mut names = Vec::with_capacity(field_ids.len());
    for fid in field_ids {
        if let Ok(dict) = doc.get_object(fid).and_then(Object::as_dict) {
            if let Ok(name) = dict.get(b"T").and_then(Object::as_str) {
                names.push(String::from_utf8_lossy(name).into_owned());
            }
        }
    }
    Ok(names)
}

/// Read back the `/AS` appearance state of a widget: the field itself
/// for a kid-less checkbox, or the `index`-th kid of a radio group. A
/// test helper for asserting the visible checked state, not just `/V`.
#[must_use]
pub fn read_widget_appearance_state(
    pdf: &[u8],
    name: &str,
    index: Option<usize>,
) -> Option<String> {
    let doc = Document::load_mem(pdf).ok()?;
    let (_, field_ids) = locate_acroform(&doc).ok()?;
    for fid in field_ids {
        let dict = doc.get_object(fid).and_then(Object::as_dict).ok()?;
        if dict.get(b"T").and_then(Object::as_str).ok() != Some(name.as_bytes()) {
            continue;
        }
        let widget = match index {
            None => dict,
            Some(i) => {
                let kids = dict.get(b"Kids").and_then(Object::as_array).ok()?;
                let kid_id = kids.get(i)?.as_reference().ok()?;
                doc.get_object(kid_id).and_then(Object::as_dict).ok()?
            }
        };
        let v = widget.get(b"AS").and_then(Object::as_name).ok()?;
        return Some(String::from_utf8_lossy(v).into_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        blank_acroform, blank_acroform_with, fill_acroform, read_field_value,
        read_widget_appearance_state, FieldSpec,
    };
    use crate::PdfError;
    use std::collections::BTreeMap;

    fn fields(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn fill_populates_field_values_that_round_trip() {
        let blank = blank_acroform(&["entity_name", "registered_agent"]);
        let filled = fill_acroform(
            &blank,
            &fields(&[
                ("entity_name", "Neon Law LLC"),
                ("registered_agent", "Jane Doe"),
            ]),
        )
        .expect("fill succeeds");

        assert_eq!(
            read_field_value(&filled, "entity_name").as_deref(),
            Some("Neon Law LLC")
        );
        assert_eq!(
            read_field_value(&filled, "registered_agent").as_deref(),
            Some("Jane Doe")
        );
    }

    #[test]
    fn fill_sets_need_appearances_so_viewers_regenerate() {
        let blank = blank_acroform(&["x"]);
        let filled = fill_acroform(&blank, &fields(&[("x", "v")])).unwrap();
        let doc = lopdf::Document::load_mem(&filled).unwrap();
        let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        let catalog = doc.get_object(root).unwrap().as_dict().unwrap();
        let af_id = catalog.get(b"AcroForm").unwrap().as_reference().unwrap();
        let af = doc.get_object(af_id).unwrap().as_dict().unwrap();
        assert!(af.get(b"NeedAppearances").unwrap().as_bool().unwrap());
    }

    #[test]
    fn unmatched_field_is_a_loud_error_not_a_silent_drop() {
        let blank = blank_acroform(&["known"]);
        let err = fill_acroform(&blank, &fields(&[("nonexistent", "v")])).unwrap_err();
        match err {
            PdfError::UnmatchedField(name) => assert_eq!(name, "nonexistent"),
            other => panic!("expected UnmatchedField, got {other:?}"),
        }
    }

    #[test]
    fn xfa_form_is_rejected_loudly() {
        // Take a valid AcroForm and inject an /XFA key to mark it XFA.
        let blank = blank_acroform(&["a"]);
        let mut doc = lopdf::Document::load_mem(&blank).unwrap();
        let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        let af_id = doc
            .get_object(root)
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"AcroForm")
            .unwrap()
            .as_reference()
            .unwrap();
        if let Some(lopdf::Object::Dictionary(d)) = doc.objects.get_mut(&af_id) {
            d.set("XFA", lopdf::Object::Array(vec![]));
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();

        let err = fill_acroform(&bytes, &fields(&[("a", "v")])).unwrap_err();
        assert!(matches!(err, PdfError::XfaUnsupported));
    }

    #[test]
    fn a_pdf_without_acroform_errors() {
        // A Typst-rendered PDF has no AcroForm.
        let plain = crate::render("Hello, no form here.").unwrap();
        let err = fill_acroform(&plain, &fields(&[("a", "v")])).unwrap_err();
        assert!(matches!(err, PdfError::NoAcroForm));
    }

    #[test]
    fn garbage_bytes_are_a_parse_error() {
        let err = fill_acroform(b"not a pdf at all", &fields(&[("a", "v")])).unwrap_err();
        assert!(matches!(err, PdfError::Lopdf(_)));
    }

    #[test]
    fn checkbox_checks_with_its_arbitrary_on_state() {
        // Real packets use on-states like `managers`, `NRS 86`, `1` —
        // never assume `Yes`.
        let blank = blank_acroform_with(&[FieldSpec::Checkbox {
            name: "managers_a".into(),
            on_state: "managers".into(),
        }]);
        let filled = fill_acroform(&blank, &fields(&[("managers_a", "managers")])).unwrap();
        assert_eq!(
            read_field_value(&filled, "managers_a").as_deref(),
            Some("managers")
        );
        assert_eq!(
            read_widget_appearance_state(&filled, "managers_a", None).as_deref(),
            Some("managers"),
            "/AS must show the box as visibly checked, not just /V"
        );
    }

    #[test]
    fn checkbox_unchecks_with_off() {
        let blank = blank_acroform_with(&[FieldSpec::Checkbox {
            name: "x".into(),
            on_state: "Yes".into(),
        }]);
        let filled = fill_acroform(&blank, &fields(&[("x", "Off")])).unwrap();
        assert_eq!(read_field_value(&filled, "x").as_deref(), Some("Off"));
    }

    #[test]
    fn radio_group_selects_one_kid_and_offs_the_rest() {
        // The packets' processing-request groups: a /T-named parent,
        // /T-less kids each carrying one on-state.
        let blank = blank_acroform_with(&[FieldSpec::Radio {
            name: "processing".into(),
            options: vec!["Regular".into(), "24HOUR Expedite".into()],
        }]);
        let filled = fill_acroform(&blank, &fields(&[("processing", "24HOUR Expedite")])).unwrap();
        assert_eq!(
            read_field_value(&filled, "processing").as_deref(),
            Some("24HOUR Expedite")
        );
        assert_eq!(
            read_widget_appearance_state(&filled, "processing", Some(0)).as_deref(),
            Some("Off"),
            "the unchosen kid must be /AS Off"
        );
        assert_eq!(
            read_widget_appearance_state(&filled, "processing", Some(1)).as_deref(),
            Some("24HOUR Expedite"),
            "the chosen kid must carry the selected state"
        );
    }

    #[test]
    fn invalid_choice_is_a_loud_error_with_the_allowed_states() {
        let blank = blank_acroform_with(&[FieldSpec::Radio {
            name: "processing".into(),
            options: vec!["Regular".into(), "1HOUR Expedite".into()],
        }]);
        let err = fill_acroform(&blank, &fields(&[("processing", "Overnight")])).unwrap_err();
        match err {
            PdfError::InvalidChoice {
                field,
                value,
                allowed,
            } => {
                assert_eq!(field, "processing");
                assert_eq!(value, "Overnight");
                assert_eq!(allowed, vec!["1HOUR Expedite", "Regular"]);
            }
            other => panic!("expected InvalidChoice, got {other:?}"),
        }
    }

    #[test]
    fn pushbutton_with_no_states_rejects_any_value() {
        // A pushbutton (PRINT, Get_Checklist) has no /AP states — any
        // attempt to fill it must fail with an empty allowed list.
        let blank = blank_acroform_with(&[FieldSpec::Checkbox {
            name: "PRINT".into(),
            on_state: "ignored".into(),
        }]);
        // Strip the /AP to model a pushbutton.
        let mut doc = lopdf::Document::load_mem(&blank).unwrap();
        let ids: Vec<_> = doc.objects.keys().copied().collect();
        for id in ids {
            if let Some(lopdf::Object::Dictionary(d)) = doc.objects.get_mut(&id) {
                if d.get(b"T").and_then(lopdf::Object::as_str).ok() == Some(b"PRINT".as_slice()) {
                    d.remove(b"AP");
                }
            }
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();

        let err = fill_acroform(&bytes, &fields(&[("PRINT", "clicked")])).unwrap_err();
        assert!(matches!(
            err,
            PdfError::InvalidChoice { allowed, .. } if allowed.is_empty()
        ));
    }

    #[test]
    fn mixed_text_checkbox_radio_fill_in_one_pass() {
        let blank = blank_acroform_with(&[
            FieldSpec::Text {
                name: "entity_name".into(),
            },
            FieldSpec::Checkbox {
                name: "series".into(),
                on_state: "1".into(),
            },
            FieldSpec::Radio {
                name: "management".into(),
                options: vec!["managers".into(), "members".into()],
            },
        ]);
        let filled = fill_acroform(
            &blank,
            &fields(&[
                ("entity_name", "Neon Law LLC"),
                ("series", "1"),
                ("management", "members"),
            ]),
        )
        .unwrap();
        assert_eq!(
            read_field_value(&filled, "entity_name").as_deref(),
            Some("Neon Law LLC")
        );
        assert_eq!(read_field_value(&filled, "series").as_deref(), Some("1"));
        assert_eq!(
            read_field_value(&filled, "management").as_deref(),
            Some("members")
        );
    }
}
