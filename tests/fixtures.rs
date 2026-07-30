mod common;

use std::fs;

use oxlite::{BtreePage, PageType, Pager, Schema, SchemaObjectType, Value, scan_table_page};

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

#[test]
fn loads_schema_from_simple_fixture() {
    let db_path = common::fixture_path("simple.db");
    let mut pager = Pager::open(&db_path).unwrap();
    let schema = Schema::load(&mut pager).unwrap();
    let table = schema.table("t").unwrap();

    assert_eq!(table.object_type, SchemaObjectType::Table);
    assert_eq!(table.name, "t");
    assert_eq!(table.table_name, "t");
    assert_eq!(table.root_page, Some(2));
    assert_eq!(
        table.sql.as_deref(),
        Some("CREATE TABLE t (\n  a INTEGER,\n  b TEXT\n)")
    );
}

#[test]
fn scans_rows_from_simple_fixture_table() {
    let db_path = common::fixture_path("simple.db");
    let expected_path = common::fixture_path("simple.expected");
    let mut pager = Pager::open(&db_path).unwrap();
    let rows = scan_table_page(&mut pager, 2).unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].rowid, 1);
    assert_eq!(
        rows[0].values,
        vec![Value::Integer(10), Value::Text("alpha".to_owned())]
    );
    assert_eq!(rows[1].rowid, 2);
    assert_eq!(
        rows[1].values,
        vec![Value::Integer(20), Value::Text("beta".to_owned())]
    );
    assert_eq!(
        rows_to_sqlite_output(&rows),
        fs::read_to_string(expected_path).unwrap()
    );
}

fn rows_to_sqlite_output(rows: &[oxlite::Row]) -> String {
    let mut output = String::new();
    for row in rows {
        output.push_str(&row.rowid.to_string());
        for value in &row.values {
            output.push('|');
            output.push_str(&value_to_sqlite_output(value));
        }
        output.push('\n');
    }
    output
}

fn value_to_sqlite_output(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Integer(value) => value.to_string(),
        Value::Real(value) => value.to_string(),
        Value::Text(value) => value.clone(),
        Value::Blob(bytes) => bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>(),
    }
}
