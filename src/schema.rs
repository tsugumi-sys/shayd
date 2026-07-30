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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaObjectType {
    Table,
    Index,
    View,
    Trigger,
}

impl Schema {
    pub fn load(pager: &mut Pager) -> Result<Self> {
        let page = pager.read_page(1)?;
        let btree_page = BtreePage::parse(&page)?;
        if btree_page.header().page_type != PageType::TableLeaf {
            return Err(Error::Unsupported(
                "multi-page sqlite_schema is not supported",
            ));
        }

        let objects = btree_page
            .table_leaf_cells(&page)?
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

        Ok(Self {
            object_type,
            name,
            table_name,
            root_page,
            sql,
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
    }
}
