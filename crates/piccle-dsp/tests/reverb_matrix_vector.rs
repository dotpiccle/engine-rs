//! Guards the packaged matrix vector against drift from the pinned
//! specification.

const PACKAGED_VECTOR: &str = include_str!("../test-data/reverb-matrix-vector.json");
const SPEC_VECTOR: &str =
    include_str!("../../../piccle-spec/test-vectors/numeric/reverb-matrix-vector.json");

#[test]
fn packaged_matrix_vector_matches_the_pinned_specification() {
    let packaged: serde_json::Value =
        serde_json::from_str(PACKAGED_VECTOR).expect("packaged matrix vector must be valid JSON");
    let specification: serde_json::Value =
        serde_json::from_str(SPEC_VECTOR).expect("specification matrix vector must be valid JSON");

    assert_eq!(packaged, specification);
}
