use std::{
    fs::File,
    io::{self, BufReader},
    path::Path,
};

use openpaintr::Document;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pdn")
        .join(name)
}

#[test]
fn old_pdn_3510_matches_reference_png_pixel_for_pixel() -> io::Result<()> {
    let pdn = Document::read_pdn(File::open(fixture("oldPDN3510.pdn"))?)?;
    let reference = Document::read_png(BufReader::new(File::open(fixture(
        "oldPDN3510-reference.png",
    ))?))?;
    assert_eq!(pdn.dimensions(), reference.dimensions());
    let actual = pdn.composite()?;
    let expected = reference.composite()?;
    let diffs = actual
        .iter()
        .zip(&expected)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(diffs, 0, "divergência de composição em oldPDN3510.pdn");
    Ok(())
}

#[test]
fn flatten_blend_and_versioned_pdn_fixtures_round_trip_cleanly() -> io::Result<()> {
    for (name, expected_layers) in [
        ("Untitled.pdn", 1usize),
        ("Untitled3.pdn", 2),
        ("oldPDN3510.pdn", 2),
        ("FlattenBlendTest.pdn", 14),
    ] {
        let doc = Document::read_pdn(File::open(fixture(name))?)?;
        assert_eq!(doc.layer_count(), expected_layers, "{name}");
        let orig_comp = doc.composite()?;

        let mut encoded = Vec::new();
        doc.write_pdn(&mut encoded)?;
        let reopened = Document::read_pdn(encoded.as_slice())?;
        assert_eq!(reopened.dimensions(), doc.dimensions(), "{name}");
        assert_eq!(reopened.layer_count(), doc.layer_count(), "{name}");
        for idx in 0..doc.layer_count() {
            assert_eq!(reopened.layer_name(idx), doc.layer_name(idx), "{name}#{idx}");
            assert_eq!(
                reopened.layer_properties(idx),
                doc.layer_properties(idx),
                "{name}#{idx}"
            );
            assert_eq!(
                reopened.layer_blend_mode(idx),
                doc.layer_blend_mode(idx),
                "{name}#{idx}"
            );
        }
        assert_eq!(reopened.composite()?, orig_comp, "{name}");
    }
    Ok(())
}
