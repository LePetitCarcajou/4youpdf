//! Page operations (milestone 0.2): merge, extract, split, rotate,
//! delete. Every operation builds a fresh document out of the pages it
//! keeps and writes it with [`crate::writer`]:
//!
//! - the page tree of the output is flat: one `/Pages` node, the kept
//!   pages in the requested order (ISO 32000-2, 7.7.3);
//! - the inheritable attributes (`/Resources`, `/MediaBox`, `/CropBox`,
//!   `/Rotate`, table 30) are resolved into each page, so a page means the
//!   same thing whatever tree it came from;
//! - only the objects reachable from the kept pages, the catalog and the
//!   information dictionary are copied, under new numbers, so two
//!   documents never collide and nothing dropped is carried along;
//! - a reference to a page that is not kept becomes `null`, then the
//!   structures that carried it are cleaned: a link annotation without a
//!   destination is dropped, a `/Dest` or `/GoTo` action to a dropped page
//!   is removed, a named destination to a dropped page is forgotten.
//!
//! An encrypted source is deciphered by [`Document`] and written in the
//! clear, as [`crate::writer`] does for a plain rewrite: callers that
//! report on the operation must say so.
//!
//! Known losses, by design of this first version: the output keeps the
//! catalog of the first document only, so when merging, `/PageLabels`,
//! `/StructTreeRoot`, `/Metadata`, `/OCProperties` and viewer preferences
//! of the other documents are not carried over (their pages, resources,
//! annotations, outlines, named destinations and form fields are). A page
//! listed twice in a page tree is taken once. Page indices are 0-based.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::ops::Range;

use crate::document::Document;
use crate::object::{Dict, Name, ObjRef, Object};
use crate::version::PdfVersion;
use crate::writer::Writer;
use crate::{Error, Result};

/// Attributes a page inherits from its ancestors (ISO 32000-2, table 30).
const INHERITABLE: [&str; 4] = ["Resources", "MediaBox", "CropBox", "Rotate"];

/// A page of a source document, its inheritable attributes resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// The page object, when the page tree holds it by reference (the
    /// normal case). `None` for a page dictionary written directly in a
    /// `/Kids` array.
    pub reference: Option<ObjRef>,
    /// The page dictionary with the inherited attributes filled in and
    /// `/Parent` left out: what the page is, independently of the tree.
    pub dict: Dict,
}

/// The pages of `doc` in reading order, inheritable attributes resolved
/// (7.7.3.4). Tolerant: a node without `/Type` is a page unless it has
/// `/Kids`, a kid that is not a dictionary is skipped, a node reached
/// twice (loop, or a page listed twice) is taken once.
pub fn pages(doc: &Document<'_>) -> Result<Vec<Page>> {
    Ok(walk(doc)?.0)
}

/// The pages of `doc` and the references of the intermediate nodes of its
/// page tree (which the output never copies).
fn walk(doc: &Document<'_>) -> Result<(Vec<Page>, Vec<ObjRef>)> {
    let catalog = doc.catalog()?;
    let root = catalog
        .get(&Name::new("Pages"))
        .cloned()
        .ok_or_else(|| structure("catalog has no /Pages"))?;
    let mut pages = Vec::new();
    let mut nodes = Vec::new();
    let mut visited = HashSet::new();
    // Explicit stack: a deep or degenerate tree must not overflow ours.
    let mut stack = vec![(root, Dict::new())];
    while let Some((node, inherited)) = stack.pop() {
        let (reference, dict) = match node {
            Object::Reference(r) => {
                if !visited.insert(r) {
                    continue;
                }
                match doc.get(r)? {
                    Some(Object::Dict(d)) => (Some(r), d),
                    _ => continue,
                }
            }
            Object::Dict(d) => (None, d),
            _ => continue,
        };
        let mut attrs = inherited;
        for key in INHERITABLE {
            if let Some(value) = dict.get(&Name::new(key)) {
                attrs.insert(Name::new(key), value.clone());
            }
        }
        match dict.get(&Name::new("Kids")) {
            Some(kids) => {
                if let Some(r) = reference {
                    nodes.push(r);
                }
                if let Object::Array(kids) = doc.resolve(kids)? {
                    for kid in kids.into_iter().rev() {
                        stack.push((kid, attrs.clone()));
                    }
                }
            }
            None => {
                let mut page = dict;
                page.remove(&Name::new("Parent"));
                for (key, value) in attrs {
                    page.entry(key).or_insert(value);
                }
                pages.push(Page {
                    reference,
                    dict: page,
                });
            }
        }
    }
    if pages.is_empty() {
        return Err(structure("the page tree holds no page"));
    }
    Ok((pages, nodes))
}

/// Concatenate `documents` in order: every page of the first, then of the
/// second, and so on. The catalog, information dictionary and `/ID` come
/// from the first document; outlines are chained, named destinations and
/// form fields are merged (the first document wins on a name clash).
pub fn merge(documents: &[Document<'_>]) -> Result<Vec<u8>> {
    if documents.is_empty() {
        return Err(operation("no document to merge"));
    }
    let mut builder = Builder::new(documents)?;
    let keep: Vec<(usize, usize)> = builder
        .sources
        .iter()
        .enumerate()
        .flat_map(|(i, s)| (0..s.pages.len()).map(move |p| (i, p)))
        .collect();
    let version = documents
        .iter()
        .map(Document::version)
        .max()
        .unwrap_or(PdfVersion { major: 1, minor: 4 });
    builder.build(&keep, version, |_, _| {})
}

/// A document holding only the pages at `indices` (0-based), in that
/// order. Reordering is extracting in another order. Every index must
/// exist and appear once.
pub fn extract_pages(doc: &Document<'_>, indices: &[usize]) -> Result<Vec<u8>> {
    let mut builder = Builder::new(std::slice::from_ref(doc))?;
    check_selection(indices, builder.sources[0].pages.len(), false)?;
    let keep: Vec<(usize, usize)> = indices.iter().map(|&p| (0, p)).collect();
    builder.build(&keep, doc.version(), |_, _| {})
}

/// The document without the pages at `indices` (0-based). At least one
/// page must remain.
pub fn delete_pages(doc: &Document<'_>, indices: &[usize]) -> Result<Vec<u8>> {
    let mut builder = Builder::new(std::slice::from_ref(doc))?;
    let count = builder.sources[0].pages.len();
    check_selection(indices, count, true)?;
    let dropped: BTreeSet<usize> = indices.iter().copied().collect();
    let keep: Vec<(usize, usize)> = (0..count)
        .filter(|p| !dropped.contains(p))
        .map(|p| (0, p))
        .collect();
    if keep.is_empty() {
        return Err(operation("deleting every page would leave no page"));
    }
    builder.build(&keep, doc.version(), |_, _| {})
}

/// The document with the pages at `indices` (0-based) turned by `degrees`
/// clockwise, a multiple of 90, added to their current `/Rotate` and
/// normalised into `0..360` (7.7.3.3). An empty selection turns nothing.
pub fn rotate(doc: &Document<'_>, indices: &[usize], degrees: i32) -> Result<Vec<u8>> {
    if degrees % 90 != 0 {
        return Err(operation(format!(
            "rotation must be a multiple of 90 degrees, not {degrees}"
        )));
    }
    let mut builder = Builder::new(std::slice::from_ref(doc))?;
    let count = builder.sources[0].pages.len();
    check_selection(indices, count, true)?;
    let selected: BTreeSet<usize> = indices.iter().copied().collect();
    let keep: Vec<(usize, usize)> = (0..count).map(|p| (0, p)).collect();
    builder.build(&keep, doc.version(), |position, page| {
        if !selected.contains(&position) {
            return;
        }
        let rotate = Name::new("Rotate");
        let current = match page.get(&rotate) {
            Some(Object::Integer(i)) => i32::try_from(*i).unwrap_or(0),
            Some(Object::Real(f)) => *f as i32,
            _ => 0,
        };
        let turned = (current + degrees).rem_euclid(360);
        if turned == 0 {
            page.remove(&rotate);
        } else {
            page.insert(rotate, Object::Integer(i64::from(turned)));
        }
    })
}

/// One document per range of `ranges` (0-based, end excluded), each
/// holding those pages in order. Ranges may overlap or leave pages out;
/// an empty range or one past the last page is an error.
pub fn split(doc: &Document<'_>, ranges: &[Range<usize>]) -> Result<Vec<Vec<u8>>> {
    let mut parts = Vec::with_capacity(ranges.len());
    for range in ranges {
        if range.is_empty() {
            return Err(operation(format!(
                "empty page range {}..{}",
                range.start, range.end
            )));
        }
        let indices: Vec<usize> = range.clone().collect();
        parts.push(extract_pages(doc, &indices)?);
    }
    Ok(parts)
}

/// Ranges of `every` pages covering `page_count` pages, the last one
/// shorter when the count is not a multiple. Empty when either is zero.
pub fn ranges_every(page_count: usize, every: usize) -> Vec<Range<usize>> {
    if every == 0 {
        return Vec::new();
    }
    (0..page_count)
        .step_by(every)
        .map(|start| start..(start + every).min(page_count))
        .collect()
}

/// Every index exists and appears once; a selection may be empty only
/// when `allow_empty`.
fn check_selection(indices: &[usize], count: usize, allow_empty: bool) -> Result<()> {
    if indices.is_empty() && !allow_empty {
        return Err(operation("no page selected"));
    }
    let mut seen = BTreeSet::new();
    for &index in indices {
        if index >= count {
            return Err(Error::NoSuchPage { index, count });
        }
        if !seen.insert(index) {
            return Err(operation(format!("page {index} selected more than once")));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Building the output
// ---------------------------------------------------------------------------

/// A source document with its page list.
struct Source<'d, 'a> {
    doc: &'d Document<'a>,
    pages: Vec<Page>,
}

/// Copies objects from the sources into a new numbering, on demand.
struct Builder<'d, 'a> {
    sources: Vec<Source<'d, 'a>>,
    /// Source object (document index, reference) to output number.
    map: HashMap<(usize, ObjRef), u32>,
    /// Pages not kept and page-tree nodes: a reference to them is `null`.
    dropped: HashSet<(usize, ObjRef)>,
    /// Objects given a number but not copied yet.
    queue: VecDeque<(usize, ObjRef, u32)>,
    out: BTreeMap<u32, Object>,
    next: u32,
}

impl<'d, 'a> Builder<'d, 'a> {
    fn new(documents: &'d [Document<'a>]) -> Result<Self> {
        let mut sources = Vec::with_capacity(documents.len());
        let mut dropped = HashSet::new();
        for (i, doc) in documents.iter().enumerate() {
            let (pages, nodes) = walk(doc)?;
            for page in &pages {
                if let Some(r) = page.reference {
                    dropped.insert((i, r));
                }
            }
            // Intermediate page-tree nodes are never copied: the output
            // has its own tree.
            for r in nodes {
                dropped.insert((i, r));
            }
            sources.push(Source { doc, pages });
        }
        Ok(Builder {
            sources,
            map: HashMap::new(),
            dropped,
            queue: VecDeque::new(),
            out: BTreeMap::new(),
            next: 1,
        })
    }

    fn alloc(&mut self) -> Result<u32> {
        let num = self.next;
        self.next = num
            .checked_add(1)
            .ok_or_else(|| operation("too many objects for one file"))?;
        Ok(num)
    }

    /// Give a source object its output number without copying it, for
    /// objects the builder writes itself (kept pages, outline roots).
    fn assign(&mut self, i: usize, r: ObjRef) -> Result<u32> {
        let num = self.alloc()?;
        self.map.insert((i, r), num);
        self.dropped.remove(&(i, r));
        Ok(num)
    }

    /// Output object for a source reference: mapped, queued for copy, or
    /// `null` for a dropped page or page-tree node.
    fn map_ref(&mut self, i: usize, r: ObjRef) -> Result<Object> {
        if let Some(&num) = self.map.get(&(i, r)) {
            return Ok(Object::Reference(ObjRef { num, gen: 0 }));
        }
        if self.dropped.contains(&(i, r)) {
            return Ok(Object::Null);
        }
        let num = self.alloc()?;
        self.map.insert((i, r), num);
        self.queue.push_back((i, r, num));
        Ok(Object::Reference(ObjRef { num, gen: 0 }))
    }

    /// A direct object of document `i`, its references remapped. Depth is
    /// bounded by the parser ([`crate::parser::MAX_DEPTH`]).
    fn copy(&mut self, i: usize, obj: &Object) -> Result<Object> {
        Ok(match obj {
            Object::Reference(r) => self.map_ref(i, *r)?,
            Object::Array(items) => Object::Array(
                items
                    .iter()
                    .map(|item| self.copy(i, item))
                    .collect::<Result<Vec<_>>>()?,
            ),
            Object::Dict(dict) => Object::Dict(self.copy_dict(i, dict)?),
            Object::Stream { dict, data } => {
                // The writer sets `/Length` from the data: an indirect
                // length object of the source must not even get a number.
                let mut dict = dict.clone();
                dict.remove(&Name::new("Length"));
                Object::Stream {
                    dict: self.copy_dict(i, &dict)?,
                    data: data.clone(),
                }
            }
            other => other.clone(),
        })
    }

    fn copy_dict(&mut self, i: usize, dict: &Dict) -> Result<Dict> {
        dict.iter()
            .map(|(key, value)| Ok((key.clone(), self.copy(i, value)?)))
            .collect()
    }

    /// Copy every queued object. A source object that cannot be read
    /// becomes `null`: the references to it stay valid.
    fn drain(&mut self) -> Result<()> {
        while let Some((i, r, num)) = self.queue.pop_front() {
            let copied = match self.sources[i].doc.get(r) {
                Ok(Some(obj)) => self.copy(i, &obj)?,
                _ => Object::Null,
            };
            self.out.insert(num, copied);
        }
        Ok(())
    }

    /// Build and write the document made of `keep`, pairs `(document,
    /// page index)` in output order. `modify` is applied to each kept page
    /// dictionary with its position in the output.
    fn build(
        &mut self,
        keep: &[(usize, usize)],
        version: PdfVersion,
        modify: impl Fn(usize, &mut Dict),
    ) -> Result<Vec<u8>> {
        let root_num = self.alloc()?;
        let root_ref = Object::Reference(ObjRef {
            num: root_num,
            gen: 0,
        });
        // Numbers first, so that references between kept pages (links,
        // annotations' /P) resolve whatever the order.
        let mut numbers = Vec::with_capacity(keep.len());
        for &(i, p) in keep {
            let page = self.sources[i]
                .pages
                .get(p)
                .ok_or(Error::NoSuchPage {
                    index: p,
                    count: self.sources[i].pages.len(),
                })?
                .clone();
            let num = match page.reference {
                Some(r) => self.assign(i, r)?,
                None => self.alloc()?,
            };
            numbers.push((num, i, page));
        }
        let mut kids = Vec::with_capacity(numbers.len());
        for (position, (num, i, page)) in numbers.into_iter().enumerate() {
            let mut dict = self.copy_dict(i, &page.dict)?;
            dict.insert(Name::new("Type"), Object::Name(Name::new("Page")));
            dict.insert(Name::new("Parent"), root_ref.clone());
            modify(position, &mut dict);
            self.out.insert(num, Object::Dict(dict));
            kids.push(Object::Reference(ObjRef { num, gen: 0 }));
        }
        let mut pages_root = Dict::new();
        pages_root.insert(Name::new("Type"), Object::Name(Name::new("Pages")));
        pages_root.insert(
            Name::new("Count"),
            Object::Integer(i64::try_from(kids.len()).unwrap_or(i64::MAX)),
        );
        pages_root.insert(Name::new("Kids"), Object::Array(kids));
        self.out.insert(root_num, Object::Dict(pages_root));

        // The catalog of the first document, its /Pages replaced.
        let catalog_num = self.alloc()?;
        let source_catalog = self.sources[0].doc.catalog()?;
        let mut catalog = self.copy_dict(0, &source_catalog)?;
        catalog.insert(Name::new("Pages"), root_ref);
        if self.sources.len() > 1 {
            self.merge_outlines(&mut catalog)?;
            self.merge_form_fields(&mut catalog)?;
        }
        self.out.insert(catalog_num, Object::Dict(catalog));
        let mut trailer = Dict::new();
        trailer.insert(
            Name::new("Root"),
            Object::Reference(ObjRef {
                num: catalog_num,
                gen: 0,
            }),
        );
        let source_trailer = self.sources[0].doc.trailer();
        if let Some(Object::Reference(r)) = source_trailer.get(&Name::new("Info")) {
            if let Object::Reference(info) = self.map_ref(0, *r)? {
                trailer.insert(Name::new("Info"), Object::Reference(info));
            }
        }
        if let Some(id) = source_trailer.get(&Name::new("ID")) {
            trailer.insert(Name::new("ID"), id.clone());
        }
        self.drain()?;
        if self.sources.len() > 1 {
            self.merge_named_destinations(catalog_num)?;
            self.drain()?;
        }
        clean_dead_destinations(&mut self.out, catalog_num);
        sweep(&mut self.out, &trailer);
        Writer::new(version).write_objects(&self.out, &trailer)
    }

    /// Chain the outline trees of the sources into one (12.3.3): the first
    /// items of each document follow the last items of the previous one,
    /// under a single `/Outlines` root.
    fn merge_outlines(&mut self, catalog: &mut Dict) -> Result<()> {
        let outlines_key = Name::new("Outlines");
        let root_num = self.alloc()?;
        let root_ref = Object::Reference(ObjRef {
            num: root_num,
            gen: 0,
        });
        // (first, last) items of each document, in output numbering, and
        // the top-level items whose /Parent must become the new root.
        let mut chains: Vec<(ObjRef, ObjRef)> = Vec::new();
        let mut top_level: Vec<u32> = Vec::new();
        let mut count = 0i64;
        for i in 0..self.sources.len() {
            let doc = self.sources[i].doc;
            let entry = match doc.catalog()?.get(&outlines_key) {
                Some(entry) => entry.clone(),
                None => continue,
            };
            if let Object::Reference(r) = entry {
                // The old root is never copied: its items point at the new one.
                self.map.insert((i, r), root_num);
            }
            let Object::Dict(root) = doc.resolve(&entry)? else {
                continue;
            };
            let (Some(Object::Reference(first)), Some(Object::Reference(last))) =
                (root.get(&Name::new("First")), root.get(&Name::new("Last")))
            else {
                continue;
            };
            if let Some(Object::Integer(n)) = root.get(&Name::new("Count")) {
                count = count.saturating_add(*n);
            }
            // Walk the top-level chain in the source, bounded by a visited set.
            let mut seen = HashSet::new();
            let mut item = *first;
            let mut last_seen = *first;
            while seen.insert(item) {
                if let Object::Reference(n) = self.map_ref(i, item)? {
                    top_level.push(n.num);
                }
                last_seen = item;
                let next = match doc.get(item)? {
                    Some(Object::Dict(d)) => d.get(&Name::new("Next")).cloned(),
                    _ => None,
                };
                match next {
                    Some(Object::Reference(n)) => item = n,
                    _ => break,
                }
            }
            let (Object::Reference(first_out), Object::Reference(last_out)) =
                (self.map_ref(i, *first)?, self.map_ref(i, *last)?)
            else {
                continue;
            };
            // Prefer the last item actually reached over a wrong /Last.
            let last_out = match self.map_ref(i, last_seen)? {
                Object::Reference(r) if !seen.contains(last) => r,
                _ => last_out,
            };
            chains.push((first_out, last_out));
        }
        if chains.is_empty() {
            catalog.remove(&outlines_key);
            return Ok(());
        }
        self.drain()?;
        for pair in chains.windows(2) {
            let (prev_last, next_first) = (pair[0].1, pair[1].0);
            if let Some(Object::Dict(d)) = self.out.get_mut(&prev_last.num) {
                d.insert(Name::new("Next"), Object::Reference(next_first));
            }
            if let Some(Object::Dict(d)) = self.out.get_mut(&next_first.num) {
                d.insert(Name::new("Prev"), Object::Reference(prev_last));
            }
        }
        for num in top_level {
            if let Some(Object::Dict(d)) = self.out.get_mut(&num) {
                d.insert(Name::new("Parent"), root_ref.clone());
            }
        }
        let mut root = Dict::new();
        root.insert(Name::new("Type"), Object::Name(Name::new("Outlines")));
        if let (Some(first), Some(last)) = (chains.first(), chains.last()) {
            root.insert(Name::new("First"), Object::Reference(first.0));
            root.insert(Name::new("Last"), Object::Reference(last.1));
        }
        root.insert(Name::new("Count"), Object::Integer(count));
        self.out.insert(root_num, Object::Dict(root));
        catalog.insert(outlines_key, root_ref);
        Ok(())
    }

    /// One `/AcroForm` holding the fields of every source (12.7.3): the
    /// dictionary of the first document that has one, its `/Fields`
    /// extended with the others'.
    fn merge_form_fields(&mut self, catalog: &mut Dict) -> Result<()> {
        let key = Name::new("AcroForm");
        let mut base: Option<Dict> = None;
        let mut fields = Vec::new();
        for i in 0..self.sources.len() {
            let doc = self.sources[i].doc;
            let Some(entry) = doc.catalog()?.get(&key).cloned() else {
                continue;
            };
            let Object::Dict(form) = doc.resolve(&entry)? else {
                continue;
            };
            if let Some(Object::Array(items)) = form
                .get(&Name::new("Fields"))
                .map(|f| doc.resolve(f))
                .transpose()?
            {
                for item in &items {
                    fields.push(self.copy(i, item)?);
                }
            }
            if base.is_none() {
                base = Some(self.copy_dict(i, &form)?);
            }
        }
        let Some(mut form) = base else {
            return Ok(());
        };
        form.insert(Name::new("Fields"), Object::Array(fields));
        let num = self.alloc()?;
        self.out.insert(num, Object::Dict(form));
        catalog.insert(key, Object::Reference(ObjRef { num, gen: 0 }));
        Ok(())
    }

    /// One name tree of destinations for every source (12.3.2.3): the
    /// `/Dests` dictionaries of PDF 1.1 and the `/Names /Dests` trees are
    /// flattened into a single leaf node, sorted, the first document
    /// winning on a duplicate name. Runs after the first drain: the
    /// catalog's `/Names` dictionary may be an object already copied.
    fn merge_named_destinations(&mut self, catalog_num: u32) -> Result<()> {
        let mut merged: BTreeMap<Vec<u8>, Object> = BTreeMap::new();
        let mut found = false;
        for i in 0..self.sources.len() {
            let doc = self.sources[i].doc;
            let catalog = doc.catalog()?;
            if let Some(Object::Dict(dests)) = catalog
                .get(&Name::new("Dests"))
                .map(|d| doc.resolve(d))
                .transpose()?
            {
                found = true;
                for (name, value) in &dests {
                    if let Entry::Vacant(slot) = merged.entry(name.0.clone()) {
                        slot.insert(self.copy(i, value)?);
                    }
                }
            }
            if let Some(Object::Dict(names)) = catalog
                .get(&Name::new("Names"))
                .map(|d| doc.resolve(d))
                .transpose()?
            {
                if let Some(tree) = names.get(&Name::new("Dests")) {
                    found = true;
                    for (name, value) in name_tree_pairs(doc, tree)? {
                        if let Entry::Vacant(slot) = merged.entry(name) {
                            slot.insert(self.copy(i, &value)?);
                        }
                    }
                }
            }
        }
        if !found {
            return Ok(());
        }
        let mut leaf = Vec::with_capacity(merged.len() * 2);
        for (name, value) in merged {
            leaf.push(Object::String(name));
            leaf.push(value);
        }
        let mut node = Dict::new();
        node.insert(Name::new("Names"), Object::Array(leaf));
        let Some(Object::Dict(catalog)) = self.out.get_mut(&catalog_num) else {
            return Ok(());
        };
        catalog.remove(&Name::new("Dests"));
        match catalog.get(&Name::new("Names")).cloned() {
            Some(Object::Reference(r)) => {
                if let Some(Object::Dict(names)) = self.out.get_mut(&r.num) {
                    names.insert(Name::new("Dests"), Object::Dict(node));
                }
            }
            Some(Object::Dict(mut names)) => {
                names.insert(Name::new("Dests"), Object::Dict(node));
                catalog.insert(Name::new("Names"), Object::Dict(names));
            }
            _ => {
                let mut names = Dict::new();
                names.insert(Name::new("Dests"), Object::Dict(node));
                catalog.insert(Name::new("Names"), Object::Dict(names));
            }
        }
        Ok(())
    }
}

/// Every `(name, value)` pair of a name tree (7.9.6), in tree order.
/// Tolerant: a node reached twice is skipped, a malformed node ignored.
fn name_tree_pairs(doc: &Document<'_>, root: &Object) -> Result<Vec<(Vec<u8>, Object)>> {
    let mut pairs = Vec::new();
    let mut visited = HashSet::new();
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if let Object::Reference(r) = node {
            if !visited.insert(r) {
                continue;
            }
        }
        let Object::Dict(dict) = doc.resolve(&node)? else {
            continue;
        };
        if let Some(Object::Array(kids)) = dict
            .get(&Name::new("Kids"))
            .map(|k| doc.resolve(k))
            .transpose()?
        {
            for kid in kids.into_iter().rev() {
                stack.push(kid);
            }
        }
        if let Some(Object::Array(names)) = dict
            .get(&Name::new("Names"))
            .map(|n| doc.resolve(n))
            .transpose()?
        {
            for pair in names.chunks(2) {
                if let [Object::String(name), value] = pair {
                    pairs.push((name.clone(), value.clone()));
                }
            }
        }
    }
    Ok(pairs)
}

// ---------------------------------------------------------------------------
// Cleaning what pointed at dropped pages
// ---------------------------------------------------------------------------

/// Remove what a dropped page leaves behind: `/Dest` entries and `/GoTo`
/// actions whose destination array starts with `null`, named
/// destinations of that kind, link annotations left with neither `/Dest`
/// nor `/A`, and `null` entries of `/Annots`. Decisions are taken on a
/// snapshot so that the order of the walk does not matter.
fn clean_dead_destinations(out: &mut BTreeMap<u32, Object>, catalog_num: u32) {
    let snapshot = out.clone();
    for obj in out.values_mut() {
        clean_object(obj, &snapshot);
    }
    // The PDF 1.1 `/Dests` dictionary is recognised by its place in the
    // catalog, not by its shape: when indirect, clean it here.
    let old_dests = match snapshot.get(&catalog_num) {
        Some(Object::Dict(catalog)) => catalog.get(&Name::new("Dests")).cloned(),
        _ => None,
    };
    if let Some(Object::Reference(r)) = old_dests {
        if let Some(Object::Dict(dests)) = out.get_mut(&r.num) {
            dests.retain(|_, v| !is_dead_target(v, &snapshot));
        }
    }
}

fn clean_object(obj: &mut Object, snapshot: &BTreeMap<u32, Object>) {
    match obj {
        Object::Array(items) => {
            for item in items.iter_mut() {
                clean_object(item, snapshot);
            }
        }
        Object::Dict(dict) | Object::Stream { dict, .. } => clean_dict(dict, snapshot),
        _ => {}
    }
}

fn clean_dict(dict: &mut Dict, snapshot: &BTreeMap<u32, Object>) {
    let dest = Name::new("Dest");
    let action = Name::new("A");
    let dest_array = Name::new("D");
    if dict.get(&dest).is_some_and(|d| is_dead_dest(d, snapshot)) {
        dict.remove(&dest);
    }
    if dict
        .get(&action)
        .is_some_and(|a| is_dead_action(a, snapshot))
    {
        dict.remove(&action);
    }
    if dict
        .get(&dest_array)
        .is_some_and(|d| is_dead_dest(d, snapshot))
    {
        dict.remove(&dest_array);
    }
    if let Some(Object::Array(names)) = dict.get_mut(&Name::new("Names")) {
        let kept: Vec<Object> = names
            .chunks(2)
            .filter(|pair| !pair.get(1).is_some_and(|v| is_dead_target(v, snapshot)))
            .flat_map(|pair| pair.iter().cloned())
            .collect();
        *names = kept;
    }
    if let Some(Object::Dict(dests)) = dict.get_mut(&Name::new("Dests")) {
        dests.retain(|_, v| !is_dead_target(v, snapshot));
    }
    if let Some(Object::Array(annots)) = dict.get_mut(&Name::new("Annots")) {
        annots.retain(|a| !is_dead_annotation(a, snapshot));
    }
    for value in dict.values_mut() {
        clean_object(value, snapshot);
    }
}

/// The object behind `obj`, one reference deep.
fn deref<'o>(obj: &'o Object, snapshot: &'o BTreeMap<u32, Object>) -> Option<&'o Object> {
    match obj {
        Object::Reference(r) => snapshot.get(&r.num),
        other => Some(other),
    }
}

/// A destination array whose page became `null`.
fn is_dead_dest(obj: &Object, snapshot: &BTreeMap<u32, Object>) -> bool {
    matches!(deref(obj, snapshot), Some(Object::Array(items)) if matches!(items.first(), Some(Object::Null)))
}

/// A `/GoTo` action without a live destination (12.6.4.2).
fn is_dead_action(obj: &Object, snapshot: &BTreeMap<u32, Object>) -> bool {
    let Some(Object::Dict(dict)) = deref(obj, snapshot) else {
        return false;
    };
    let goto = matches!(dict.get(&Name::new("S")), Some(Object::Name(n)) if n.0 == b"GoTo");
    goto && dict
        .get(&Name::new("D"))
        .is_none_or(|d| is_dead_dest(d, snapshot))
}

/// A named destination's value: a destination array, or a dictionary
/// holding one under `/D` (12.3.2.3).
fn is_dead_target(obj: &Object, snapshot: &BTreeMap<u32, Object>) -> bool {
    if is_dead_dest(obj, snapshot) {
        return true;
    }
    match deref(obj, snapshot) {
        Some(Object::Dict(dict)) => dict
            .get(&Name::new("D"))
            .is_some_and(|d| is_dead_dest(d, snapshot)),
        Some(Object::Null) | None => true,
        _ => false,
    }
}

/// An `/Annots` entry to drop: `null`, or a link annotation whose
/// destination and action are both gone (12.5.6.5).
fn is_dead_annotation(obj: &Object, snapshot: &BTreeMap<u32, Object>) -> bool {
    let Some(annot) = deref(obj, snapshot) else {
        return true;
    };
    let Object::Dict(dict) = annot else {
        return matches!(annot, Object::Null);
    };
    let link = matches!(dict.get(&Name::new("Subtype")), Some(Object::Name(n)) if n.0 == b"Link");
    if !link {
        return false;
    }
    let dest_alive = dict
        .get(&Name::new("Dest"))
        .is_some_and(|d| !is_dead_dest(d, snapshot));
    let action_alive = dict
        .get(&Name::new("A"))
        .is_some_and(|a| !is_dead_action(a, snapshot));
    !dest_alive && !action_alive
}

/// Keep only the objects reachable from the trailer.
fn sweep(out: &mut BTreeMap<u32, Object>, trailer: &Dict) {
    let mut reachable = BTreeSet::new();
    let mut stack: Vec<u32> = Vec::new();
    for value in trailer.values() {
        collect_refs(value, &mut stack);
    }
    while let Some(num) = stack.pop() {
        if !reachable.insert(num) {
            continue;
        }
        if let Some(obj) = out.get(&num) {
            collect_refs(obj, &mut stack);
        }
    }
    out.retain(|num, _| reachable.contains(num));
}

fn collect_refs(obj: &Object, into: &mut Vec<u32>) {
    match obj {
        Object::Reference(r) => into.push(r.num),
        Object::Array(items) => items.iter().for_each(|i| collect_refs(i, into)),
        Object::Dict(dict) | Object::Stream { dict, .. } => {
            dict.values().for_each(|v| collect_refs(v, into));
        }
        _ => {}
    }
}

fn structure(message: impl Into<String>) -> Error {
    Error::BadStructure {
        message: message.into(),
    }
}

fn operation(message: impl Into<String>) -> Error {
    Error::BadOperation {
        message: message.into(),
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(ranges_every(0, 3), Vec::<Range<usize>>::new());
        assert_eq!(ranges_every(7, 0), Vec::<Range<usize>>::new());
        assert_eq!(ranges_every(7, 3), vec![0..3, 3..6, 6..7]);
        assert_eq!(ranges_every(6, 3), vec![0..3, 3..6]);
        assert_eq!(ranges_every(2, 5), vec![0..2]);
    }

    #[test]
    fn selections() {
        assert!(check_selection(&[0, 2, 1], 3, false).is_ok());
        assert!(check_selection(&[], 3, true).is_ok());
        assert!(matches!(
            check_selection(&[], 3, false),
            Err(Error::BadOperation { .. })
        ));
        assert_eq!(
            check_selection(&[0, 3], 3, false),
            Err(Error::NoSuchPage { index: 3, count: 3 })
        );
        assert!(matches!(
            check_selection(&[1, 1], 3, false),
            Err(Error::BadOperation { message }) if message.contains("more than once")
        ));
    }

    #[test]
    fn dead_destination_predicates() {
        let mut snapshot = BTreeMap::new();
        snapshot.insert(
            5,
            Object::Array(vec![Object::Null, Object::Name(Name::new("Fit"))]),
        );
        snapshot.insert(
            6,
            Object::Array(vec![Object::Reference(ObjRef { num: 9, gen: 0 })]),
        );
        let dead = Object::Reference(ObjRef { num: 5, gen: 0 });
        let alive = Object::Reference(ObjRef { num: 6, gen: 0 });
        assert!(is_dead_dest(&dead, &snapshot));
        assert!(!is_dead_dest(&alive, &snapshot));
        assert!(!is_dead_dest(&Object::String(b"name".to_vec()), &snapshot));
        let mut goto = Dict::new();
        goto.insert(Name::new("S"), Object::Name(Name::new("GoTo")));
        goto.insert(Name::new("D"), dead.clone());
        assert!(is_dead_action(&Object::Dict(goto.clone()), &snapshot));
        goto.insert(Name::new("D"), alive.clone());
        assert!(!is_dead_action(&Object::Dict(goto.clone()), &snapshot));
        goto.remove(&Name::new("D"));
        assert!(is_dead_action(&Object::Dict(goto.clone()), &snapshot));
        goto.insert(Name::new("S"), Object::Name(Name::new("URI")));
        assert!(!is_dead_action(&Object::Dict(goto), &snapshot));
        let mut link = Dict::new();
        link.insert(Name::new("Subtype"), Object::Name(Name::new("Link")));
        assert!(is_dead_annotation(&Object::Dict(link.clone()), &snapshot));
        link.insert(Name::new("Dest"), alive);
        assert!(!is_dead_annotation(&Object::Dict(link), &snapshot));
        let mut text = Dict::new();
        text.insert(Name::new("Subtype"), Object::Name(Name::new("Text")));
        assert!(!is_dead_annotation(&Object::Dict(text), &snapshot));
        assert!(is_dead_annotation(&Object::Null, &snapshot));
    }

    #[test]
    fn sweep_keeps_the_reachable_objects() {
        let mut out = BTreeMap::new();
        out.insert(1, Object::Reference(ObjRef { num: 2, gen: 0 }));
        out.insert(2, Object::Integer(0));
        out.insert(3, Object::Integer(0));
        let mut trailer = Dict::new();
        trailer.insert(
            Name::new("Root"),
            Object::Reference(ObjRef { num: 1, gen: 0 }),
        );
        sweep(&mut out, &trailer);
        assert_eq!(out.keys().copied().collect::<Vec<_>>(), vec![1, 2]);
    }
}
