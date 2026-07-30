mod common;

use std::fs;

use oxlite::{BtreePage, Database, PageType, Pager, Schema, SchemaObjectType, Value, scan_table};

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
    assert_eq!(
        schema
            .table_schema("t")
            .unwrap()
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
}

#[test]
fn scans_rows_from_simple_fixture_table() {
    let db_path = common::fixture_path("simple.db");
    let expected_path = common::fixture_path("simple.expected");
    let mut pager = Pager::open(&db_path).unwrap();
    let rows = scan_table(&mut pager, 2).unwrap();

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

#[test]
fn database_api_scans_simple_fixture_table() {
    let db_path = common::fixture_path("simple.db");
    let expected_path = common::fixture_path("simple.expected");
    let mut database = Database::open(&db_path).unwrap();

    assert_eq!(database.schema().table("t").unwrap().root_page, Some(2));

    let rows = database.scan_table("t").unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows_to_sqlite_output(&rows),
        fs::read_to_string(expected_path).unwrap()
    );
}

#[test]
fn scans_rows_from_multipage_fixture_table() {
    let db_path = common::fixture_path("multipage.db");
    let expected_path = common::fixture_path("multipage.expected");
    let mut database = Database::open(&db_path).unwrap();

    assert_eq!(database.schema().table("big").unwrap().root_page, Some(2));

    let mut pager = Pager::open(&db_path).unwrap();
    let root_page = pager.read_page(2).unwrap();
    let root_btree_page = BtreePage::parse(&root_page).unwrap();

    assert_eq!(root_btree_page.header().page_type, PageType::TableInterior);

    let rows = database.scan_table("big").unwrap();

    assert_eq!(rows.len(), 120);
    assert_eq!(rows[0].rowid, 1);
    assert_eq!(
        rows[0].values,
        vec![
            Value::Integer(1),
            Value::Text("row-001-abcdefghijklmnopqrstuvwxyz".to_owned())
        ]
    );
    assert_eq!(rows[119].rowid, 120);
    assert_eq!(
        rows[119].values,
        vec![
            Value::Integer(120),
            Value::Text("row-120-abcdefghijklmnopqrstuvwxyz".to_owned())
        ]
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
