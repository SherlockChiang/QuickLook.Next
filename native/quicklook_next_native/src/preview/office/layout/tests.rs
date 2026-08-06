use super::{parse_relationships, part_base_dir, rels_path_for_part};

use super::super::super::OfficeContext;

#[test]
fn office_relationships_parse_ids_and_targets() {
    let context = OfficeContext::new(None);
    let relationships = parse_relationships(
        &context,
        r#"<Relationships xmlns="r"><Relationship Id="rId1" Target="../media/image1.png"/><Relationship Id="rId2" Target="drawing1.xml"/></Relationships>"#,
    )
    .expect("relationships");

    assert_eq!(
        relationships.get("rId1").map(String::as_str),
        Some("../media/image1.png")
    );
    assert_eq!(
        relationships.get("rId2").map(String::as_str),
        Some("drawing1.xml")
    );
}

#[test]
fn office_part_paths_follow_ooxml_relationship_layout() {
    assert_eq!(
        rels_path_for_part("ppt/slideLayouts/slideLayout1.xml"),
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels"
    );
    assert_eq!(part_base_dir("xl/drawings/drawing1.xml"), "xl/drawings/");
    assert_eq!(part_base_dir("[Content_Types].xml"), "");
}
