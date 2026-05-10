# rbatis-py

Python async database client powered by **rbdc** (Rust Database Connectivity) — a high-performance async database connectivity layer.

Supports **SQLite**, **MySQL**, **PostgreSQL**, **MSSQL**, **Turso/libSQL**, and **DuckDB**.

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

    # Transaction (auto commit/rollback)
    tx = await db.begin()
    async with tx.auto_commit() as g:
        await g.exec("UPDATE user SET age = ? WHERE name = ?", [31, "Alice"])

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

| Method | Description |
|--------|-------------|
| `exec(sql, params)` | Execute INSERT/UPDATE/DELETE |
| `exec_decode(sql, params)` | Query returns `List[Dict]` |
| `insert(table, data)` | Insert one row |
| `insert_batch(table, data_list)` | Batch insert |
| `select_by_map(table, condition)` | Select by condition |
| `update_by_map(table, data, condition)` | Update by condition |
| `delete_by_map(table, condition)` | Delete by condition |
| `link(url)` | Connect to database |
| `acquire()` | Acquire a raw connection from pool |
| `begin()` | Begin transaction (explicit, returns `Transaction`) |
| `begin_defer()` | Begin transaction with auto-commit guard |
| `commit()` | Commit current active transaction |
| `rollback()` | Rollback current active transaction |
| `ping()` | Test connection |
| `close()` | Close connection |
| `set_pool_max_size(n)` | Set max connections in the pool |
| `set_pool_max_idle(n)` | Set max idle connections in the pool |
| `set_pool_connect_timeout(s)` | Set connection timeout (seconds) |
| `set_pool_max_lifetime(s)` | Set max connection lifetime (seconds) |
| `pool_state()` | Inspect pool state (returns dict) |

### Transaction

Three transaction modes are supported:

**A) Explicit (manual commit/rollback):**

```python
tx = await db.begin()
try:
    await tx.exec("INSERT INTO user (name) VALUES (?)", ["Alice"])
    await tx.commit()
except Exception:
    await tx.rollback()
    raise
```

**B) Auto-commit via `auto_commit()`:**

```python
tx = await db.begin()
async with tx.auto_commit() as g:
    await g.exec("INSERT INTO user (name) VALUES (?)", ["Alice"])
    # auto-commit on success, auto-rollback on exception
```

If you call `await g.commit()` or `await g.rollback()` explicitly inside the block,
the guard will no-op on exit:

```python
tx = await db.begin()
async with tx.auto_commit() as g:
    await g.exec("INSERT ...")
    await g.commit()   # explicit commit — guard does nothing on exit
```

**C) One-liner via `begin_defer()`:**

```python
async with db.begin_defer() as g:
    await g.exec("INSERT INTO user (name) VALUES (?)", ["Alice"])
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
    async with tx.auto_commit() as g:
        await g.exec("INSERT INTO user (name) VALUES (?)", ["Alice"])
finally:
    conn.close()  # return to pool (not async)
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

Python types are automatically converted to/from rbdc extended types:

| Python type | Database type |
|-------------|---------------|
| `datetime.datetime` | `DateTime` |
| `datetime.date` | `Date` |
| `datetime.time` | `Time` |
| `decimal.Decimal` | `Decimal` |
| `uuid.UUID` | `Uuid` |
| `dict` / `list` | `Json` |

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
uv run python examples/tx.py              # transaction patterns
uv run python examples/crud_usage.py      # Model CRUD
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
