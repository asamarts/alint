use example_project::hello;

#[test]
fn greets() {
    assert!(!hello().is_empty());
}
