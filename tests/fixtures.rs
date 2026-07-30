mod common;

use std::fs;

use oxlite::Pager;

#[test]
fn simple_fixture_is_available() {
    let db_path = common::fixture_path("simple.db");
    let expected_path = common::fixture_path("simple.expected");

    let mut pager = Pager::open(&db_path).unwrap();
    let page = pager.read_page(1).unwrap();

    assert_eq!(pager.header().page_size.get(), 4096);
    assert_eq!(page.number(), 1);
    assert_eq!(page.bytes().len(), 4096);
    assert_eq!(
        fs::read_to_string(expected_path).unwrap(),
        "1|10|alpha\n2|20|beta\n"
    );
}
