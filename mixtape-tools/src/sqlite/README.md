# SQLite Tools

A comprehensive set of SQLite database tools for AI agents, with granular permission control.

## Quick Start

```rust
use mixtape_core::Agent;
use mixtape_tools::sqlite;

// Read-only agent
let agent = Agent::builder()
    .bedrock(ClaudeSonnet4)
    .add_tools(sqlite::read_only_tools())
    .build()
    .await?;

// Full access agent
let agent = Agent::builder()
    .bedrock(ClaudeSonnet4)
    .add_tools(sqlite::all_tools())
    .build()
    .await?;
```

## Tool Groups

| Function | Count | Description |
|----------|-------|-------------|
| `read_only_tools()` | 9 | Read-only operations - exploration, queries, backups |
| `destructive_tools()` | 7 | Write operations - schema changes, data modifications |
| `transaction_tools()` | 3 | Transaction management |
| `all_tools()` | 19 | Everything |

## Common Patterns

### Read-Only Explorer

For agents that should only query databases:

```rust
.add_tools(sqlite::read_only_tools())
```

Includes: open/close databases, list/describe tables, SELECT queries, export schema, backups.

### Data Entry

For agents that insert/update data but don't modify schema:

```rust
use mixtape_tools::sqlite::{self, *};

.add_tools(sqlite::read_only_tools())
.add_tool(WriteQueryTool)
.add_tool(BulkInsertTool)
.add_tools(sqlite::transaction_tools())
```

### Schema Migration

For agents that manage database schemas:

```rust
use mixtape_tools::sqlite::{self, *};

.add_tools(sqlite::read_only_tools())
.add_tool(CreateTableTool)
.add_tool(SchemaQueryTool)
.add_tool(ImportSchemaTool)
.add_tools(sqlite::transaction_tools())
```

## All Tools

### Safe (Read-Only)

| Tool | Description |
|------|-------------|
| `sqlite_open_database` | Open or create a database |
| `sqlite_close_database` | Close a database connection |
| `sqlite_list_databases` | Discover database files |
| `sqlite_database_info` | Get database metadata |
| `sqlite_list_tables` | List tables and views |
| `sqlite_describe_table` | Get table schema |
| `sqlite_read_query` | SELECT/PRAGMA/EXPLAIN queries |
| `sqlite_export_schema` | Export schema as SQL or JSON |
| `sqlite_backup` | Create database backup |

### Destructive (Write/Modify)

| Tool | Description |
|------|-------------|
| `sqlite_create_table` | Create a new table |
| `sqlite_drop_table` | Drop a table |
| `sqlite_write_query` | INSERT/UPDATE/DELETE |
| `sqlite_schema_query` | CREATE/ALTER/DROP DDL |
| `sqlite_bulk_insert` | Batch insert records |
| `sqlite_import_schema` | Import and execute schema |
| `sqlite_vacuum` | Optimize database storage |

### Transaction Management

| Tool | Description |
|------|-------------|
| `sqlite_begin_transaction` | Start a transaction |
| `sqlite_commit_transaction` | Commit a transaction |
| `sqlite_rollback_transaction` | Rollback a transaction |

## Multi-Database Support

All tools support working with multiple databases simultaneously. Use the `db_path` parameter to specify which database to operate on, or omit it to use the default (first opened) database.

```rust
// Open multiple databases
sqlite_open_database { db_path: "/data/users.db" }
sqlite_open_database { db_path: "/data/products.db" }

// Query specific database
sqlite_read_query {
    query: "SELECT * FROM users",
    db_path: "/data/users.db"
}
```
