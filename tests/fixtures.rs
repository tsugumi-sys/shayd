mod common;

use std::fs;

use oxlite::{BtreePage, PageType, Pager, Value};

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

#[test]
fn reads_table_leaf_cells_from_simple_fixture() {
    let db_path = common::fixture_path("simple.db");
    let mut pager = Pager::open(&db_path).unwrap();
    let page = pager.read_page(2).unwrap();
    let btree_page = BtreePage::parse(&page).unwrap();
    let cells = btree_page.table_leaf_cells(&page).unwrap();

    assert_eq!(btree_page.header().page_type, PageType::TableLeaf);
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0].rowid, 1);
    assert_eq!(
        cells[0].record.values(),
        &[Value::Integer(10), Value::Text("alpha".to_owned())]
    );
    assert_eq!(cells[1].rowid, 2);
    assert_eq!(
        cells[1].record.values(),
        &[Value::Integer(20), Value::Text("beta".to_owned())]
    );
}
