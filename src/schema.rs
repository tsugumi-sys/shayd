use crate::btree::{BtreePage, PageType};
use crate::error::{Error, Result};
use crate::pager::Pager;
use crate::record::{Record, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    objects: Vec<SchemaObject>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaObject {
    pub object_type: SchemaObjectType,
    pub name: String,
    pub table_name: String,
    pub root_page: Option<u32>,
    pub sql: Option<String>,
    pub table_schema: Option<TableSchema>,
    pub index_schema: Option<IndexSchema>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaObjectType {
    Table,
    Index,
    View,
    Trigger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnSchema>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSchema {
    pub name: String,
    pub declared_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSchema {
    pub name: String,
    pub table_name: String,
    pub root_page: Option<u32>,
    pub columns: Vec<String>,
    pub unique: bool,
}

impl Schema {
    pub fn load(pager: &mut Pager) -> Result<Self> {
        let page = pager.read_page(1)?;
        let usable_size = pager.header().usable_space() as usize;
        let btree_page = BtreePage::parse_with_usable_size(&page, usable_size)?;
        if btree_page.header().page_type != PageType::TableLeaf {
            return Err(Error::Unsupported(
                "multi-page sqlite_schema is not supported",
            ));
        }

        let objects = btree_page
            .table_leaf_cells_with_usable_size(&page, usable_size)?
            .into_iter()
            .map(|cell| SchemaObject::decode(&cell.record))
            .collect::<Result<Vec<_>>>()?;

        Ok(Self { objects })
    }

    pub fn objects(&self) -> &[SchemaObject] {
        &self.objects
    }

    pub fn table(&self, name: &str) -> Option<&SchemaObject> {
        self.objects
            .iter()
            .find(|object| object.object_type == SchemaObjectType::Table && object.name == name)
    }

    pub fn table_schema(&self, name: &str) -> Option<&TableSchema> {
        self.table(name)
            .and_then(|object| object.table_schema.as_ref())
    }

    pub fn indexes_for_table(&self, table_name: &str) -> Vec<&IndexSchema> {
        self.objects
            .iter()
            .filter_map(|object| object.index_schema.as_ref())
            .filter(|index| index.table_name == table_name)
            .collect()
    }

    pub fn index(&self, name: &str) -> Option<&IndexSchema> {
        self.objects
            .iter()
            .filter_map(|object| object.index_schema.as_ref())
            .find(|index| index.name == name)
    }

    pub fn index_for_table_column(
        &self,
        table_name: &str,
        column_name: &str,
    ) -> Option<&IndexSchema> {
        self.indexes_for_table(table_name)
            .into_iter()
            .find(|index| index.columns.iter().any(|column| column == column_name))
    }
}

impl SchemaObject {
    fn decode(record: &Record) -> Result<Self> {
        let values = record.values();
        if values.len() != 5 {
            return Err(Error::InvalidSchema(
                "sqlite_schema records must have 5 columns",
            ));
        }

        let object_type = SchemaObjectType::parse(text_value(&values[0], "type")?)?;
        let name = text_value(&values[1], "name")?.to_owned();
        let table_name = text_value(&values[2], "tbl_name")?.to_owned();
        let root_page = match &values[3] {
            Value::Integer(0) | Value::Null => None,
            Value::Integer(value) if *value > 0 => Some(
                u32::try_from(*value)
                    .map_err(|_| Error::InvalidSchema("rootpage is out of range"))?,
            ),
            Value::Integer(_) => return Err(Error::InvalidSchema("rootpage cannot be negative")),
            _ => return Err(Error::InvalidSchema("rootpage must be an integer")),
        };
        let sql = match &values[4] {
            Value::Null => None,
            value => Some(text_value(value, "sql")?.to_owned()),
        };
        let table_schema = match (object_type, sql.as_deref()) {
            (SchemaObjectType::Table, Some(sql)) => Some(TableSchema::parse(sql)?),
            _ => None,
        };
        let index_schema = match (object_type, sql.as_deref()) {
            (SchemaObjectType::Index, Some(sql)) => {
                let mut index_schema = IndexSchema::parse(sql)?;
                index_schema.root_page = root_page;
                Some(index_schema)
            }
            _ => None,
        };

        Ok(Self {
            object_type,
            name,
            table_name,
            root_page,
            sql,
            table_schema,
            index_schema,
        })
    }
}

impl SchemaObjectType {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "table" => Ok(Self::Table),
            "index" => Ok(Self::Index),
            "view" => Ok(Self::View),
            "trigger" => Ok(Self::Trigger),
            _ => Err(Error::InvalidSchema("unknown schema object type")),
        }
    }
}

impl TableSchema {
    pub fn parse(sql: &str) -> Result<Self> {
        let sql = sql.trim();
        let rest = strip_ascii_prefix(sql, "CREATE")
            .and_then(|rest| strip_ascii_prefix(rest, "TABLE"))
            .ok_or(Error::Unsupported(
                "only simple CREATE TABLE schema SQL is supported",
            ))?;
        let open_paren = rest
            .find('(')
            .ok_or(Error::InvalidSchema("CREATE TABLE missing column list"))?;
        let close_paren = rest.rfind(')').ok_or(Error::InvalidSchema(
            "CREATE TABLE missing closing parenthesis",
        ))?;
        if close_paren <= open_paren {
            return Err(Error::InvalidSchema("empty CREATE TABLE column list"));
        }

        let name = parse_identifier(rest[..open_paren].trim())?.to_owned();
        let column_defs = split_top_level_commas(&rest[open_paren + 1..close_paren])?;
        let mut columns = Vec::with_capacity(column_defs.len());
        for column_def in column_defs {
            let column_def = column_def.trim();
            if is_table_constraint(column_def) {
                return Err(Error::Unsupported("table constraints are not supported"));
            }

            let (column_name, declared_type) = parse_column_def(column_def)?;
            columns.push(ColumnSchema {
                name: column_name.to_owned(),
                declared_type: declared_type.map(str::to_owned),
            });
        }
        if columns.is_empty() {
            return Err(Error::InvalidSchema("CREATE TABLE must have columns"));
        }

        Ok(Self { name, columns })
    }
}

impl IndexSchema {
    pub fn parse(sql: &str) -> Result<Self> {
        let sql = sql.trim();
        let rest = strip_ascii_prefix(sql, "CREATE").ok_or(Error::Unsupported(
            "only simple CREATE INDEX schema SQL is supported",
        ))?;
        let (unique, rest) = match strip_ascii_prefix(rest, "UNIQUE") {
            Some(rest) => (true, rest),
            None => (false, rest),
        };
        let rest = strip_ascii_prefix(rest, "INDEX").ok_or(Error::Unsupported(
            "only simple CREATE INDEX schema SQL is supported",
        ))?;
        let on_offset = find_ascii_keyword(rest, "ON")
            .ok_or(Error::InvalidSchema("CREATE INDEX missing ON clause"))?;
        let name = parse_simple_identifier(rest[..on_offset].trim())?.to_owned();
        let rest = rest[on_offset + "ON".len()..].trim_start();

        let open_paren = rest
            .find('(')
            .ok_or(Error::InvalidSchema("CREATE INDEX missing column list"))?;
        let close_paren = rest.rfind(')').ok_or(Error::InvalidSchema(
            "CREATE INDEX missing closing parenthesis",
        ))?;
        if close_paren <= open_paren {
            return Err(Error::InvalidSchema("empty CREATE INDEX column list"));
        }
        if !rest[close_paren + 1..].trim().is_empty() {
            return Err(Error::Unsupported(
                "partial indexes and trailing index clauses are not supported",
            ));
        }

        let table_name = parse_simple_identifier(rest[..open_paren].trim())?.to_owned();
        let columns = split_top_level_commas(&rest[open_paren + 1..close_paren])?;
        if columns.len() != 1 {
            return Err(Error::Unsupported("multi-column indexes are not supported"));
        }
        let column_name = parse_simple_identifier(columns[0].trim())?.to_owned();

        Ok(Self {
            name,
            table_name,
            root_page: None,
            columns: vec![column_name],
            unique,
        })
    }
}

fn text_value<'a>(value: &'a Value, column: &'static str) -> Result<&'a str> {
    match value {
        Value::Text(text) => Ok(text),
        _ => Err(Error::InvalidSchema(match column {
            "type" => "schema type must be text",
            "name" => "schema name must be text",
            "tbl_name" => "schema tbl_name must be text",
            "sql" => "schema sql must be text",
            _ => "schema column must be text",
        })),
    }
}

fn strip_ascii_prefix<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    let input = input.trim_start();
    input
        .get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
        .then_some(input[prefix.len()..].trim_start())
}

fn parse_identifier(input: &str) -> Result<&str> {
    let identifier = input
        .split_whitespace()
        .next()
        .ok_or(Error::InvalidSchema("missing identifier"))?;
    if identifier.starts_with(['"', '\'', '`', '[']) {
        return Err(Error::Unsupported("quoted identifiers are not supported"));
    }
    Ok(identifier)
}

fn parse_simple_identifier(input: &str) -> Result<&str> {
    let identifier = parse_identifier(input)?;
    if identifier.len() != input.trim().len()
        || identifier.is_empty()
        || identifier.as_bytes()[0].is_ascii_digit()
        || !identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(Error::Unsupported(
            "only simple unquoted identifiers are supported",
        ));
    }
    Ok(identifier)
}

fn find_ascii_keyword(input: &str, keyword: &str) -> Option<usize> {
    input
        .split_ascii_whitespace()
        .scan(0, |offset, part| {
            let start = input[*offset..]
                .find(part)
                .map(|relative| *offset + relative)?;
            *offset = start + part.len();
            Some((start, part))
        })
        .find_map(|(start, part)| part.eq_ignore_ascii_case(keyword).then_some(start))
}

fn split_top_level_commas(input: &str) -> Result<Vec<&str>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0_usize;

    for (index, byte) in input.bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(Error::InvalidSchema("unbalanced column definition"))?;
            }
            b',' if depth == 0 => {
                parts.push(&input[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }

    if depth != 0 {
        return Err(Error::InvalidSchema("unbalanced column definition"));
    }
    parts.push(&input[start..]);
    Ok(parts)
}

fn is_table_constraint(column_def: &str) -> bool {
    let Some(first) = column_def.split_whitespace().next() else {
        return false;
    };
    matches!(
        first.to_ascii_uppercase().as_str(),
        "CONSTRAINT" | "PRIMARY" | "FOREIGN" | "UNIQUE" | "CHECK"
    )
}

fn parse_column_def(column_def: &str) -> Result<(&str, Option<&str>)> {
    let column_name = parse_identifier(column_def)?;
    let rest = column_def[column_name.len()..].trim();
    if rest.is_empty() {
        return Ok((column_name, None));
    }

    let declared_type = rest
        .split(|character: char| character.is_ascii_whitespace())
        .next()
        .filter(|value| !value.is_empty());
    Ok((column_name, declared_type))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_schema_object_record() {
        let record = Record::new(vec![
            Value::Text("table".to_owned()),
            Value::Text("t".to_owned()),
            Value::Text("t".to_owned()),
            Value::Integer(2),
            Value::Text("CREATE TABLE t(a INTEGER, b TEXT)".to_owned()),
        ]);

        let object = SchemaObject::decode(&record).unwrap();

        assert_eq!(object.object_type, SchemaObjectType::Table);
        assert_eq!(object.name, "t");
        assert_eq!(object.table_name, "t");
        assert_eq!(object.root_page, Some(2));
        assert_eq!(
            object.sql,
            Some("CREATE TABLE t(a INTEGER, b TEXT)".to_owned())
        );
        assert_eq!(
            object.table_schema,
            Some(TableSchema {
                name: "t".to_owned(),
                columns: vec![
                    ColumnSchema {
                        name: "a".to_owned(),
                        declared_type: Some("INTEGER".to_owned()),
                    },
                    ColumnSchema {
                        name: "b".to_owned(),
                        declared_type: Some("TEXT".to_owned()),
                    },
                ],
            })
        );
        assert_eq!(object.index_schema, None);
    }

    #[test]
    fn decodes_index_schema_object_record() {
        let record = Record::new(vec![
            Value::Text("index".to_owned()),
            Value::Text("idx_t_a".to_owned()),
            Value::Text("t".to_owned()),
            Value::Integer(3),
            Value::Text("CREATE INDEX idx_t_a ON t(a)".to_owned()),
        ]);

        let object = SchemaObject::decode(&record).unwrap();

        assert_eq!(object.object_type, SchemaObjectType::Index);
        assert_eq!(object.name, "idx_t_a");
        assert_eq!(object.table_name, "t");
        assert_eq!(object.root_page, Some(3));
        assert_eq!(object.table_schema, None);
        assert_eq!(
            object.index_schema,
            Some(IndexSchema {
                name: "idx_t_a".to_owned(),
                table_name: "t".to_owned(),
                root_page: Some(3),
                columns: vec!["a".to_owned()],
                unique: false,
            })
        );
    }

    #[test]
    fn parses_create_table_column_metadata() {
        let table = TableSchema::parse("CREATE TABLE t (\n  a INTEGER,\n  b TEXT\n)").unwrap();

        assert_eq!(table.name, "t");
        assert_eq!(
            table.columns,
            vec![
                ColumnSchema {
                    name: "a".to_owned(),
                    declared_type: Some("INTEGER".to_owned()),
                },
                ColumnSchema {
                    name: "b".to_owned(),
                    declared_type: Some("TEXT".to_owned()),
                },
            ]
        );
    }

    #[test]
    fn rejects_table_constraints_for_now() {
        assert!(matches!(
            TableSchema::parse("CREATE TABLE t (a INTEGER, PRIMARY KEY (a))"),
            Err(Error::Unsupported("table constraints are not supported"))
        ));
    }

    #[test]
    fn parses_create_index_metadata() {
        assert_eq!(
            IndexSchema::parse("CREATE INDEX idx_t_a ON t(a)").unwrap(),
            IndexSchema {
                name: "idx_t_a".to_owned(),
                table_name: "t".to_owned(),
                root_page: None,
                columns: vec!["a".to_owned()],
                unique: false,
            }
        );
    }

    #[test]
    fn parses_create_unique_index_metadata() {
        assert_eq!(
            IndexSchema::parse("CREATE UNIQUE INDEX idx_t_a ON t(a)").unwrap(),
            IndexSchema {
                name: "idx_t_a".to_owned(),
                table_name: "t".to_owned(),
                root_page: None,
                columns: vec!["a".to_owned()],
                unique: true,
            }
        );
    }

    #[test]
    fn rejects_unsupported_index_forms() {
        assert!(matches!(
            IndexSchema::parse("CREATE INDEX idx_t_ab ON t(a, b)"),
            Err(Error::Unsupported("multi-column indexes are not supported"))
        ));
        assert!(matches!(
            IndexSchema::parse("CREATE INDEX idx_t_lower ON t(lower(a))"),
            Err(Error::Unsupported(
                "only simple unquoted identifiers are supported"
            ))
        ));
        assert!(matches!(
            IndexSchema::parse("CREATE INDEX idx_t_a ON t(a) WHERE a > 1"),
            Err(Error::Unsupported(
                "partial indexes and trailing index clauses are not supported"
            ))
        ));
    }
}
