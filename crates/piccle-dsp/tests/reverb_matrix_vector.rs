//! Guards the packaged matrix vector against drift from the pinned
//! specification.

const PACKAGED_VECTOR: &str = include_str!("../test-data/reverb-matrix-vector.json");
const SPEC_VECTOR: &str =
    include_str!("../../../piccle-spec/test-vectors/numeric/reverb-matrix-vector.json");

#[test]
fn packaged_matrix_vector_matches_the_pinned_specification() {
    assert_eq!(PACKAGED_VECTOR, SPEC_VECTOR);
}
