mod common;

use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};

use oxlite::{
    BtreePage, Database, DatabaseHeader, Error, IndexSchema, PageType, Pager, QueryResultRow,
    Schema, SchemaObjectType, Value, lookup_rowid, scan_table,
};

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
fn opens_simple_fixture_from_memory() {
    let db_path = common::fixture_path("simple.db");
    let bytes = fs::read(db_path).unwrap();
    let mut pager = Pager::from_bytes(bytes).unwrap();
    let page = pager.read_page(1).unwrap();

    assert_eq!(pager.header().page_size.get(), 4096);
    assert_eq!(pager.path().to_string_lossy(), "<memory>");
    assert_eq!(page.number(), 1);
    assert_eq!(page.bytes().len(), 4096);
}

#[test]
fn execute_sql_reads_memory_backed_database() {
    let db_path = common::fixture_path("simple.db");
    let expected_path = common::fixture_path("simple.expected");
    let bytes = fs::read(db_path).unwrap();
    let mut database = Database::open_bytes(bytes).unwrap();
    let rows = database.execute_sql("SELECT rowid, a, b FROM t").unwrap();

    assert_eq!(
        query_rows_to_sqlite_output(&rows),
        fs::read_to_string(expected_path).unwrap()
    );
}

#[test]
fn read_transaction_executes_simple_select() {
    let db_path = common::fixture_path("simple.db");
    let expected_path = common::fixture_path("simple.expected");
    let mut database = Database::open(&db_path).unwrap();
    let mut transaction = database.read_transaction().unwrap();
    let rows = transaction
        .execute_sql("SELECT rowid, a, b FROM t")
        .unwrap();

    assert_eq!(
        query_rows_to_sqlite_output(&rows),
        fs::read_to_string(expected_path).unwrap()
    );
}

#[test]
fn rejects_truncated_memory_backed_database() {
    let db_path = common::fixture_path("simple.db");
    let mut bytes = fs::read(db_path).unwrap();
    bytes.truncate(DatabaseHeader::SIZE - 1);

    assert!(matches!(
        Pager::from_bytes(bytes),
        Err(Error::Truncated {
            context: "database image",
            ..
        })
    ));
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
fn loads_index_metadata_from_indexed_fixture() {
    let db_path = common::fixture_path("indexed.db");
    let mut pager = Pager::open(&db_path).unwrap();
    let schema = Schema::load(&mut pager).unwrap();
    let indexes = schema.indexes_for_table("items");

    assert_eq!(indexes.len(), 2);
    assert_eq!(
        schema.index_for_table_column("items", "a"),
        Some(&IndexSchema {
            name: "idx_items_a".to_owned(),
            table_name: "items".to_owned(),
            root_page: Some(3),
            columns: vec!["a".to_owned()],
            unique: false,
        })
    );
    assert_eq!(
        schema.index_for_table_column("items", "b"),
        Some(&IndexSchema {
            name: "idx_items_b".to_owned(),
            table_name: "items".to_owned(),
            root_page: Some(4),
            columns: vec!["b".to_owned()],
            unique: true,
        })
    );
    assert!(schema.index_for_table_column("items", "missing").is_none());
}

#[test]
fn reads_index_leaf_cells_from_indexed_fixture() {
    let db_path = common::fixture_path("indexed.db");
    let mut pager = Pager::open(&db_path).unwrap();
    let schema = Schema::load(&mut pager).unwrap();
    let index = schema.index_for_table_column("items", "a").unwrap();
    let page = pager.read_page(index.root_page.unwrap()).unwrap();
    let btree_page = BtreePage::parse(&page).unwrap();
    let cells = btree_page
        .index_leaf_cells(&page, pager.header().usable_space() as usize)
        .unwrap();

    assert_eq!(btree_page.header().page_type, PageType::IndexLeaf);
    assert_eq!(cells.len(), 3);
    assert_eq!(
        cells
            .iter()
            .map(|cell| cell.record.values().to_vec())
            .collect::<Vec<_>>(),
        vec![
            vec![Value::Integer(10), Value::Integer(1)],
            vec![Value::Integer(20), Value::Integer(2)],
            vec![Value::Integer(30), Value::Integer(3)],
        ]
    );
}

#[test]
fn finds_rowids_by_indexed_integer_value_from_fixture() {
    let db_path = common::fixture_path("indexed.db");
    let mut pager = Pager::open(&db_path).unwrap();
    let schema = Schema::load(&mut pager).unwrap();
    let index = schema.index_for_table_column("items", "a").unwrap();
    let page = pager.read_page(index.root_page.unwrap()).unwrap();
    let btree_page = BtreePage::parse(&page).unwrap();
    let rowids = btree_page
        .index_leaf_rowids_for_value(
            &page,
            pager.header().usable_space() as usize,
            &Value::Integer(20),
        )
        .unwrap();

    assert_eq!(rowids, vec![2]);
}

#[test]
fn finds_rowids_by_indexed_text_value_from_fixture() {
    let db_path = common::fixture_path("indexed.db");
    let mut pager = Pager::open(&db_path).unwrap();
    let schema = Schema::load(&mut pager).unwrap();
    let index = schema.index_for_table_column("items", "b").unwrap();
    let page = pager.read_page(index.root_page.unwrap()).unwrap();
    let btree_page = BtreePage::parse(&page).unwrap();
    let rowids = btree_page
        .index_leaf_rowids_for_value(
            &page,
            pager.header().usable_space() as usize,
            &Value::Text("beta".to_owned()),
        )
        .unwrap();

    assert_eq!(rowids, vec![2]);
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
fn database_api_scans_named_rows_from_simple_fixture_table() {
    let db_path = common::fixture_path("simple.db");
    let mut database = Database::open(&db_path).unwrap();
    let rows = database.scan_table_named("t").unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].rowid(), 1);
    assert_eq!(rows[0].get("a"), Some(&Value::Integer(10)));
    assert_eq!(rows[0].get("b"), Some(&Value::Text("alpha".to_owned())));
    assert_eq!(rows[0].get("missing"), None);
    assert_eq!(rows[1].rowid(), 2);
    assert_eq!(rows[1].get("a"), Some(&Value::Integer(20)));
    assert_eq!(rows[1].get("b"), Some(&Value::Text("beta".to_owned())));
}

#[test]
fn query_api_projects_simple_fixture_rows() {
    let db_path = common::fixture_path("simple.db");
    let mut database = Database::open(&db_path).unwrap();
    let query = database.query_table("t").select(["rowid", "a", "b"]);
    let rows = database.execute_table_query(query).unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get("rowid"), Some(&Value::Integer(1)));
    assert_eq!(rows[0].get("a"), Some(&Value::Integer(10)));
    assert_eq!(rows[0].get("b"), Some(&Value::Text("alpha".to_owned())));
    assert_eq!(rows[1].get("rowid"), Some(&Value::Integer(2)));
    assert_eq!(rows[1].get("a"), Some(&Value::Integer(20)));
    assert_eq!(rows[1].get("b"), Some(&Value::Text("beta".to_owned())));
}

#[test]
fn query_api_projects_selected_column() {
    let db_path = common::fixture_path("simple.db");
    let mut database = Database::open(&db_path).unwrap();
    let query = database.query_table("t").select(["b"]);
    let rows = database.execute_table_query(query).unwrap();

    assert_eq!(
        rows[0].values(),
        &[("b".to_owned(), Value::Text("alpha".to_owned()))]
    );
    assert_eq!(
        rows[1].values(),
        &[("b".to_owned(), Value::Text("beta".to_owned()))]
    );
}

#[test]
fn query_api_filters_by_rowid() {
    let db_path = common::fixture_path("simple.db");
    let mut database = Database::open(&db_path).unwrap();
    let query = database
        .query_table("t")
        .select(["rowid", "a", "b"])
        .rowid_eq(2);
    let rows = database.execute_table_query(query).unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("rowid"), Some(&Value::Integer(2)));
    assert_eq!(rows[0].get("a"), Some(&Value::Integer(20)));
    assert_eq!(rows[0].get("b"), Some(&Value::Text("beta".to_owned())));
}

#[test]
fn lookup_rowid_reads_expected_simple_fixture_row() {
    let db_path = common::fixture_path("simple.db");
    let mut pager = Pager::open(&db_path).unwrap();
    let row = lookup_rowid(&mut pager, 2, 2).unwrap().unwrap();

    assert_eq!(row.rowid, 2);
    assert_eq!(
        row.values,
        vec![Value::Integer(20), Value::Text("beta".to_owned())]
    );
}

#[test]
fn lookup_rowid_returns_none_for_missing_row() {
    let db_path = common::fixture_path("simple.db");
    let mut pager = Pager::open(&db_path).unwrap();

    assert_eq!(lookup_rowid(&mut pager, 2, 999).unwrap(), None);
}

#[test]
fn query_api_rejects_unknown_columns() {
    let db_path = common::fixture_path("simple.db");
    let mut database = Database::open(&db_path).unwrap();
    let query = database.query_table("t").select(["missing"]);

    assert!(database.execute_table_query(query).is_err());
}

#[test]
fn execute_sql_projects_all_columns() {
    let db_path = common::fixture_path("simple.db");
    let mut database = Database::open(&db_path).unwrap();
    let rows = database.execute_sql("SELECT * FROM t").unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get("a"), Some(&Value::Integer(10)));
    assert_eq!(rows[0].get("b"), Some(&Value::Text("alpha".to_owned())));
    assert_eq!(rows[1].get("a"), Some(&Value::Integer(20)));
    assert_eq!(rows[1].get("b"), Some(&Value::Text("beta".to_owned())));
}

#[test]
fn execute_sql_projects_selected_columns() {
    let db_path = common::fixture_path("simple.db");
    let mut database = Database::open(&db_path).unwrap();
    let rows = database.execute_sql("SELECT a, b FROM t").unwrap();

    assert_eq!(
        rows[0].values(),
        &[
            ("a".to_owned(), Value::Integer(10)),
            ("b".to_owned(), Value::Text("alpha".to_owned()))
        ]
    );
}

#[test]
fn execute_sql_filters_by_rowid() {
    let db_path = common::fixture_path("simple.db");
    let mut database = Database::open(&db_path).unwrap();
    let rows = database
        .execute_sql("SELECT rowid, a FROM t WHERE rowid = 2")
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].values(),
        &[
            ("rowid".to_owned(), Value::Integer(2)),
            ("a".to_owned(), Value::Integer(20))
        ]
    );
}

#[test]
fn execute_sql_preserves_table_and_column_errors() {
    let db_path = common::fixture_path("simple.db");
    let mut database = Database::open(&db_path).unwrap();

    assert!(matches!(
        database.execute_sql("SELECT a FROM missing"),
        Err(Error::InvalidSchema("table schema not found"))
    ));
    assert!(matches!(
        database.execute_sql("SELECT missing FROM t"),
        Err(Error::InvalidSchema("unknown projection column"))
    ));
}

#[test]
fn execute_sql_rejects_unsupported_sql() {
    let db_path = common::fixture_path("simple.db");
    let mut database = Database::open(&db_path).unwrap();

    assert!(matches!(
        database.execute_sql("DELETE FROM t"),
        Err(Error::InvalidSql("expected keyword"))
    ));
}

#[test]
fn execute_sql_matches_simple_fixture_expected() {
    let db_path = common::fixture_path("simple.db");
    let expected_path = common::fixture_path("simple.expected");
    let mut database = Database::open(&db_path).unwrap();
    let rows = database.execute_sql("SELECT rowid, a, b FROM t").unwrap();

    assert_eq!(
        query_rows_to_sqlite_output(&rows),
        fs::read_to_string(expected_path).unwrap()
    );
}

#[test]
fn execute_sql_matches_selected_column_output() {
    let db_path = common::fixture_path("simple.db");
    let mut database = Database::open(&db_path).unwrap();
    let rows = database.execute_sql("SELECT b FROM t").unwrap();

    assert_eq!(query_rows_to_sqlite_output(&rows), "alpha\nbeta\n");
}

#[test]
fn execute_sql_matches_rowid_filter_output() {
    let db_path = common::fixture_path("simple.db");
    let mut database = Database::open(&db_path).unwrap();
    let rows = database
        .execute_sql("SELECT rowid, a, b FROM t WHERE rowid = 2")
        .unwrap();

    assert_eq!(query_rows_to_sqlite_output(&rows), "2|20|beta\n");
}

#[test]
fn execute_sql_matches_unindexed_equality_filter_output() {
    let db_path = common::fixture_path("simple.db");
    let mut database = Database::open(&db_path).unwrap();
    let rows = database
        .execute_sql("SELECT rowid, a, b FROM t WHERE a = 20")
        .unwrap();

    assert_eq!(query_rows_to_sqlite_output(&rows), "2|20|beta\n");
}

#[test]
fn execute_sql_uses_indexed_integer_equality_filter() {
    let db_path = common::fixture_path("indexed.db");
    let mut database = Database::open(&db_path).unwrap();
    let rows = database
        .execute_sql("SELECT rowid, a, b FROM items WHERE a = 20")
        .unwrap();

    assert_eq!(query_rows_to_sqlite_output(&rows), "2|20|beta\n");
}

#[test]
fn execute_sql_uses_indexed_text_equality_filter() {
    let db_path = common::fixture_path("indexed.db");
    let mut database = Database::open(&db_path).unwrap();
    let rows = database
        .execute_sql("SELECT rowid, a, b FROM items WHERE b = 'beta'")
        .unwrap();

    assert_eq!(query_rows_to_sqlite_output(&rows), "2|20|beta\n");
}

#[test]
fn execute_sql_returns_empty_for_missing_indexed_value() {
    let db_path = common::fixture_path("indexed.db");
    let mut database = Database::open(&db_path).unwrap();
    let rows = database
        .execute_sql("SELECT rowid, a, b FROM items WHERE a = 999")
        .unwrap();

    assert!(rows.is_empty());
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

#[test]
fn lookup_rowid_reads_expected_multipage_fixture_row() {
    let db_path = common::fixture_path("multipage.db");
    let mut pager = Pager::open(&db_path).unwrap();
    let row = lookup_rowid(&mut pager, 2, 120).unwrap().unwrap();

    assert_eq!(row.rowid, 120);
    assert_eq!(
        row.values,
        vec![
            Value::Integer(120),
            Value::Text("row-120-abcdefghijklmnopqrstuvwxyz".to_owned())
        ]
    );
}

#[test]
fn lookup_rowid_reuses_overflow_payload_loading() {
    let db_path = common::fixture_path("overflow.db");
    let mut pager = Pager::open(&db_path).unwrap();
    let row = lookup_rowid(&mut pager, 2, 1).unwrap().unwrap();

    assert_eq!(row.rowid, 1);
    assert_eq!(row.values[0], Value::Integer(1));
    assert_eq!(row.values[1], Value::Text("x".repeat(1800)));
}

#[test]
fn execute_sql_matches_multipage_fixture_expected() {
    let db_path = common::fixture_path("multipage.db");
    let expected_path = common::fixture_path("multipage.expected");
    let mut database = Database::open(&db_path).unwrap();
    let rows = database.execute_sql("SELECT rowid, a, b FROM big").unwrap();

    assert_eq!(
        query_rows_to_sqlite_output(&rows),
        fs::read_to_string(expected_path).unwrap()
    );
}

#[test]
fn rejects_table_btree_cycles() {
    let db_path = copy_fixture_to_temp("multipage.db", "cycle");
    overwrite_first_root_child_page(&db_path, 2).unwrap();

    let mut database = Database::open(&db_path).unwrap();

    assert!(matches!(
        database.scan_table("big"),
        Err(Error::InvalidBtreePage("table b-tree cycle detected"))
    ));

    fs::remove_file(db_path).unwrap();
}

#[test]
fn rejects_table_child_page_beyond_database_size() {
    let db_path = copy_fixture_to_temp("multipage.db", "bad-child-page");
    let pager = Pager::open(&db_path).unwrap();
    let bad_page_number = pager.database_size_pages() + 1;
    drop(pager);
    overwrite_first_root_child_page(&db_path, bad_page_number).unwrap();

    let mut database = Database::open(&db_path).unwrap();

    assert!(matches!(
        database.scan_table("big"),
        Err(Error::InvalidDatabaseHeader(
            "page number exceeds database size"
        ))
    ));

    fs::remove_file(db_path).unwrap();
}

#[test]
fn database_api_scans_named_rows_from_multipage_fixture_table() {
    let db_path = common::fixture_path("multipage.db");
    let mut database = Database::open(&db_path).unwrap();
    let rows = database.scan_table_named("big").unwrap();

    assert_eq!(rows.len(), 120);
    assert_eq!(rows[0].rowid(), 1);
    assert_eq!(rows[0].get("a"), Some(&Value::Integer(1)));
    assert_eq!(
        rows[0].get("b"),
        Some(&Value::Text(
            "row-001-abcdefghijklmnopqrstuvwxyz".to_owned()
        ))
    );
    assert_eq!(rows[119].rowid(), 120);
    assert_eq!(rows[119].get("a"), Some(&Value::Integer(120)));
    assert_eq!(
        rows[119].get("b"),
        Some(&Value::Text(
            "row-120-abcdefghijklmnopqrstuvwxyz".to_owned()
        ))
    );
}

#[test]
fn scans_rows_from_overflow_fixture_table() {
    let db_path = common::fixture_path("overflow.db");
    let expected_path = common::fixture_path("overflow.expected");
    let mut database = Database::open(&db_path).unwrap();

    let rows = database.scan_table("large").unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].rowid, 1);
    assert_eq!(rows[0].values.len(), 2);
    assert_eq!(rows[0].values[0], Value::Integer(1));
    assert_eq!(rows[0].values[1], Value::Text("x".repeat(1800)));
    assert_eq!(
        rows_to_sqlite_output(&rows),
        fs::read_to_string(expected_path).unwrap()
    );
}

#[test]
fn execute_sql_matches_overflow_fixture_expected() {
    let db_path = common::fixture_path("overflow.db");
    let expected_path = common::fixture_path("overflow.expected");
    let mut database = Database::open(&db_path).unwrap();
    let rows = database
        .execute_sql("SELECT rowid, a, b FROM large")
        .unwrap();

    assert_eq!(
        query_rows_to_sqlite_output(&rows),
        fs::read_to_string(expected_path).unwrap()
    );
}

#[test]
fn rejects_overflow_chain_that_ends_early() {
    let db_path = copy_fixture_to_temp("overflow.db", "overflow-corrupt");

    let mut pager = Pager::open(&db_path).unwrap();
    let root_page_number = Schema::load(&mut pager)
        .unwrap()
        .table("large")
        .unwrap()
        .root_page
        .unwrap();
    let root_page = pager.read_page(root_page_number).unwrap();
    let btree_page = BtreePage::parse(&root_page).unwrap();
    let first_overflow_page = btree_page
        .table_leaf_payloads(&root_page, pager.header().usable_space() as usize)
        .unwrap()[0]
        .first_overflow_page
        .unwrap();

    let page_size = pager.header().page_size.get() as u64;
    drop(pager);

    let mut file = OpenOptions::new().write(true).open(&db_path).unwrap();
    file.seek(SeekFrom::Start(
        u64::from(first_overflow_page - 1) * page_size,
    ))
    .unwrap();
    file.write_all(&0_u32.to_be_bytes()).unwrap();

    let mut database = Database::open(&db_path).unwrap();

    assert!(matches!(
        database.scan_table("large"),
        Err(Error::InvalidBtreePage("overflow chain ended early"))
    ));

    fs::remove_file(db_path).unwrap();
}

#[test]
fn rejects_overflow_page_beyond_database_size() {
    let db_path = copy_fixture_to_temp("overflow.db", "overflow-bad-page");

    let mut pager = Pager::open(&db_path).unwrap();
    let root_page_number = Schema::load(&mut pager)
        .unwrap()
        .table("large")
        .unwrap()
        .root_page
        .unwrap();
    let root_page = pager.read_page(root_page_number).unwrap();
    let btree_page = BtreePage::parse(&root_page).unwrap();
    let first_overflow_page = btree_page
        .table_leaf_payloads(&root_page, pager.header().usable_space() as usize)
        .unwrap()[0]
        .first_overflow_page
        .unwrap();

    let page_size = pager.header().page_size.get() as u64;
    let bad_page_number = pager.database_size_pages() + 1;
    drop(pager);

    let mut file = OpenOptions::new().write(true).open(&db_path).unwrap();
    file.seek(SeekFrom::Start(
        u64::from(first_overflow_page - 1) * page_size,
    ))
    .unwrap();
    file.write_all(&bad_page_number.to_be_bytes()).unwrap();

    let mut database = Database::open(&db_path).unwrap();

    assert!(matches!(
        database.scan_table("large"),
        Err(Error::InvalidDatabaseHeader(
            "page number exceeds database size"
        ))
    ));

    fs::remove_file(db_path).unwrap();
}

fn copy_fixture_to_temp(fixture_name: &str, label: &str) -> std::path::PathBuf {
    let source_path = common::fixture_path(fixture_name);
    let db_path = std::env::temp_dir().join(format!("oxlite-{label}-{}.db", std::process::id()));
    fs::copy(source_path, &db_path).unwrap();
    db_path
}

fn overwrite_first_root_child_page(
    path: &std::path::Path,
    child_page_number: u32,
) -> std::io::Result<()> {
    let mut pager = Pager::open(path).unwrap();
    let page_size = pager.header().page_size.get() as u64;
    let root_page = pager.read_page(2).unwrap();
    let root_btree_page = BtreePage::parse(&root_page).unwrap();
    let first_cell_offset = u64::from(root_btree_page.cell_pointers()[0]);
    drop(pager);

    let mut file = OpenOptions::new().write(true).open(path)?;
    file.seek(SeekFrom::Start(page_size + first_cell_offset))?;
    file.write_all(&child_page_number.to_be_bytes())
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

fn query_rows_to_sqlite_output(rows: &[QueryResultRow]) -> String {
    let mut output = String::new();
    for row in rows {
        for (index, (_, value)) in row.values().iter().enumerate() {
            if index > 0 {
                output.push('|');
            }
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
