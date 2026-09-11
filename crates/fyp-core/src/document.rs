//! Document access: opens a file through its cross-reference table and reads
//! objects on demand (ISO 32000-2, 7.5 and 7.7).
//!
//! Only classic xref tables are read so far. A table that points to the
//! wrong place is an error: rebuilding it by scanning the file is milestone
//! 0.1, step 3.

use crate::object::{Dict, Name, ObjRef, Object};
use crate::parser::Parser;
use crate::version::{self, PdfVersion};
use crate::xref::{Xref, XrefEntry};
use crate::{Error, Result};

/// A PDF file opened through its cross-reference table. Objects are parsed
/// lazily from the borrowed input.
#[derive(Debug, Clone)]
pub struct Document<'a> {
    input: &'a [u8],
    version: PdfVersion,
    xref: Xref,
}

impl<'a> Document<'a> {
    /// Check the header, then load the cross-reference chain announced by
    /// the last `startxref`.
    pub fn open(input: &'a [u8]) -> Result<Document<'a>> {
        let info = version::quick_info(input)?;
        let startxref = info.startxref.ok_or(Error::MissingStartxref)?;
        let xref = Xref::parse(input, startxref)?;
        Ok(Document {
            input,
            version: info.version,
            xref,
        })
    }

    /// Version declared in the header.
    pub fn version(&self) -> PdfVersion {
        self.version
    }

    /// Trailer of the newest cross-reference section (ISO 32000-2, 7.5.5).
    pub fn trailer(&self) -> &Dict {
        self.xref.trailer()
    }

    /// Cross-reference table merged across incremental updates.
    pub fn xref(&self) -> &Xref {
        &self.xref
    }

    /// Read object `r` at the offset given by the table.
    ///
    /// `Ok(None)` when the table does not list `r` as in use with that
    /// generation: such a reference denotes the null object
    /// (ISO 32000-2, 7.3.10). If the object found at the offset is not `r`,
    /// the table is wrong and this is an [`Error::Syntax`].
    pub fn get(&self, r: ObjRef) -> Result<Option<Object>> {
        let offset = match self.xref.get(r.num) {
            Some(XrefEntry::InUse { offset, gen }) if gen == r.gen => offset,
            _ => return Ok(None),
        };
        if offset >= self.input.len() {
            return Err(Error::BadXref {
                offset,
                message: format!("object {} {} lies beyond end of file", r.num, r.gen),
            });
        }
        let (found, obj) = Parser::at(self.input, offset).parse_indirect()?;
        if found != r {
            return Err(Error::Syntax {
                offset,
                message: format!(
                    "xref points to object {} {}, found {} {}",
                    r.num, r.gen, found.num, found.gen
                ),
            });
        }
        Ok(Some(obj))
    }

    /// Follow `obj` one level if it is a reference; a reference to a missing
    /// object gives `null`. Any other object is returned unchanged.
    pub fn resolve(&self, obj: &Object) -> Result<Object> {
        match obj {
            Object::Reference(r) => Ok(self.get(*r)?.unwrap_or(Object::Null)),
            other => Ok(other.clone()),
        }
    }

    /// Document catalog, the trailer's `/Root` (ISO 32000-2, 7.7.2).
    pub fn catalog(&self) -> Result<Dict> {
        let root = self
            .trailer()
            .get(&Name::new("Root"))
            .ok_or_else(|| structure("trailer has no /Root"))?;
        self.resolve_dict(root, "/Root")
    }

    /// Number of pages: `/Count` of the root of the page tree
    /// (ISO 32000-2, 7.7.3.2).
    pub fn page_count(&self) -> Result<usize> {
        let catalog = self.catalog()?;
        let pages = catalog
            .get(&Name::new("Pages"))
            .ok_or_else(|| structure("catalog has no /Pages"))?;
        let pages = self.resolve_dict(pages, "/Pages")?;
        let count = pages
            .get(&Name::new("Count"))
            .ok_or_else(|| structure("page tree root has no /Count"))?;
        self.resolve(count)?
            .as_i64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| structure("page tree /Count is not a non-negative integer"))
    }

    fn resolve_dict(&self, obj: &Object, what: &str) -> Result<Dict> {
        match self.resolve(obj)? {
            Object::Dict(dict) => Ok(dict),
            _ => Err(structure(format!("{what} is not a dictionary"))),
        }
    }
}

fn structure(message: impl Into<String>) -> Error {
    Error::BadStructure {
        message: message.into(),
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const CATALOG: &str = "<< /Type /Catalog /Pages 2 0 R >>";

    /// One-section PDF where `objects[i]` becomes object `i + 1`. `patch`
    /// may alter the offsets written to the xref to simulate broken tables.
    fn build(objects: &[&str], patch: impl FnOnce(&mut [usize])) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
        }
        patch(offsets.as_mut_slice());
        let startxref = out.len();
        let size = offsets.len() + 1;
        out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
        for offset in &offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{startxref}\n%%EOF\n")
                .as_bytes(),
        );
        out
    }

    #[test]
    fn indirect_count_is_resolved() {
        let file = build(
            &[CATALOG, "<< /Type /Pages /Kids [] /Count 3 0 R >>", "2"],
            |_| {},
        );
        let doc = Document::open(&file).expect("open");
        assert_eq!(doc.page_count(), Ok(2));
    }

    #[test]
    fn object_header_must_match_reference() {
        let file = build(&[CATALOG, "<< /Type /Pages /Kids [] /Count 0 >>"], |o| {
            o.swap(0, 1)
        });
        let doc = Document::open(&file).expect("open");
        assert!(matches!(
            doc.get(ObjRef { num: 1, gen: 0 }),
            Err(Error::Syntax { .. })
        ));
        assert!(matches!(doc.page_count(), Err(Error::Syntax { .. })));
    }

    #[test]
    fn object_offset_beyond_end_of_file() {
        let file = build(&[CATALOG, "<< /Type /Pages /Kids [] /Count 0 >>"], |o| {
            o[1] = 999_999_999
        });
        let doc = Document::open(&file).expect("open");
        assert!(matches!(
            doc.get(ObjRef { num: 2, gen: 0 }),
            Err(Error::BadXref {
                offset: 999_999_999,
                ..
            })
        ));
    }

    #[test]
    fn unlisted_free_or_other_generation_is_null() {
        let file = build(&[CATALOG], |_| {});
        let doc = Document::open(&file).expect("open");
        assert_eq!(doc.get(ObjRef { num: 1, gen: 1 }), Ok(None));
        assert_eq!(doc.get(ObjRef { num: 0, gen: 65535 }), Ok(None));
        assert_eq!(doc.get(ObjRef { num: 9, gen: 0 }), Ok(None));
        assert_eq!(
            doc.resolve(&Object::Reference(ObjRef { num: 9, gen: 0 })),
            Ok(Object::Null)
        );
        assert_eq!(doc.resolve(&Object::Integer(7)), Ok(Object::Integer(7)));
    }

    #[test]
    fn broken_page_tree_is_an_error() {
        let negative = build(&[CATALOG, "<< /Type /Pages /Kids [] /Count -1 >>"], |_| {});
        let no_pages = build(&["<< /Type /Catalog >>"], |_| {});
        let root_not_dict = build(&["[1 2 3]"], |_| {});
        for file in [negative, no_pages, root_not_dict] {
            let doc = Document::open(&file).expect("open");
            let count = doc.page_count();
            assert!(
                matches!(count, Err(Error::BadStructure { .. })),
                "{count:?}"
            );
        }
        let no_root = b"%PDF-1.7\nxref\n0 1\n0000000000 65535 f \ntrailer\n<< /Size 1 >>\nstartxref\n9\n%%EOF\n";
        let doc = Document::open(no_root).expect("open");
        assert!(matches!(doc.catalog(), Err(Error::BadStructure { .. })));
    }

    #[test]
    fn missing_startxref() {
        let file = b"%PDF-1.7\n1 0 obj << >> endobj\n";
        assert_eq!(
            Document::open(file).map(|_| ()),
            Err(Error::MissingStartxref)
        );
    }
}
