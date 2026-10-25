//! The kind of justification in `w:jc` (ECMA-376 Part 1 §17.18.44): kashida justification for
//! Arabic (`lowKashida`, `mediumKashida`, `highKashida`) and Thai distribution
//! (`thaiDistribute`) read as justified/distributed and are written back as they were, not as
//! `both`/`distribute`. Fixtures are hand-written from the schema.

use std::io::{Read, Write};

use wordcraft_doc::props::{Align, JustifyStyle, ParaProps};
use wordcraft_doc::{Document, Paragraph, para_block};

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zw.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        zw.write_all(bytes).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

fn docx(body: &str) -> Vec<u8> {
    let root = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
    );
    zip(&[("_rels/.rels", root.as_bytes()), ("word/document.xml", doc.as_bytes())])
}

fn document_xml(bytes: &[u8]) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name("word/document.xml").unwrap().read_to_string(&mut s).unwrap();
    s
}

fn jc_para(val: &str) -> String {
    format!(r#"<w:p><w:pPr><w:bidi/><w:jc w:val="{val}"/></w:pPr><w:r><w:rPr><w:rtl/></w:rPr><w:t>نص عربي مضبوط</w:t></w:r></w:p>"#)
}

const VALUES: [(&str, Align, Option<JustifyStyle>); 6] = [
    ("both", Align::Justify, None),
    ("lowKashida", Align::Justify, Some(JustifyStyle::LowKashida)),
    ("mediumKashida", Align::Justify, Some(JustifyStyle::MediumKashida)),
    ("highKashida", Align::Justify, Some(JustifyStyle::HighKashida)),
    ("distribute", Align::Distribute, None),
    ("thaiDistribute", Align::Distribute, Some(JustifyStyle::Thai)),
];

#[test]
fn justification_kinds_are_read() {
    let body: String = VALUES.iter().map(|(v, ..)| jc_para(v)).collect();
    let d = wordcraft_docx::read(&docx(&body)).unwrap();
    let ps: Vec<&Paragraph> = d.body.iter().filter_map(|b| b.as_para()).collect();
    assert_eq!(ps.len(), VALUES.len());
    for (p, (val, align, style)) in ps.iter().zip(VALUES) {
        assert_eq!(p.props.align, Some(align), "{val}");
        assert_eq!(p.props.justify_style, style, "{val}");
    }
}

#[test]
fn justification_kinds_survive_open_and_save() {
    let body: String = VALUES.iter().map(|(v, ..)| jc_para(v)).collect();
    let d = wordcraft_docx::read(&docx(&body)).unwrap();
    let xml = document_xml(&wordcraft_docx::write(&d).unwrap());
    let written: Vec<&str> = xml.match_indices(r#"<w:jc w:val=""#).map(|(i, m)| xml[i + m.len()..].split('"').next().unwrap_or("")).collect();
    let expected: Vec<&str> = VALUES.iter().map(|(v, ..)| *v).collect();
    assert_eq!(written, expected, "{xml}");
}

#[test]
fn other_alignments_name_no_kind() {
    let body: String = ["left", "center", "right", "start", "end"].iter().map(|v| jc_para(v)).collect();
    let d = wordcraft_docx::read(&docx(&body)).unwrap();
    for p in d.body.iter().filter_map(|b| b.as_para()) {
        assert_eq!(p.props.justify_style, None, "{:?}", p.props.align);
    }
}

#[test]
fn a_kind_that_does_not_fit_the_alignment_is_not_written() {
    // A centred paragraph that still carries a kashida kind (inherited, or left behind) is written
    // as centred; Thai distribution only applies to a distributed paragraph and kashida only to a
    // justified one.
    let cases = [
        (Align::Center, JustifyStyle::LowKashida, "center"),
        (Align::Distribute, JustifyStyle::HighKashida, "distribute"),
        (Align::Justify, JustifyStyle::Thai, "both"),
    ];
    for (align, style, expected) in cases {
        let mut p = Paragraph::with_text("x", Default::default());
        p.props = ParaProps { align: Some(align), justify_style: Some(style), ..Default::default() };
        let mut d = Document::new();
        d.body = vec![para_block(p)];
        let xml = document_xml(&wordcraft_docx::write(&d).unwrap());
        assert!(xml.contains(&format!(r#"<w:jc w:val="{expected}"/>"#)), "{align:?} + {style:?}: {xml}");
    }
}

#[test]
fn an_explicit_both_overrides_an_inherited_kashida() {
    // Style (or earlier) formatting with kashida, then the paragraph's own `both`: alignment and
    // kind are one `w:jc` value, so the paragraph's plain justification wins.
    let style = ParaProps { align: Some(Align::Justify), justify_style: Some(JustifyStyle::HighKashida), ..Default::default() };
    let mut resolved = style.clone();
    resolved.overlay(&ParaProps { align: Some(Align::Justify), ..Default::default() });
    assert_eq!((resolved.align, resolved.justify_style), (Some(Align::Justify), None));
    // A paragraph that says nothing about alignment keeps the style's kashida.
    let mut resolved = style.clone();
    resolved.overlay(&ParaProps { indent_left: Some(10.0), ..Default::default() });
    assert_eq!(resolved.justify_style, Some(JustifyStyle::HighKashida));
}
