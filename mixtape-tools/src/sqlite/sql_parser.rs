//! SQL parsing utilities for table extraction
//!
//! This module provides functions to extract table names from SQL queries
//! and categorize them as read or write operations.

use sqlparser::ast::{
    Delete, Expr, FromTable, FunctionArg, FunctionArgExpr, Insert, Query, Select, SetExpr,
    Statement, TableFactor, TableObject, TableWithJoins, Update, UpdateTableFromKind,
};
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;
use std::collections::HashSet;

/// Categorize tables by operation type
#[derive(Debug, Default)]
pub struct TableOperations {
    /// Tables being read from (SELECT, subqueries, JOINs)
    pub read: HashSet<String>,
    /// Tables being written to (INSERT, UPDATE, DELETE target)
    pub write: HashSet<String>,
}

/// Extract tables categorized by read/write operation
pub fn extract_table_operations(sql: &str) -> Result<TableOperations, String> {
    let dialect = SQLiteDialect {};
    let statements =
        Parser::parse_sql(&dialect, sql).map_err(|e| format!("Failed to parse SQL: {}", e))?;

    let mut ops = TableOperations::default();
    for statement in statements {
        categorize_tables(&statement, &mut ops);
    }
    Ok(ops)
}

fn categorize_tables(stmt: &Statement, ops: &mut TableOperations) {
    match stmt {
        Statement::Query(query) => extract_tables_from_query(query, &mut ops.read),
        Statement::Insert(Insert { table, source, .. }) => {
            if let TableObject::TableName(name) = table {
                ops.write.insert(name.to_string());
            }
            if let Some(src) = source {
                extract_tables_from_query(src, &mut ops.read);
            }
        }
        Statement::Update(Update {
            table,
            from,
            selection,
            ..
        }) => {
            // The target table is being written
            if let TableFactor::Table { name, .. } = &table.relation {
                ops.write.insert(name.to_string());
            }
            // JOINs in UPDATE are reads
            for join in &table.joins {
                if let TableFactor::Table { name, .. } = &join.relation {
                    ops.read.insert(name.to_string());
                }
            }
            // FROM clause tables are reads
            if let Some(from_kind) = from {
                let from_tables = match from_kind {
                    UpdateTableFromKind::BeforeSet(tables)
                    | UpdateTableFromKind::AfterSet(tables) => tables,
                };
                for twj in from_tables {
                    extract_tables_from_table_with_joins(twj, &mut ops.read);
                }
            }
            if let Some(expr) = selection {
                extract_tables_from_expr(expr, &mut ops.read);
            }
        }
        Statement::Delete(Delete {
            from, selection, ..
        }) => {
            match from {
                FromTable::WithFromKeyword(tables) | FromTable::WithoutKeyword(tables) => {
                    for twj in tables {
                        if let TableFactor::Table { name, .. } = &twj.relation {
                            ops.write.insert(name.to_string());
                        }
                    }
                }
            }
            if let Some(expr) = selection {
                extract_tables_from_expr(expr, &mut ops.read);
            }
        }
        _ => {}
    }
}

fn extract_tables_from_query(query: &Query, tables: &mut HashSet<String>) {
    // Handle CTEs (WITH clause)
    if let Some(with) = &query.with {
        for cte in &with.cte_tables {
            extract_tables_from_query(&cte.query, tables);
        }
    }
    extract_tables_from_set_expr(&query.body, tables);
}

fn extract_tables_from_set_expr(body: &SetExpr, tables: &mut HashSet<String>) {
    match body {
        SetExpr::Select(select) => extract_tables_from_select(select, tables),
        SetExpr::Query(query) => extract_tables_from_query(query, tables),
        SetExpr::SetOperation { left, right, .. } => {
            extract_tables_from_set_expr(left, tables);
            extract_tables_from_set_expr(right, tables);
        }
        _ => {}
    }
}

fn extract_tables_from_select(select: &Select, tables: &mut HashSet<String>) {
    for twj in &select.from {
        extract_tables_from_table_with_joins(twj, tables);
    }
    if let Some(expr) = &select.selection {
        extract_tables_from_expr(expr, tables);
    }
}

fn extract_tables_from_table_with_joins(twj: &TableWithJoins, tables: &mut HashSet<String>) {
    extract_tables_from_table_factor(&twj.relation, tables);
    for join in &twj.joins {
        extract_tables_from_table_factor(&join.relation, tables);
    }
}

fn extract_tables_from_table_factor(factor: &TableFactor, tables: &mut HashSet<String>) {
    match factor {
        TableFactor::Table { name, .. } => {
            tables.insert(name.to_string());
        }
        TableFactor::Derived { subquery, .. } => {
            extract_tables_from_query(subquery, tables);
        }
        TableFactor::NestedJoin {
            table_with_joins, ..
        } => {
            extract_tables_from_table_with_joins(table_with_joins, tables);
        }
        _ => {}
    }
}

fn extract_tables_from_expr(expr: &Expr, tables: &mut HashSet<String>) {
    match expr {
        Expr::Subquery(query) => extract_tables_from_query(query, tables),
        Expr::InSubquery { subquery, .. } => extract_tables_from_query(subquery, tables),
        Expr::Exists { subquery, .. } => extract_tables_from_query(subquery, tables),
        Expr::BinaryOp { left, right, .. } => {
            extract_tables_from_expr(left, tables);
            extract_tables_from_expr(right, tables);
        }
        Expr::UnaryOp { expr, .. } => extract_tables_from_expr(expr, tables),
        Expr::Between {
            expr, low, high, ..
        } => {
            extract_tables_from_expr(expr, tables);
            extract_tables_from_expr(low, tables);
            extract_tables_from_expr(high, tables);
        }
        Expr::Case {
            operand,
            conditions,
            else_result,
            ..
        } => {
            if let Some(op) = operand {
                extract_tables_from_expr(op, tables);
            }
            for case_when in conditions {
                extract_tables_from_expr(&case_when.condition, tables);
                extract_tables_from_expr(&case_when.result, tables);
            }
            if let Some(else_r) = else_result {
                extract_tables_from_expr(else_r, tables);
            }
        }
        Expr::Nested(inner) => extract_tables_from_expr(inner, tables),
        Expr::InList { expr, list, .. } => {
            extract_tables_from_expr(expr, tables);
            for item in list {
                extract_tables_from_expr(item, tables);
            }
        }
        Expr::Function(func) => {
            if let sqlparser::ast::FunctionArguments::List(arg_list) = &func.args {
                for arg in &arg_list.args {
                    if let FunctionArg::Unnamed(FunctionArgExpr::Expr(e))
                    | FunctionArg::Named {
                        arg: FunctionArgExpr::Expr(e),
                        ..
                    } = arg
                    {
                        extract_tables_from_expr(e, tables);
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_select() {
        let ops = extract_table_operations("SELECT * FROM users").unwrap();
        assert!(ops.read.contains("users"));
        assert_eq!(ops.read.len(), 1);
        assert!(ops.write.is_empty());
    }

    #[test]
    fn test_extract_join() {
        let ops =
            extract_table_operations("SELECT * FROM users u JOIN orders o ON u.id = o.user_id")
                .unwrap();
        assert!(ops.read.contains("users"));
        assert!(ops.read.contains("orders"));
        assert_eq!(ops.read.len(), 2);
        assert!(ops.write.is_empty());
    }

    #[test]
    fn test_extract_subquery() {
        let ops = extract_table_operations(
            "SELECT * FROM users WHERE id IN (SELECT user_id FROM active_sessions)",
        )
        .unwrap();
        assert!(ops.read.contains("users"));
        assert!(ops.read.contains("active_sessions"));
        assert_eq!(ops.read.len(), 2);
    }

    #[test]
    fn test_extract_insert() {
        let ops = extract_table_operations("INSERT INTO users (name) VALUES ('test')").unwrap();
        assert!(ops.write.contains("users"));
        assert!(ops.read.is_empty());
    }

    #[test]
    fn test_categorize_insert_select() {
        let ops = extract_table_operations(
            "INSERT INTO archive SELECT * FROM logs WHERE created_at < '2024-01-01'",
        )
        .unwrap();
        assert!(ops.write.contains("archive"));
        assert!(ops.read.contains("logs"));
    }

    #[test]
    fn test_extract_update() {
        let ops =
            extract_table_operations("UPDATE users SET status = 'active' WHERE id = 1").unwrap();
        assert!(ops.write.contains("users"));
    }

    #[test]
    fn test_extract_delete() {
        let ops = extract_table_operations("DELETE FROM users WHERE id = 1").unwrap();
        assert!(ops.write.contains("users"));
    }

    #[test]
    fn test_extract_cte() {
        let ops = extract_table_operations(
            "WITH active AS (SELECT * FROM users WHERE active = 1) SELECT * FROM active JOIN orders ON active.id = orders.user_id"
        ).unwrap();
        assert!(ops.read.contains("users"));
        assert!(ops.read.contains("orders"));
    }

    #[test]
    fn test_extract_union() {
        let ops =
            extract_table_operations("SELECT id FROM users UNION SELECT id FROM admins").unwrap();
        assert!(ops.read.contains("users"));
        assert!(ops.read.contains("admins"));
    }

    #[test]
    fn test_invalid_sql() {
        let result = extract_table_operations("NOT VALID SQL AT ALL");
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_statements() {
        let ops =
            extract_table_operations("SELECT * FROM users; INSERT INTO logs (msg) VALUES ('test')")
                .unwrap();
        assert!(ops.read.contains("users"));
        assert!(ops.write.contains("logs"));
    }
}
