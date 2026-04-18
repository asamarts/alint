use example_project::mod1::helpers::clamp;

#[test]
fn clamps() {
    assert_eq!(clamp(50, 0, 10), 10);
}
