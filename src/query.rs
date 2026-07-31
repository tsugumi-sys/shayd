use crate::error::{Error, Result};
use crate::record::Value;
use crate::table::NamedRow;

#[derive(Debug, Clone)]
pub struct TableQuery {
    table_name: String,
    projections: Vec<String>,
    rowid_eq: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryResultRow {
    values: Vec<(String, Value)>,
}

impl TableQuery {
    pub fn new(table_name: impl Into<String>) -> Self {
        Self {
            table_name: table_name.into(),
            projections: Vec::new(),
            rowid_eq: None,
        }
    }

    pub fn table_name(&self) -> &str {
        &self.table_name
    }

    pub fn select<const N: usize>(mut self, projections: [&str; N]) -> Self {
        self.projections = projections.into_iter().map(str::to_owned).collect();
        self
    }

    pub(crate) fn select_columns(mut self, projections: Vec<String>) -> Self {
        self.projections = projections;
        self
    }

    pub fn rowid_eq(mut self, rowid: i64) -> Self {
        self.rowid_eq = Some(rowid);
        self
    }

    pub(crate) fn execute(self, rows: Vec<NamedRow>) -> Result<Vec<QueryResultRow>> {
        let projections: Vec<String> = if self.projections.is_empty() {
            default_projections(rows.first())
        } else {
            self.projections
        };

        rows.into_iter()
            .filter(|row| self.rowid_eq.is_none_or(|rowid| row.rowid() == rowid))
            .map(|row| project_row(&row, &projections))
            .collect()
    }
}

impl QueryResultRow {
    fn new(values: Vec<(String, Value)>) -> Self {
        Self { values }
    }

    pub fn get(&self, column_name: &str) -> Option<&Value> {
        self.values
            .iter()
            .find(|(name, _)| name == column_name)
            .map(|(_, value)| value)
    }

    pub fn values(&self) -> &[(String, Value)] {
        &self.values
    }
}

fn default_projections(row: Option<&NamedRow>) -> Vec<String> {
    let Some(row) = row else {
        return Vec::new();
    };

    row.columns().to_vec()
}

fn project_row(row: &NamedRow, projections: &[String]) -> Result<QueryResultRow> {
    let mut values = Vec::with_capacity(projections.len());
    for projection in projections {
        let value = if projection == "rowid" {
            Value::Integer(row.rowid())
        } else {
            row.get(projection)
                .ok_or(Error::InvalidSchema("unknown projection column"))?
                .clone()
        };
        values.push((projection.clone(), value));
    }

    Ok(QueryResultRow::new(values))
}
