# rbatis-py

Python bindings for [rbatis](https://github.com/rbatis/rbatis) — a high-performance async ORM written in Rust.

Supports **SQLite**, **MySQL**, **PostgreSQL**, and **MSSQL**.

## Installation

```bash
# https://pypi.org/project/rbatis-py/
pip install rbatis-py
```

Requires Python ≥ 3.8.

## Quick Start

### Raw SQL

```python
import asyncio
from rbatis_py import RBatis

async def main():
    db = RBatis()
    await db.link("sqlite:///path/to/test.db")

    # CREATE / INSERT / UPDATE / DELETE
    await db.exec("CREATE TABLE IF NOT EXISTS user (id INTEGER PRIMARY KEY, name TEXT, age INTEGER)")
    await db.exec("INSERT INTO user (name, age) VALUES (?, ?)", ["Alice", 30])

    # Query — returns List[Dict]
    rows = await db.exec_decode("SELECT * FROM user")
    for row in rows:
        print(row["name"], row["age"])

    # Transaction (auto mode)
    async with db.begin_defer():
        await db.exec("UPDATE user SET age = ? WHERE name = ?", [31, "Alice"])

asyncio.run(main())
```

### CRUD via Model

```python
from rbatis_py import RBatis, Model

class User(Model):
    __table__ = "user"
    id: int | None = None
    name: str | None = None
    age: int | None = None

async def main():
    db = RBatis()
    await db.link("sqlite:///path/to/test.db")

    await User.insert(db, {"name": "Alice", "age": 30})
    await User.insert_batch(db, [
        {"name": "Bob", "age": 25},
        {"name": "Charlie", "age": 35},
    ])
    rows = await User.select_by_map(db, {"name": "Alice"})
    affected = await User.update_by_map(db, {"age": 31}, {"name": "Alice"})
    affected = await User.delete_by_map(db, {"name": "Bob"})
```

## API

### RBatis

| Method | Description | Rust equivalent |
|--------|-------------|-----------------|
| `exec(sql, params)` | Execute INSERT/UPDATE/DELETE | `rb.exec(sql, args).await` |
| `exec_decode(sql, params)` | Query returns `List[Dict]` | `rb.exec_decode::<Vec<Value>>(sql, args).await` |
| `insert(table, data)` | Insert one row | `Model::insert(&rb, &table).await` |
| `insert_batch(table, data_list)` | Batch insert | `Model::insert_batch(&rb, &tables, n).await` |
| `select_by_map(table, condition)` | Select by condition | `Model::select_by_map(&rb, condition).await` |
| `update_by_map(table, data, condition)` | Update by condition | `Model::update_by_map(&rb, &table, condition).await` |
| `delete_by_map(table, condition)` | Delete by condition | `Model::delete_by_map(&rb, condition).await` |
| `link(url)` | Connect to database | `rb.link(driver, url).await` |
| `acquire()` | Acquire a raw connection from pool | `rb.acquire().await` |
| `begin()` | Begin transaction (explicit, returns `Transaction`) | `rb.acquire_begin().await` |
| `begin_defer()` | Begin transaction (auto context manager) | `rb.acquire_begin().await` |
| `commit()` | Commit current active transaction | `tx.commit().await` |
| `rollback()` | Rollback current active transaction | `tx.rollback().await` |
| `ping()` | Test connection | `rb.exec("SELECT 1").await` |
| `close()` | Close connection | — |
| `set_pool_max_size(n)` | Set max connections in the pool | `pool.set_max_open_conns(n).await` |
| `set_pool_max_idle(n)` | Set max idle connections in the pool | `pool.set_max_idle_conns(n).await` |
| `set_pool_connect_timeout(s)` | Set connection timeout (seconds) | `pool.set_timeout(dur).await` |
| `set_pool_max_lifetime(s)` | Set max connection lifetime (seconds) | `pool.set_conn_max_lifetime(dur).await` |
| `pool_state()` | Inspect pool state (returns dict) | `pool.state().await` |

### Transaction

Two transaction modes are supported:

**A) Explicit (manual commit/rollback):**

```python
tx = await db.begin()
try:
    await tx.exec("INSERT INTO user (name) VALUES (?)", ["Alice"])
    await tx.exec("INSERT INTO user (name) VALUES (?)", ["Bob"])
    await tx.commit()
except Exception:
    await tx.rollback()
    raise
```

**B) Auto (context manager):**

```python
async with db.begin_defer():
    await db.exec("INSERT INTO user (name) VALUES (?)", ["Alice"])
    # auto-commit on success, auto-rollback on exception
```

### Connection

Acquire a raw connection from the pool to run queries in isolation:

```python
conn = await db.acquire()
try:
    rows = await conn.exec_decode("SELECT * FROM user WHERE age > ?", [20])

    # You can also begin a transaction on this specific connection
    tx = await conn.begin()
    try:
        await tx.exec("INSERT INTO user (name) VALUES (?)", ["Alice"])
        await tx.commit()
    except Exception:
        await tx.rollback()
        raise
finally:
    await conn.close()  # return to pool
```

### Model

Define a table model:

```python
class User(Model):
    __table__ = "user"
    id: int | None = None
    name: str | None = None
    age: int | None = None
```

Then use classmethods for CRUD. The ``db`` parameter accepts **RBatis**, **Connection**, or **Transaction** — all support ``exec()`` / ``exec_decode()``:

```python
await User.insert(db, {...})
await User.select_by_map(db, {condition})
await User.update_by_map(db, {set_data}, {condition})
await User.delete_by_map(db, {condition})
```

For example, with a raw connection:

```python
conn = await db.acquire()
try:
    await User.insert(conn, {"name": "Alice"})
    rows = await User.select_by_map(conn, {"name": "Alice"})
finally:
    conn.close()
```

## Type Conversion

Python types are automatically converted to/from rbatis extended types:

| Python type | rbs serialization | Database type |
|-------------|-------------------|---------------|
| `datetime.datetime` | `Ext("DateTime", ...)` | `rbdc::DateTime` |
| `datetime.date` | `Ext("Date", ...)` | `rbdc::Date` |
| `datetime.time` | `Ext("Time", ...)` | `rbdc::Time` |
| `decimal.Decimal` | `Ext("Decimal", ...)` | `rbdc::Decimal` |
| `uuid.UUID` | `Ext("Uuid", ...)` | `rbdc::Uuid` |
| `dict` / `list` | `Ext("Json", ...)` | `rbdc::Json` |

**Note:** SQLite stores all types as TEXT; the conversion to Python types works for all databases.
PostgreSQL returns timestamps as integer (milliseconds) rather than `datetime` objects.

## Connection URLs

```python
# SQLite
await db.link("sqlite:///path/to/db.sqlite")

# MySQL
await db.link("mysql://user:pass@localhost:3306/db")

# PostgreSQL
await db.link("postgres://user:pass@localhost:5432/db")

# MSSQL
await db.link("jdbc:sqlserver://localhost:1433;User=SA;Password=xxx;Database=db")
```

## Connection Pool

Configure the connection pool after calling `link()`. These methods must be called **after** a connection is established.

```python
await db.link("mysql://user:pass@localhost:3306/mydb")

# Configure pool
await db.set_pool_max_size(20)          # Max 20 connections
await db.set_pool_max_idle(5)           # Keep at most 5 idle connections
await db.set_pool_connect_timeout(30)   # Timeout waiting for a connection (seconds)
await db.set_pool_max_lifetime(3600)    # Max connection lifetime (seconds)

# Inspect pool state
state = await db.pool_state()
print(state)
# e.g. {"max_open": 20, "connections": 3, "in_use": 1, "idle": 2, "waits": 0, ...}
```

## Development

```bash
git clone https://github.com/rbatis/rbatis-py.git
cd rbatis-py
pip install maturin
maturin develop  # build and install in current venv
```

### Running examples

```bash
uv run python examples/basic_usage.py     # exec, exec_decode, transactions
uv run python examples/crud_usage.py      # Model CRUD
uv run python examples/crud_usage.py mysql     # with MySQL
uv run python examples/crud_usage.py postgres  # with PostgreSQL
```

## Publishing to PyPI

```bash
# Install maturin if not already installed
pip install maturin

# Build wheel
maturin build

# Publish to PyPI (requires PyPI account and API token)
maturin publish

# Or use the official GitHub Action:
# https://github.com/PyO3/maturin-action
```

## License

MIT
