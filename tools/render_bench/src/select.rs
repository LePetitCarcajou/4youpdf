//! How the page set was chosen, so that it can be chosen again the same way
//! (`fyp-render-bench select`). Every page of the fixtures goes in: they are
//! the cases of file structure the core handles (repaired tables, encryption,
//! cross-reference streams). From the public corpus, every page is described
//! by tags of what a renderer may get wrong: filters, fonts, colour spaces,
//! shadings, transparency, annotations, geometry, repaired or encrypted file;
//! `normalize` folds the names the norm does not define into one tag. Pages
//! are then taken greedily, rarest tag first, until every tag has
//! `per_feature` pages, preferring the page that covers the most missing tags,
//! then the cheapest. The cheapest pages are seldom real documents: the
//! costliest pages come next, one per file (`heavy`), then a few pages drawn
//! at random, with a fixed seed, from each lot. A corpus page goes in only if
//! the reference engine renders it within `max_render_ms`.
//!
//! Given the same corpus and the same answers of the reference engine, the
//! choice is the same. The set it produces is versioned: it is not chosen
//! again at every run.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use fyp_core::document::Document;
use fyp_core::encryption::Cipher;
use fyp_core::lexer::{is_whitespace, Lexer, Token};
use fyp_core::object::{Dict, Name, ObjRef, Object};
use fyp_core::ops;

use crate::engine::{self, PageOutcome, Places};
use crate::pageset::{PageEntry, PageSet, FORMAT};
use crate::protocol::{PageRequest, Request, PROTOCOL};
use crate::system;

/// A page of the corpus the selection may take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The file, relative to the root.
    pub file: String,
    /// SHA-256 of the file.
    pub sha256: String,
    /// Page number, 0-based.
    pub index: usize,
    /// Password that opened the file.
    pub password: String,
    /// What the page holds.
    pub features: BTreeSet<String>,
    /// A rough cost of drawing it: bytes of content, pixels of images.
    pub cost: u64,
}

/// Objects a page scan may read, bytes of content it may decode, tokens of
/// one content stream it may read: bounds for files built to be expensive.
const MAX_OBJECTS: usize = 20_000;
const MAX_DECODED: usize = 8 << 20;
const MAX_TOKENS: usize = 400_000;
/// Depth of nested resources, colour spaces and functions followed.
const MAX_DEPTH: usize = 8;

const STANDARD_14: [&str; 14] = [
    "Times-Roman",
    "Times-Bold",
    "Times-Italic",
    "Times-BoldItalic",
    "Helvetica",
    "Helvetica-Bold",
    "Helvetica-Oblique",
    "Helvetica-BoldOblique",
    "Courier",
    "Courier-Bold",
    "Courier-Oblique",
    "Courier-BoldOblique",
    "Symbol",
    "ZapfDingbats",
];

/// What a page holds, as tags.
struct Scanner<'s, 'a> {
    doc: &'s Document<'a>,
    features: BTreeSet<String>,
    visited: HashSet<ObjRef>,
    objects: usize,
    decoded: usize,
    cost: u64,
}

impl<'s, 'a> Scanner<'s, 'a> {
    fn new(doc: &'s Document<'a>) -> Self {
        Scanner {
            doc,
            features: BTreeSet::new(),
            visited: HashSet::new(),
            objects: 0,
            decoded: 0,
            cost: 0,
        }
    }

    fn flag(&mut self, feature: &str) {
        self.features.insert(feature.to_string());
    }

    fn tag(&mut self, prefix: &str, name: &[u8]) {
        self.features
            .insert(format!("{prefix}:{}", String::from_utf8_lossy(name)));
    }

    /// `obj`, a reference followed once: never twice the same object, never
    /// beyond [`MAX_OBJECTS`].
    fn resolve(&mut self, obj: &Object) -> Option<Object> {
        match obj {
            Object::Reference(r) => {
                if self.objects >= MAX_OBJECTS || !self.visited.insert(*r) {
                    return None;
                }
                self.objects += 1;
                self.doc.get(*r).ok().flatten()
            }
            other => Some(other.clone()),
        }
    }

    /// A value of `dict`, a reference followed without counting it.
    fn value(&self, dict: &Dict, key: &str) -> Option<Object> {
        dict.get(&Name::new(key))
            .and_then(|value| self.doc.resolve(value).ok())
    }

    fn name(&self, dict: &Dict, key: &str) -> Option<Vec<u8>> {
        match self.value(dict, key) {
            Some(Object::Name(name)) => Some(name.0),
            _ => None,
        }
    }

    fn number(&self, dict: &Dict, key: &str) -> Option<f64> {
        match self.value(dict, key) {
            Some(Object::Integer(n)) => Some(n as f64),
            Some(Object::Real(x)) => Some(x),
            _ => None,
        }
    }

    fn is(&self, dict: &Dict, key: &str, expected: &str) -> bool {
        self.name(dict, key).as_deref() == Some(expected.as_bytes())
    }

    fn filters(&self, dict: &Dict) -> Vec<Vec<u8>> {
        match self.value(dict, "Filter") {
            Some(Object::Name(name)) => vec![name.0],
            Some(Object::Array(items)) => items
                .iter()
                .filter_map(|item| match self.doc.resolve(item) {
                    Ok(Object::Name(name)) => Some(name.0),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn page(&mut self, page: &Dict) {
        let rotate = match self.value(page, "Rotate") {
            Some(Object::Integer(r)) => r.rem_euclid(360),
            _ => 0,
        };
        if rotate != 0 {
            self.flag(&format!("page:rotate-{rotate}"));
        }
        let media = self.value(page, "MediaBox");
        if let (Some(crop), Some(media)) = (self.value(page, "CropBox"), &media) {
            if crop != *media {
                self.flag("page:cropbox");
            }
        }
        if let Some(Object::Array(corners)) = &media {
            let origin = corners.iter().take(2).any(|c| match c {
                Object::Integer(n) => *n != 0,
                Object::Real(x) => *x != 0.0,
                _ => false,
            });
            if origin {
                self.flag("page:origin-offset");
            }
        }
        if self
            .number(page, "UserUnit")
            .is_some_and(|unit| unit != 1.0)
        {
            self.flag("page:user-unit");
        }
        match page
            .get(&Name::new("Contents"))
            .and_then(|c| self.resolve(c))
        {
            None => self.flag("content:none"),
            Some(Object::Array(parts)) => {
                for part in parts {
                    if let Some(stream) = self.resolve(&part) {
                        self.content(&stream);
                    }
                }
            }
            Some(stream) => self.content(&stream),
        }
        if let Some(resources) = page.get(&Name::new("Resources")) {
            self.resources(resources, 0);
        }
        if let Some(Object::Dict(group)) = self.value(page, "Group") {
            if self.is(&group, "S", "Transparency") {
                self.flag("transparency:page-group");
            }
        }
        if let Some(Object::Array(annotations)) = self.value(page, "Annots") {
            for annotation in annotations.iter().take(200) {
                self.annotation(annotation);
            }
        }
    }

    fn annotation(&mut self, obj: &Object) {
        let Some(Object::Dict(annotation)) = self.resolve(obj) else {
            return;
        };
        let Some(subtype) = self.name(&annotation, "Subtype") else {
            return;
        };
        if subtype == b"Link" || subtype == b"Popup" {
            return;
        }
        let appearance = match self.value(&annotation, "AP") {
            Some(Object::Dict(ap)) => ap.get(&Name::new("N")).cloned(),
            _ => None,
        };
        match appearance.and_then(|normal| self.resolve(&normal)) {
            Some(stream @ Object::Stream { .. }) => {
                self.tag("annotation", &subtype);
                self.form(&stream, 1);
            }
            Some(Object::Dict(states)) => {
                self.tag("annotation", &subtype);
                for state in states.values().take(4) {
                    if let Some(stream) = self.resolve(state) {
                        self.form(&stream, 1);
                    }
                }
            }
            _ => {
                self.features.insert(format!(
                    "annotation:{}-no-appearance",
                    String::from_utf8_lossy(&subtype)
                ));
            }
        }
    }

    fn content(&mut self, stream: &Object) {
        let Object::Stream { dict, .. } = stream else {
            return;
        };
        for filter in self.filters(dict) {
            self.tag("content-filter", &filter);
        }
        if self.decoded >= MAX_DECODED {
            return;
        }
        let Ok(data) = self.doc.decoded(stream) else {
            self.flag("content:undecodable");
            return;
        };
        self.decoded += data.len();
        self.cost += data.len() as u64;
        self.operators(&data);
    }

    fn operators(&mut self, data: &[u8]) {
        let mut lexer = Lexer::new(data);
        let mut previous: Option<Token> = None;
        let mut inline: Vec<Token> = Vec::new();
        for _ in 0..MAX_TOKENS {
            let token = match lexer.next_token() {
                Ok(Token::Eof) | Err(_) => break,
                Ok(token) => token,
            };
            if let Token::Keyword(keyword) = &token {
                match keyword.as_slice() {
                    b"BI" => {
                        self.flag("content:inline-image");
                        inline.clear();
                    }
                    b"ID" => {
                        for pair in inline.windows(2) {
                            if let [Token::Name(key), Token::Name(filter)] = pair {
                                if key == b"F" || key == b"Filter" {
                                    self.tag("image-filter", full_filter_name(filter));
                                }
                            }
                        }
                        inline.clear();
                        match inline_image_end(data, lexer.pos()) {
                            Some(end) => lexer.seek(end),
                            None => break,
                        }
                    }
                    b"k" | b"K" => self.flag("colorspace:DeviceCMYK"),
                    b"sh" => self.flag("content:sh"),
                    b"Tj" | b"TJ" | b"'" | b"\"" => self.flag("content:text"),
                    b"Tr" => {
                        if let Some(Token::Integer(mode)) = previous {
                            if mode != 0 {
                                self.flag(&format!("text:render-mode-{mode}"));
                            }
                        }
                    }
                    _ => {}
                }
            } else if inline.len() < 64 {
                inline.push(token.clone());
            }
            previous = Some(token);
        }
    }

    fn resources(&mut self, obj: &Object, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let Some(Object::Dict(resources)) = self.resolve(obj) else {
            return;
        };
        for (kind, entries) in &resources {
            let Some(Object::Dict(entries)) = self.resolve(entries) else {
                continue;
            };
            for entry in entries.values() {
                match kind.0.as_slice() {
                    b"Font" => self.font(entry, depth),
                    b"XObject" => self.xobject(entry, depth),
                    b"ColorSpace" => self.colorspace(entry, 0),
                    b"Pattern" => self.pattern(entry, depth),
                    b"Shading" => self.shading(entry),
                    b"ExtGState" => self.graphics_state(entry, depth),
                    b"Properties" => {
                        if let Some(Object::Dict(properties)) = self.resolve(entry) {
                            if self.is(&properties, "Type", "OCG")
                                || self.is(&properties, "Type", "OCMD")
                            {
                                self.flag("content:optional-content");
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn xobject(&mut self, obj: &Object, depth: usize) {
        let Some(stream) = self.resolve(obj) else {
            return;
        };
        let Object::Stream { dict, .. } = &stream else {
            return;
        };
        match self.name(dict, "Subtype").as_deref() {
            Some(b"Image") => self.image(dict),
            Some(b"Form") => self.form(&stream, depth + 1),
            Some(b"PS") => self.flag("xobject:PS"),
            _ => {}
        }
    }

    fn form(&mut self, stream: &Object, depth: usize) {
        let Object::Stream { dict, .. } = stream else {
            return;
        };
        if let Some(Object::Dict(group)) = self.value(dict, "Group") {
            if self.is(&group, "S", "Transparency") {
                self.flag("transparency:group");
                if matches!(self.value(&group, "K"), Some(Object::Bool(true))) {
                    self.flag("transparency:knockout");
                }
            }
        }
        if dict.contains_key(&Name::new("OC")) {
            self.flag("content:optional-content");
        }
        self.content(stream);
        if let Some(resources) = dict.get(&Name::new("Resources")) {
            self.resources(resources, depth);
        }
    }

    fn image(&mut self, dict: &Dict) {
        let filters = self.filters(dict);
        if filters.is_empty() {
            self.flag("image-filter:none");
        }
        for filter in &filters {
            self.tag("image-filter", filter);
        }
        if matches!(self.value(dict, "ImageMask"), Some(Object::Bool(true))) {
            self.flag("image:stencil-mask");
        } else {
            if let Some(space) = dict.get(&Name::new("ColorSpace")) {
                self.colorspace(space, 0);
            }
            if let Some(bits) = self.number(dict, "BitsPerComponent").filter(|b| *b != 8.0) {
                self.flag(&format!("image:bpc-{bits}"));
            }
        }
        if dict.contains_key(&Name::new("SMask")) {
            self.flag("image:soft-mask");
        }
        match self.value(dict, "Mask") {
            Some(Object::Array(_)) => self.flag("image:color-key-mask"),
            Some(Object::Stream { .. }) => self.flag("image:explicit-mask"),
            _ => {}
        }
        if dict.contains_key(&Name::new("Decode")) {
            self.flag("image:decode-array");
        }
        let pixels = self.number(dict, "Width").unwrap_or(0.0).max(0.0)
            * self.number(dict, "Height").unwrap_or(0.0).max(0.0);
        self.cost += pixels.min(1e12) as u64;
    }

    fn colorspace(&mut self, obj: &Object, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let Some(space) = self.resolve(obj) else {
            return;
        };
        match &space {
            Object::Name(name) => match name.0.as_slice() {
                b"DeviceGray" | b"G" => self.flag("colorspace:DeviceGray"),
                b"DeviceRGB" | b"RGB" => self.flag("colorspace:DeviceRGB"),
                b"DeviceCMYK" | b"CMYK" => self.flag("colorspace:DeviceCMYK"),
                b"Pattern" => self.flag("colorspace:Pattern"),
                _ => {}
            },
            Object::Array(items) => {
                let Some(Ok(Object::Name(family))) = items.first().map(|f| self.doc.resolve(f))
                else {
                    return;
                };
                match family.0.as_slice() {
                    b"ICCBased" => {
                        let components = items
                            .get(1)
                            .and_then(|profile| self.resolve(profile))
                            .and_then(|profile| {
                                profile.as_dict().and_then(|d| self.number(d, "N"))
                            });
                        let components = components.map_or("?".to_string(), |n| n.to_string());
                        self.flag(&format!("colorspace:ICCBased-{components}"));
                    }
                    b"Indexed" | b"I" | b"Pattern" => {
                        self.tag(
                            "colorspace",
                            if family.0 == b"Pattern" {
                                b"Pattern"
                            } else {
                                b"Indexed"
                            },
                        );
                        if let Some(base) = items.get(1) {
                            self.colorspace(base, depth + 1);
                        }
                    }
                    b"Separation" | b"DeviceN" => {
                        self.tag("colorspace", &family.0);
                        if family.0 == b"DeviceN" {
                            if let Some(Object::Dict(attributes)) =
                                items.get(4).and_then(|a| self.resolve(a))
                            {
                                if self.is(&attributes, "Subtype", "NChannel") {
                                    self.flag("colorspace:NChannel");
                                }
                            }
                        }
                        if let Some(tint) = items.get(3) {
                            self.function(tint, depth + 1);
                        }
                    }
                    other => self.tag("colorspace", other),
                }
            }
            _ => {}
        }
    }

    fn function(&mut self, obj: &Object, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        match self.resolve(obj) {
            Some(Object::Array(functions)) => {
                for function in &functions {
                    self.function(function, depth + 1);
                }
            }
            Some(function) => {
                let Some(dict) = function.as_dict() else {
                    return;
                };
                if let Some(kind) = self.number(dict, "FunctionType") {
                    self.flag(&format!("function:type-{kind}"));
                }
                if let Some(parts) = dict.get(&Name::new("Functions")) {
                    self.function(parts, depth + 1);
                }
            }
            None => {}
        }
    }

    fn pattern(&mut self, obj: &Object, depth: usize) {
        let Some(pattern) = self.resolve(obj) else {
            return;
        };
        let Some(dict) = pattern.as_dict() else {
            return;
        };
        match self.number(dict, "PatternType") {
            Some(1.0) => {
                self.flag("pattern:tiling");
                if self.number(dict, "PaintType") == Some(2.0) {
                    self.flag("pattern:uncolored");
                }
                self.form(&pattern, depth + 1);
            }
            Some(2.0) => {
                self.flag("pattern:shading");
                if let Some(shading) = dict.get(&Name::new("Shading")) {
                    self.shading(shading);
                }
                if let Some(state) = dict.get(&Name::new("ExtGState")) {
                    self.graphics_state(state, depth);
                }
            }
            _ => {}
        }
    }

    fn shading(&mut self, obj: &Object) {
        let Some(shading) = self.resolve(obj) else {
            return;
        };
        let Some(dict) = shading.as_dict() else {
            return;
        };
        if let Some(kind) = self.number(dict, "ShadingType") {
            self.flag(&format!("shading:type-{kind}"));
        }
        if let Some(space) = dict.get(&Name::new("ColorSpace")) {
            self.colorspace(space, 0);
        }
        if let Some(function) = dict.get(&Name::new("Function")) {
            self.function(function, 0);
        }
    }

    fn graphics_state(&mut self, obj: &Object, depth: usize) {
        let Some(Object::Dict(state)) = self.resolve(obj) else {
            return;
        };
        if ["CA", "ca"]
            .iter()
            .any(|key| self.number(&state, key).is_some_and(|alpha| alpha < 1.0))
        {
            self.flag("transparency:constant-alpha");
        }
        let blend = |name: &Name| name.0 != b"Normal" && name.0 != b"Compatible";
        match self.value(&state, "BM") {
            Some(Object::Name(mode)) if blend(&mode) => self.tag("blend", &mode.0),
            Some(Object::Array(modes)) => {
                for mode in modes {
                    if let Object::Name(mode) = mode {
                        if blend(&mode) {
                            self.tag("blend", &mode.0);
                        }
                    }
                }
            }
            _ => {}
        }
        if let Some(Object::Dict(mask)) = self.value(&state, "SMask") {
            match self.name(&mask, "S") {
                Some(kind) => self.tag("transparency:soft-mask", &kind),
                None => self.flag("transparency:soft-mask"),
            }
            if let Some(group) = mask.get(&Name::new("G")) {
                if let Some(stream) = self.resolve(group) {
                    self.form(&stream, depth + 1);
                }
            }
        }
        let overprint = ["OP", "op"]
            .iter()
            .any(|key| matches!(self.value(&state, key), Some(Object::Bool(true))));
        if overprint {
            self.flag("graphics-state:overprint");
        }
        for key in ["TR", "TR2"] {
            match self.value(&state, key) {
                Some(Object::Name(name)) if name.0 == b"Identity" || name.0 == b"Default" => {}
                Some(_) => self.flag("graphics-state:transfer-function"),
                None => {}
            }
        }
        if state.contains_key(&Name::new("HT")) {
            self.flag("graphics-state:halftone");
        }
    }

    fn font(&mut self, obj: &Object, depth: usize) {
        let Some(Object::Dict(font)) = self.resolve(obj) else {
            return;
        };
        let Some(subtype) = self.name(&font, "Subtype") else {
            return;
        };
        self.tag("font", &subtype);
        let mut described = font.clone();
        match subtype.as_slice() {
            b"Type0" => {
                match self.value(&font, "Encoding") {
                    Some(Object::Name(name)) if name.0.starts_with(b"Identity-") => {
                        self.flag("cmap:identity");
                    }
                    Some(Object::Name(_)) => self.flag("cmap:predefined"),
                    Some(Object::Stream { .. }) => self.flag("cmap:embedded"),
                    _ => {}
                }
                let descendant = match self.value(&font, "DescendantFonts") {
                    Some(Object::Array(descendants)) => descendants.first().cloned(),
                    _ => None,
                };
                match descendant.and_then(|d| self.resolve(&d)) {
                    Some(Object::Dict(cid_font)) => {
                        if let Some(kind) = self.name(&cid_font, "Subtype") {
                            self.tag("font:Type0", &kind);
                        }
                        described = cid_font;
                    }
                    _ => return,
                }
            }
            b"Type3" => {
                if let Some(resources) = font.get(&Name::new("Resources")) {
                    self.resources(resources, depth + 1);
                }
                if let Some(Object::Dict(glyphs)) = self.value(&font, "CharProcs") {
                    for glyph in glyphs.values().take(16) {
                        if let Some(stream) = self.resolve(glyph) {
                            self.content(&stream);
                        }
                    }
                }
                return;
            }
            _ => {
                if let Some(Object::Dict(encoding)) = self.value(&font, "Encoding") {
                    if encoding.contains_key(&Name::new("Differences")) {
                        self.flag("encoding:differences");
                    }
                }
            }
        }
        let descriptor = described
            .get(&Name::new("FontDescriptor"))
            .and_then(|d| self.resolve(d));
        let embedded = descriptor.as_ref().and_then(Object::as_dict).and_then(|d| {
            if d.contains_key(&Name::new("FontFile")) {
                Some("Type1".to_string())
            } else if d.contains_key(&Name::new("FontFile2")) {
                Some("TrueType".to_string())
            } else {
                d.get(&Name::new("FontFile3"))
                    .map(|file| match self.doc.resolve(file) {
                        Ok(Object::Stream { dict, .. }) => {
                            self.name(&dict, "Subtype").map_or("?".to_string(), |s| {
                                String::from_utf8_lossy(&s).into_owned()
                            })
                        }
                        _ => "?".to_string(),
                    })
            }
        });
        match embedded {
            Some(kind) => self.flag(&format!("fontfile:{kind}")),
            None => {
                let base = self.name(&font, "BaseFont").unwrap_or_default();
                let base = String::from_utf8_lossy(&base).into_owned();
                // A subset is named `ABCDEF+Name` (ISO 32000-2, 9.6.4).
                let plain = match base.split_once('+') {
                    Some((prefix, name)) if prefix.len() == 6 => name.to_string(),
                    _ => base,
                };
                if STANDARD_14.contains(&plain.as_str()) {
                    self.flag("font:standard-14-not-embedded");
                } else {
                    self.flag("font:not-embedded");
                }
            }
        }
    }
}

/// Filters of ISO 32000-2, 7.4, table 6.
const FILTERS: [&str; 10] = [
    "ASCIIHexDecode",
    "ASCII85Decode",
    "LZWDecode",
    "FlateDecode",
    "RunLengthDecode",
    "CCITTFaxDecode",
    "JBIG2Decode",
    "DCTDecode",
    "JPXDecode",
    "Crypt",
];

/// Blend modes of ISO 32000-2, 11.3.5, tables 134 and 135.
const BLEND_MODES: [&str; 17] = [
    "Normal",
    "Compatible",
    "Multiply",
    "Screen",
    "Overlay",
    "Darken",
    "Lighten",
    "ColorDodge",
    "ColorBurn",
    "HardLight",
    "SoftLight",
    "Difference",
    "Exclusion",
    "Hue",
    "Saturation",
    "Color",
    "Luminosity",
];

/// Markup annotations and form fields: without an appearance stream, how
/// they look is the viewer's to draw from their keys (ISO 32000-2, 12.5.6).
const DRAWN_WITHOUT_APPEARANCE: [&str; 17] = [
    "Text",
    "FreeText",
    "Line",
    "Square",
    "Circle",
    "Polygon",
    "PolyLine",
    "Highlight",
    "Underline",
    "Squiggly",
    "StrikeOut",
    "Caret",
    "Ink",
    "Stamp",
    "FileAttachment",
    "Redact",
    "Widget",
];

/// A tag as the selection counts it. A name the norm does not define, for a
/// filter, a blend mode, a colour space, a font or font file, a shading or a
/// function, becomes `unknown`: the files built to test one share one tag
/// instead of taking two pages each. Annotations are told apart by whether
/// they carry their appearance and, without one, only for the types a viewer
/// draws from their keys.
fn normalize(tag: &str) -> String {
    let Some((prefix, name)) = tag.split_once(':') else {
        return tag.to_string();
    };
    let among = |names: &[&str]| {
        if names.contains(&name) {
            tag.to_string()
        } else {
            format!("{prefix}:unknown")
        }
    };
    match prefix {
        "content-filter" => among(&FILTERS),
        "image-filter" if name == "none" => tag.to_string(),
        "image-filter" => among(&FILTERS),
        "blend" => among(&BLEND_MODES),
        "colorspace" => among(&[
            "DeviceGray",
            "DeviceRGB",
            "DeviceCMYK",
            "CalGray",
            "CalRGB",
            "Lab",
            "ICCBased-1",
            "ICCBased-3",
            "ICCBased-4",
            "Indexed",
            "Pattern",
            "Separation",
            "DeviceN",
            "NChannel",
        ]),
        "font" => among(&[
            "Type0",
            "Type1",
            "MMType1",
            "TrueType",
            "Type3",
            "Type0:CIDFontType0",
            "Type0:CIDFontType2",
            "not-embedded",
            "standard-14-not-embedded",
        ]),
        "fontfile" => among(&["Type1", "TrueType", "Type1C", "CIDFontType0C", "OpenType"]),
        "shading" => among(&[
            "type-1", "type-2", "type-3", "type-4", "type-5", "type-6", "type-7",
        ]),
        "function" => among(&["type-0", "type-2", "type-3", "type-4"]),
        "image" => among(&[
            "bpc-1",
            "bpc-2",
            "bpc-4",
            "bpc-16",
            "stencil-mask",
            "soft-mask",
            "color-key-mask",
            "explicit-mask",
            "decode-array",
        ]),
        "text" => among(&[
            "render-mode-1",
            "render-mode-2",
            "render-mode-3",
            "render-mode-4",
            "render-mode-5",
            "render-mode-6",
            "render-mode-7",
        ]),
        "page" => among(&[
            "rotate-90",
            "rotate-180",
            "rotate-270",
            "cropbox",
            "origin-offset",
            "user-unit",
        ]),
        "transparency" => among(&[
            "constant-alpha",
            "group",
            "knockout",
            "page-group",
            "soft-mask",
            "soft-mask:Alpha",
            "soft-mask:Luminosity",
        ]),
        "annotation" => match name.strip_suffix("-no-appearance") {
            None if name == "Widget" => tag.to_string(),
            None => "annotation:appearance".to_string(),
            Some(kind) if DRAWN_WITHOUT_APPEARANCE.contains(&kind) => tag.to_string(),
            Some(_) => "annotation:other-no-appearance".to_string(),
        },
        _ => tag.to_string(),
    }
}

/// The full name of a filter an inline image may abbreviate
/// (ISO 32000-2, 8.9.7, table 92).
fn full_filter_name(name: &[u8]) -> &[u8] {
    match name {
        b"AHx" => b"ASCIIHexDecode",
        b"A85" => b"ASCII85Decode",
        b"LZW" => b"LZWDecode",
        b"Fl" => b"FlateDecode",
        b"RL" => b"RunLengthDecode",
        b"CCF" => b"CCITTFaxDecode",
        b"DCT" => b"DCTDecode",
        other => other,
    }
}

/// Offset just after the `EI` that ends the data of an inline image starting
/// at `from`: preceded by white space, followed by white space or the end.
fn inline_image_end(data: &[u8], from: usize) -> Option<usize> {
    (from.max(1)..data.len().saturating_sub(1)).find_map(|i| {
        let ends = data.get(i..i + 2) == Some(b"EI")
            && data.get(i - 1).copied().is_some_and(is_whitespace)
            && data.get(i + 2).copied().is_none_or(is_whitespace);
        ends.then_some(i + 2)
    })
}

/// Width and height of the page as displayed, in points: its crop box, or
/// its media box, turned by `/Rotate`.
fn displayed_size(doc: &Document<'_>, page: &Dict) -> (f64, f64) {
    let rectangle = |key: &str| -> Option<(f64, f64)> {
        let Some(Ok(Object::Array(corners))) = page.get(&Name::new(key)).map(|r| doc.resolve(r))
        else {
            return None;
        };
        let values: Vec<f64> = corners
            .iter()
            .filter_map(|corner| match doc.resolve(corner) {
                Ok(Object::Integer(n)) => Some(n as f64),
                Ok(Object::Real(x)) => Some(x),
                _ => None,
            })
            .collect();
        match values.as_slice() {
            [x0, y0, x1, y1] if (x1 - x0).abs() > 0.0 && (y1 - y0).abs() > 0.0 => {
                Some(((x1 - x0).abs(), (y1 - y0).abs()))
            }
            _ => None,
        }
    };
    let (width, height) = rectangle("CropBox")
        .or_else(|| rectangle("MediaBox"))
        .unwrap_or((612.0, 792.0));
    match page.get(&Name::new("Rotate")).map(|r| doc.resolve(r)) {
        Some(Ok(Object::Integer(r))) if r.rem_euclid(180) == 90 => (height, width),
        _ => (width, height),
    }
}

/// Open `bytes` with the empty password, or with the user password a qpdf
/// test file names (`U=view`).
fn open<'a>(bytes: &'a [u8], file: &str) -> Result<(Document<'a>, String), String> {
    match Document::open(bytes) {
        Ok(doc) => Ok((doc, String::new())),
        Err(fyp_core::Error::WrongPassword) => {
            let name = file.rsplit('/').next().unwrap_or(file);
            let password = name
                .split([',', '.'])
                .find_map(|part| part.strip_prefix("U="))
                .ok_or_else(|| "mot de passe inconnu".to_string())?;
            Document::open_with_password(bytes, password.as_bytes())
                .map(|doc| (doc, password.to_string()))
                .map_err(|e| e.to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// The candidate pages of `file`: its first `max_pages` pages, unless an
/// image of `width` pixels would be too flat or too tall to look at.
pub fn scan_file(
    root: &Path,
    file: &str,
    max_pages: usize,
    width: u32,
) -> Result<Vec<Candidate>, String> {
    let bytes = std::fs::read(under(root, file)).map_err(|e| format!("{file} : {e}"))?;
    let sha256 = system::sha256_hex(&bytes);
    let (doc, password) = open(&bytes, file)?;
    let mut document = BTreeSet::new();
    if doc.reconstructed().is_some() {
        document.insert("file:repaired".to_string());
    }
    if let Some(encryption) = doc.encryption() {
        let cipher = if encryption.streams == Cipher::Identity {
            encryption.strings
        } else {
            encryption.streams
        };
        let cipher = match cipher {
            Cipher::Identity => "identity",
            Cipher::Rc4 => "rc4",
            Cipher::Aes128 => "aes128",
            Cipher::Aes256 => "aes256",
        };
        document.insert(format!("file:encrypted-{cipher}"));
    }
    if !password.is_empty() {
        document.insert("file:user-password".to_string());
    }
    let pages = ops::pages(&doc).map_err(|e| e.to_string())?;
    let mut candidates = Vec::new();
    for (index, page) in pages.iter().enumerate().take(max_pages) {
        let (w, h) = displayed_size(&doc, &page.dict);
        let height = f64::from(width) * h / w;
        if !(32.0..=4.0 * f64::from(width)).contains(&height) {
            continue;
        }
        let mut scanner = Scanner::new(&doc);
        scanner.page(&page.dict);
        let mut features: BTreeSet<String> =
            scanner.features.iter().map(|tag| normalize(tag)).collect();
        features.extend(document.iter().cloned());
        candidates.push(Candidate {
            file: file.to_string(),
            sha256: sha256.clone(),
            index,
            password: password.clone(),
            features,
            cost: scanner.cost,
        });
    }
    Ok(candidates)
}

/// Scan `files` on `jobs` threads: the candidates, sorted by file and page,
/// and the files that did not open, with why.
pub fn scan(
    root: &Path,
    files: &[String],
    max_pages: usize,
    width: u32,
    jobs: usize,
) -> (Vec<Candidate>, Vec<(String, String)>) {
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1) {
            scope.spawn(|| {
                while let Some(file) = files.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let result = scan_file(root, file, max_pages, width);
                    results
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push((file.clone(), result));
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap_or_else(PoisonError::into_inner);
    results.sort_by(|x, y| x.0.cmp(&y.0));
    let mut candidates = Vec::new();
    let mut refused = Vec::new();
    for (file, result) in results {
        match result {
            Ok(pages) => candidates.extend(pages),
            Err(error) => refused.push((file, error)),
        }
    }
    (candidates, refused)
}

/// How many pages [`choose`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChooseOptions {
    /// Pages wanted for every tag.
    pub per_feature: usize,
    /// Pages at most from one file.
    pub max_per_file: usize,
    /// Pages drawn at random from each lot, besides.
    pub sample_per_lot: usize,
    /// The costliest pages besides, one per file not yet taken: loaded
    /// pages, for times that say something of real documents.
    pub heavy: usize,
    /// Candidates tried for one tag before giving it up.
    pub attempts: usize,
}

/// SplitMix64: the same draws on every machine.
struct Draws(u64);

impl Draws {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Take pages from `candidates`, sorted by file and page (see the module):
/// the positions taken, each with what it was taken for. `probe` says whether
/// the reference engine renders a candidate.
pub fn choose(
    candidates: &[Candidate],
    options: &ChooseOptions,
    probe: &mut dyn FnMut(&Candidate) -> Result<(), String>,
) -> Vec<(usize, Vec<String>)> {
    let mut holders: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (position, candidate) in candidates.iter().enumerate() {
        for feature in &candidate.features {
            holders.entry(feature.as_str()).or_default().push(position);
        }
    }
    let mut features: Vec<(&str, usize)> = holders.iter().map(|(f, h)| (*f, h.len())).collect();
    features.sort_by(|x, y| x.1.cmp(&y.1).then_with(|| x.0.cmp(y.0)));

    let mut covered: BTreeMap<&str, usize> = BTreeMap::new();
    let mut per_file: BTreeMap<&str, usize> = BTreeMap::new();
    let mut tried: BTreeSet<usize> = BTreeSet::new();
    let mut chosen = Vec::new();
    let take = |position: usize,
                why: Vec<String>,
                covered: &mut BTreeMap<_, usize>,
                per_file: &mut BTreeMap<_, usize>,
                chosen: &mut Vec<(usize, Vec<String>)>| {
        let candidate = &candidates[position];
        for feature in &candidate.features {
            *covered.entry(feature.as_str()).or_default() += 1;
        }
        *per_file.entry(candidate.file.as_str()).or_default() += 1;
        chosen.push((position, why));
    };

    for (feature, holding) in features {
        let wanted = options.per_feature.min(holding);
        let mut attempts = 0;
        while covered.get(feature).copied().unwrap_or(0) < wanted && attempts < options.attempts {
            let missing = |position: usize| {
                candidates[position]
                    .features
                    .iter()
                    .filter(|f| covered.get(f.as_str()).copied().unwrap_or(0) < options.per_feature)
                    .count()
            };
            let best = holders
                .get(feature)
                .into_iter()
                .flatten()
                .copied()
                .filter(|p| {
                    !tried.contains(p)
                        && per_file
                            .get(candidates[*p].file.as_str())
                            .copied()
                            .unwrap_or(0)
                            < options.max_per_file
                })
                .max_by(|x, y| {
                    missing(*x)
                        .cmp(&missing(*y))
                        .then_with(|| candidates[*y].cost.cmp(&candidates[*x].cost))
                        .then_with(|| y.cmp(x))
                });
            let Some(best) = best else {
                break;
            };
            attempts += 1;
            tried.insert(best);
            if probe(&candidates[best]).is_ok() {
                let why = candidates[best]
                    .features
                    .iter()
                    .filter(|f| covered.get(f.as_str()).copied().unwrap_or(0) < options.per_feature)
                    .cloned()
                    .collect();
                take(best, why, &mut covered, &mut per_file, &mut chosen);
            }
        }
    }

    // The costliest pages, one per file not yet taken. A file the reference
    // fails on is not tried again, and the probes are bounded.
    let mut costliest: Vec<usize> = (0..candidates.len()).collect();
    costliest.sort_by(|x, y| {
        candidates[*y]
            .cost
            .cmp(&candidates[*x].cost)
            .then_with(|| x.cmp(y))
    });
    let (mut heavy, mut probes) = (0, 0);
    let mut failing: BTreeSet<&str> = BTreeSet::new();
    for position in costliest {
        if heavy == options.heavy || probes == options.heavy * 4 {
            break;
        }
        let file = candidates[position].file.as_str();
        if per_file.contains_key(file) || failing.contains(file) || !tried.insert(position) {
            continue;
        }
        probes += 1;
        if probe(&candidates[position]).is_ok() {
            take(
                position,
                vec!["heavy".to_string()],
                &mut covered,
                &mut per_file,
                &mut chosen,
            );
            heavy += 1;
        } else {
            failing.insert(file);
        }
    }

    let mut lots: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (position, candidate) in candidates.iter().enumerate() {
        lots.entry(lot(&candidate.file)).or_default().push(position);
    }
    let mut draws = Draws(0x4659_5044_4652_4E44);
    for (lot, positions) in lots {
        let mut taken = 0;
        for _ in 0..options.sample_per_lot * 8 {
            if taken == options.sample_per_lot {
                break;
            }
            let Some(&position) = positions.get((draws.next() % positions.len() as u64) as usize)
            else {
                break;
            };
            let full = per_file
                .get(candidates[position].file.as_str())
                .copied()
                .unwrap_or(0)
                >= options.max_per_file;
            if full || !tried.insert(position) {
                continue;
            }
            if probe(&candidates[position]).is_ok() {
                take(
                    position,
                    vec![format!("sample:{lot}")],
                    &mut covered,
                    &mut per_file,
                    &mut chosen,
                );
                taken += 1;
            }
        }
    }
    chosen
}

/// The lot of a corpus file, `tests/corpus/<lot>/…`.
fn lot(file: &str) -> &str {
    file.strip_prefix("tests/corpus/")
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("")
}

/// `file`, relative to `root` with forward slashes, under `root`.
fn under(root: &Path, file: &str) -> PathBuf {
    file.split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

/// `*.pdf` under `dir`, relative to `root`, sorted; in sub-directories too
/// when `recursive`.
pub fn pdf_files(root: &Path, dir: &Path, recursive: bool) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, recursive: bool, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                if recursive {
                    walk(root, &path, recursive, out);
                }
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
            {
                out.push(system::relative(root, &path));
            }
        }
    }
    let mut files = Vec::new();
    walk(root, dir, recursive, &mut files);
    files.sort();
    files
}

/// What `select` is asked to do.
#[derive(Debug, Clone)]
pub struct SelectOptions {
    /// Root of the repository.
    pub root: PathBuf,
    /// Directory of the bench's executable.
    pub bin_dir: PathBuf,
    /// The engines file.
    pub engines_file: PathBuf,
    /// The reference engine, which must render every corpus page taken.
    pub reference: String,
    /// Scratch directory for the probe images.
    pub work: PathBuf,
    /// Width of the images.
    pub width: u32,
    /// How many pages to take.
    pub choose: ChooseOptions,
    /// Pages examined at most per file.
    pub max_pages_scanned: usize,
    /// Longest time the reference may take to draw a page taken.
    pub max_render_ms: f64,
    /// Silence after which the reference is stopped.
    pub timeout: Duration,
    /// Build the reference first.
    pub build: bool,
    /// Threads scanning files.
    pub jobs: usize,
    /// Tell the progress on standard error.
    pub verbose: bool,
}

/// Choose a page set, and say in a comment how it was chosen.
pub fn select(options: &SelectOptions) -> Result<(PageSet, String), String> {
    let engines = engine::load_engines(&options.engines_file)?;
    let spec = engines
        .get(&options.reference)
        .ok_or_else(|| format!("moteur inconnu : {}", options.reference))?;
    let places = Places {
        root: options.root.clone(),
        bin_dir: options.bin_dir.clone(),
    };
    if options.build {
        spec.build(&options.reference, &places)?;
    }
    let tests = options.root.join("tests");
    let fixtures = pdf_files(&options.root, &tests.join("fixtures"), false);
    let corpus = pdf_files(&options.root, &tests.join("corpus"), true);
    if corpus.is_empty() {
        return Err(
            "aucun PDF dans tests/corpus : lancer d'abord python tools/fetch_corpus.py".to_string(),
        );
    }
    let log = |text: String| {
        if options.verbose {
            eprintln!("{text}");
        }
    };
    log(format!(
        "Examen de {} fixtures et {} fichiers du corpus…",
        fixtures.len(),
        corpus.len()
    ));
    let (fixture_pages, fixture_refused) = scan(
        &options.root,
        &fixtures,
        options.max_pages_scanned,
        options.width,
        options.jobs,
    );
    let (candidates, refused) = scan(
        &options.root,
        &corpus,
        options.max_pages_scanned,
        options.width,
        options.jobs,
    );
    let tags: BTreeSet<&String> = candidates.iter().flat_map(|c| &c.features).collect();
    log(format!(
        "{} pages candidates, {} étiquettes, {} fichiers refusés par le noyau",
        candidates.len(),
        tags.len(),
        refused.len()
    ));

    std::fs::create_dir_all(&options.work)
        .map_err(|e| format!("{} : {e}", options.work.display()))?;
    let output = options.work.join("probe.png");
    let (mut probes, mut rejected) = (0, 0);
    let mut probe = |candidate: &Candidate| -> Result<(), String> {
        probes += 1;
        let request = Request {
            protocol: PROTOCOL,
            document: under(&options.root, &candidate.file),
            password: candidate.password.clone(),
            width: options.width,
            repeat: 1,
            pages: vec![PageRequest {
                index: candidate.index,
                output: output.clone(),
            }],
        };
        let run = engine::run_document(spec.command(&places), &request, options.timeout);
        let verdict = match run.pages.first() {
            Some(PageOutcome::Rendered { render_ms, .. })
                if render_ms.iter().all(|t| *t <= options.max_render_ms) =>
            {
                Ok(())
            }
            Some(PageOutcome::Rendered { render_ms, .. }) => {
                Err(format!("rendu trop long ({render_ms:?} ms)"))
            }
            Some(PageOutcome::Failed { error }) => Err(error.clone()),
            None => Err("aucune réponse".to_string()),
        };
        if let Err(error) = &verdict {
            rejected += 1;
            if options.verbose {
                eprintln!(
                    "  écartée : {} page {} : {}",
                    candidate.file,
                    candidate.index + 1,
                    engine::shorten(error, 160)
                );
            }
        }
        verdict
    };
    let chosen = choose(&candidates, &options.choose, &mut probe);

    let mut pages: Vec<PageEntry> = fixture_pages
        .iter()
        .map(|page| PageEntry {
            file: page.file.clone(),
            sha256: page.sha256.clone(),
            index: page.index,
            password: page.password.clone(),
            why: std::iter::once("fixture".to_string())
                .chain(
                    page.features
                        .iter()
                        .filter(|f| f.starts_with("file:"))
                        .cloned(),
                )
                .collect(),
        })
        .collect();
    let mut corpus_pages: Vec<PageEntry> = chosen
        .iter()
        .map(|(position, why)| {
            let candidate = &candidates[*position];
            PageEntry {
                file: candidate.file.clone(),
                sha256: candidate.sha256.clone(),
                index: candidate.index,
                password: candidate.password.clone(),
                why: why.clone(),
            }
        })
        .collect();
    corpus_pages.sort_by(|x, y| x.file.cmp(&y.file).then(x.index.cmp(&y.index)));
    pages.extend(corpus_pages);
    let covered: BTreeSet<&String> = chosen
        .iter()
        .flat_map(|(position, _)| &candidates[*position].features)
        .collect();
    let uncovered: Vec<&str> = tags
        .iter()
        .filter(|tag| !covered.contains(*tag))
        .map(|tag| tag.as_str())
        .collect();

    let mut comment = format!(
        "Jeu de pages de référence du banc de fidélité du rendu (docs/banc-rendu.md,\n« Le jeu de pages »). Choisi par `fyp-render-bench select` le {}, puis\nversionné : ne pas le choisir de nouveau sans raison, deux exécutions ne se\ncomparent que sur le même jeu.\n\n",
        system::UtcTime::now().date()
    );
    comment.push_str(&format!(
        "Fixtures : toutes leurs pages ({}). Corpus : {} pages candidates dans {} fichiers\n({} refusés par le noyau), {} étiquettes ; {} pages par étiquette au plus, {} par\nfichier au plus ; {} pages tirées au hasard par lot (graine fixe), et les {} plus\nlourdes, une par fichier ; seulement des pages que {} rend en {} ms au plus\n({} essais, {} écartées). Largeur : {} pixels.\n",
        fixture_pages.len(),
        candidates.len(),
        corpus.len(),
        refused.len(),
        tags.len(),
        options.choose.per_feature,
        options.choose.max_per_file,
        options.choose.sample_per_lot,
        options.choose.heavy,
        options.reference,
        options.max_render_ms,
        probes,
        rejected,
        options.width
    ));
    for lot in ["pdfjs", "qpdf", "verapdf"] {
        let marker = tests.join("corpus").join(lot).join(".source.json");
        if let Ok(text) = std::fs::read_to_string(marker) {
            if let Ok(source) = serde_json::from_str::<serde_json::Value>(&text) {
                comment.push_str(&format!(
                    "Lot {lot} : {}@{}.\n",
                    source["repo"].as_str().unwrap_or("?"),
                    source["sha"]
                        .as_str()
                        .and_then(|s| s.get(..12))
                        .unwrap_or("?")
                ));
            }
        }
    }
    if !uncovered.is_empty() {
        comment.push_str(&format!(
            "Étiquettes sans page (aucune page rendue par la référence) : {}.\n",
            uncovered.join(", ")
        ));
    }
    if !fixture_refused.is_empty() {
        let names: Vec<&str> = fixture_refused
            .iter()
            .map(|(file, _)| file.as_str())
            .collect();
        comment.push_str(&format!(
            "Fixtures que le noyau n'ouvre pas, absentes : {}.\n",
            names.join(", ")
        ));
    }
    let set = PageSet {
        format: FORMAT,
        width: options.width,
        pages,
    };
    Ok((set, comment))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        system::repository_root()
    }

    #[test]
    fn fixtures_are_described() {
        let features = |file: &str| {
            let pages = scan_file(&root(), file, 8, 1400).unwrap();
            assert_eq!(pages.len(), 1, "{file}");
            pages[0].features.clone()
        };
        assert!(features("tests/fixtures/bad-offsets.pdf").contains("file:repaired"));
        assert!(features("tests/fixtures/encrypted-rc4.pdf").contains("file:encrypted-rc4"));
        assert!(features("tests/fixtures/encrypted-aes256.pdf").contains("file:encrypted-aes256"));
        assert!(!features("tests/fixtures/minimal.pdf")
            .iter()
            .any(|f| f.starts_with("file:")));
        assert!(scan_file(&root(), "tests/fixtures/absent.pdf", 8, 1400).is_err());
    }

    #[test]
    fn qpdf_files_name_their_user_password() {
        let file = "tests/corpus/qpdf/enc-R3,V2,U=view,O=master.pdf";
        let Ok(bytes) = std::fs::read(root().join(file)) else {
            eprintln!("corpus not fetched: skipped");
            return;
        };
        let (doc, password) = open(&bytes, file).unwrap();
        assert_eq!(password, "view");
        assert!(doc.encryption().is_some());
        assert!(open(&bytes, "tests/corpus/qpdf/enc.pdf").is_err());
    }

    #[test]
    fn inline_images_are_skipped() {
        let data = b"BI /W 2 /H 1 /F /DCT ID \x00EI\xffEI Q EI\nq";
        // NUL is white space, but the `EI` after it is followed by 0xFF, and
        // the next one follows 0xFF: the data ends at the `EI` after `Q`.
        assert_eq!(inline_image_end(data, 23), Some(35));
        assert_eq!(inline_image_end(b"ID xxxx", 3), None);
        assert_eq!(full_filter_name(b"DCT"), b"DCTDecode");
    }

    fn candidate(file: &str, index: usize, features: &[&str], cost: u64) -> Candidate {
        Candidate {
            file: file.to_string(),
            sha256: String::new(),
            index,
            password: String::new(),
            features: features.iter().map(|f| f.to_string()).collect(),
            cost,
        }
    }

    #[test]
    fn names_the_norm_does_not_define_share_one_tag() {
        for (raw, counted) in [
            ("image-filter:DCTDecode", "image-filter:DCTDecode"),
            ("image-filter:none", "image-filter:none"),
            ("image-filter:Lzw", "image-filter:unknown"),
            ("content-filter:A85", "content-filter:unknown"),
            ("blend:Custom", "blend:unknown"),
            ("colorspace:ICCBased-?", "colorspace:unknown"),
            ("font:Type0:CIDFontType2", "font:Type0:CIDFontType2"),
            ("fontfile:?", "fontfile:unknown"),
            ("annotation:Highlight", "annotation:appearance"),
            ("annotation:Widget", "annotation:Widget"),
            (
                "annotation:Ink-no-appearance",
                "annotation:Ink-no-appearance",
            ),
            (
                "annotation:Sound-no-appearance",
                "annotation:other-no-appearance",
            ),
            ("file:repaired", "file:repaired"),
            ("content:text", "content:text"),
        ] {
            assert_eq!(normalize(raw), counted, "{raw}");
        }
    }

    #[test]
    fn the_heaviest_pages_come_one_per_file() {
        let candidates = vec![
            candidate("tests/corpus/a/1.pdf", 0, &["content:text"], 900),
            candidate("tests/corpus/a/1.pdf", 1, &["content:text"], 800),
            candidate("tests/corpus/a/2.pdf", 0, &["content:text"], 700),
            candidate("tests/corpus/a/3.pdf", 0, &["content:text"], 600),
            candidate("tests/corpus/a/4.pdf", 0, &["content:text"], 5),
        ];
        let options = ChooseOptions {
            per_feature: 1,
            max_per_file: 2,
            sample_per_lot: 0,
            heavy: 2,
            attempts: 3,
        };
        // The reference does not render file 2.
        let mut probe = |c: &Candidate| {
            if c.file.ends_with("2.pdf") {
                Err("trop lent".to_string())
            } else {
                Ok(())
            }
        };
        let chosen = choose(&candidates, &options, &mut probe);
        // The cheapest page covers the tag; then the heaviest page of file 1,
        // not its second one, and of file 3, file 2 failing.
        let positions: Vec<usize> = chosen.iter().map(|(p, _)| *p).collect();
        assert_eq!(positions, [4, 0, 3]);
        assert_eq!(chosen[1].1, ["heavy"]);
    }

    #[test]
    fn choice_covers_rare_tags_first_and_is_repeatable() {
        let candidates = vec![
            candidate(
                "tests/corpus/a/1.pdf",
                0,
                &["font:Type3", "content:text"],
                50,
            ),
            candidate("tests/corpus/a/1.pdf", 1, &["content:text"], 10),
            candidate(
                "tests/corpus/a/2.pdf",
                0,
                &["shading:type-4", "content:text"],
                80,
            ),
            candidate("tests/corpus/b/3.pdf", 0, &["font:Type3"], 5),
            candidate("tests/corpus/b/4.pdf", 0, &["content:text"], 1),
        ];
        let options = ChooseOptions {
            per_feature: 1,
            max_per_file: 1,
            sample_per_lot: 0,
            heavy: 0,
            attempts: 3,
        };
        let mut all = |_: &Candidate| Ok(());
        let chosen = choose(&candidates, &options, &mut all);
        let positions: Vec<usize> = chosen.iter().map(|(p, _)| *p).collect();
        // The rarest tag first (shading), then Type3: the page that also has
        // text is not needed, the cheaper one is taken.
        assert_eq!(positions, [2, 3]);
        assert_eq!(chosen[0].1, ["content:text", "shading:type-4"]);
        assert_eq!(choose(&candidates, &options, &mut all), chosen);
        // A page the reference does not render is replaced.
        let mut not_three = |c: &Candidate| {
            if c.file.ends_with("3.pdf") {
                Err("no".to_string())
            } else {
                Ok(())
            }
        };
        let positions: Vec<usize> = choose(&candidates, &options, &mut not_three)
            .iter()
            .map(|(p, _)| *p)
            .collect();
        assert_eq!(positions, [2, 0]);
    }
}
