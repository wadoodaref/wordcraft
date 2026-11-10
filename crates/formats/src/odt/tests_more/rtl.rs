//! Right-to-left paragraphs (ODF 1.3 §20.404 `style:writing-mode` on paragraph properties: `rl-tb`
//! and `rl`), read as WordCraft's bidirectional paragraphs and written back.

use crate::odt::tests::{NS_T, package};
use crate::odt::{export, import};
use wordcraft_doc::Document;

fn open(mode: &str) -> Document {
    let content = format!(
        r#"<office:document-content {NS_T}><office:automatic-styles><style:style style:name="P1" style:family="paragraph"><style:paragraph-properties style:writing-mode="{mode}"/></style:style></office:automatic-styles><office:body><office:text><text:p text:style-name="P1">שלום עולם</text:p><text:p>Left to right</text:p></office:text></office:body></office:document-content>"#
    );
    import(&package(&content, None)).unwrap()
}

fn bidi(d: &Document) -> Vec<Option<bool>> {
    d.body.iter().filter_map(|b| b.as_para()).map(|p| p.props.bidi).collect()
}

#[test]
fn right_to_left_paragraphs_are_read() {
    assert_eq!(bidi(&open("rl-tb")), [Some(true), None]);
    assert_eq!(bidi(&open("rl")), [Some(true), None]);
    assert_eq!(bidi(&open("lr-tb")), [None, None]);
}

#[test]
fn right_to_left_paragraphs_round_trip() {
    let d = open("rl-tb");
    assert_eq!(bidi(&import(&export(&d).unwrap()).unwrap()), [Some(true), None]);
}

#[test]
fn right_to_left_table_is_read() {
    let content = format!(
        r#"<office:document-content {NS_T}><office:automatic-styles><style:style style:name="T1" style:family="table"><style:table-properties style:writing-mode="rl-tb"/></style:style></office:automatic-styles><office:body><office:text><table:table table:name="Table1" table:style-name="T1"><table:table-column/><table:table-row><table:table-cell><text:p>cell</text:p></table:table-cell></table:table-row></table:table></office:text></office:body></office:document-content>"#
    );
    let doc = import(&package(&content, None)).unwrap();
    let table = doc.body[0].as_table().expect("expected table");
    assert!(table.props.bidi_visual);
}

#[test]
fn right_to_left_paragraph_populates_font_cs() {
    let content = format!(
        r#"<office:document-content {NS_T}><office:automatic-styles><style:style style:name="P1" style:family="paragraph"><style:paragraph-properties style:writing-mode="rl-tb"/><style:text-properties fo:font-family="Arial"/></style:style></office:automatic-styles><office:body><office:text><text:p text:style-name="P1">مرحبا</text:p></office:text></office:body></office:document-content>"#
    );
    let doc = import(&package(&content, None)).unwrap();
    let p = doc.body[0].as_para().expect("expected paragraph");
    assert_eq!(p.props.bidi, Some(true));
    assert_eq!(p.mark.font_cs.as_deref(), Some("Arial"));
    assert_eq!(p.runs[0].props.font_cs.as_deref(), Some("Arial"));
}
