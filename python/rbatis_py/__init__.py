"""rbatis-py – high-performance async ORM for Python, powered by rbatis (Rust).

Supports SQLite, MySQL, PostgreSQL, MSSQL.

Usage:

    from rbatis_py import RBatis, Model

    class User(Model):
        __table__ = "user"

    db = RBatis()
    await db.link("sqlite://target/test.db")

    # raw SQL
    await db.exec("INSERT INTO user (name) VALUES (?)", ["Alice"])
    rows = await db.exec_decode("SELECT * FROM user")

    # CRUD via Model (works with RBatis, Connection, or Transaction)
    await User.insert(db, {"name": "Bob"})
    rows = await User.select_by_map(db, {"name": "Bob"})

    # transaction
    async with db.begin_defer():
        await db.exec("UPDATE user SET name = ? WHERE id = ?", ["new", 1])
"""

from typing import Any, Dict, List

from rbatis_py._core import RBatis as _CoreRBatis
from rbatis_py._core import Transaction as _Transaction
from rbatis_py._core import Connection as _Connection
from rbatis_py._core import DeferredTransaction as _DeferredTransaction
from rbatis_py._core import __version__

__all__ = [
    "RBatis",
    "Transaction",
    "Connection",
    "DeferredTransaction",
    "Model",
    "__version__",
]


# Re-export core classes
RBatis = _CoreRBatis
Transaction = _Transaction
Connection = _Connection
DeferredTransaction = _DeferredTransaction


class Model:
    """Base class for table models (like rbatis ``crud!`` macro).

    Subclasses must define ``__table__``:

        class User(Model):
            __table__ = "user"

    Then use classmethods for CRUD.
    The ``db`` parameter accepts **RBatis**, **Connection**, or **Transaction**.

        await User.insert(db, {"name": "Alice", "age": 30})
        rows = await User.select_by_map(db, {"name": "Alice"})
    """

    __table__ = ""

    @classmethod
    async def insert(cls, db: RBatis, data: Dict[str, Any]) -> int:
        """Insert a record. Returns rows affected."""
        cols = ",".join(data.keys())
        ph = ",".join(["?"] * len(data))
        sql = "INSERT INTO {} ({}) VALUES ({})".format(cls.__table__, cols, ph)
        return await db.exec(sql, list(data.values()))

    @classmethod
    async def insert_batch(cls, db: RBatis, data_list: List[Dict[str, Any]]) -> int:
        """Batch insert records. Returns rows affected."""
        if not data_list:
            return 0
        # Collect all column names (preserve insertion order)
        seen = set()
        columns = []
        for d in data_list:
            for k in d:
                if k not in seen:
                    columns.append(k)
                    seen.add(k)
        cols_str = ",".join(columns)

        groups = []
        all_vals = []
        for d in data_list:
            row_placeholders = []
            for c in columns:
                all_vals.append(d.get(c))
                row_placeholders.append("?")
            groups.append("({})".format(",".join(row_placeholders)))

        sql = "INSERT INTO {} ({}) VALUES {}".format(cls.__table__, cols_str, ",".join(groups))
        return await db.exec(sql, all_vals)

    @classmethod
    async def select_by_map(
        cls, db: RBatis, condition: Dict[str, Any]
    ) -> List[Dict[str, Any]]:
        """Select records by equality condition. Returns list of dicts."""
        if not condition:
            raise ValueError("select_by_map needs condition dict")
        wh = " AND ".join("{} = ?".format(k) for k in condition)
        sql = "SELECT * FROM {} WHERE {}".format(cls.__table__, wh)
        return await db.exec_decode(sql, list(condition.values()))

    @classmethod
    async def update_by_map(
        cls,
        db: RBatis,
        data: Dict[str, Any],
        condition: Dict[str, Any],
    ) -> int:
        """Update records by equality condition. Returns rows affected."""
        if not data or not condition:
            raise ValueError("update_by_map needs data + condition")
        set_clause = ",".join("{} = ?".format(k) for k in data)
        wh = " AND ".join("{} = ?".format(k) for k in condition)
        sql = "UPDATE {} SET {} WHERE {}".format(cls.__table__, set_clause, wh)
        return await db.exec(sql, list(data.values()) + list(condition.values()))

    @classmethod
    async def delete_by_map(cls, db: RBatis, condition: Dict[str, Any]) -> int:
        """Delete records by equality condition. Returns rows affected."""
        if not condition:
            raise ValueError("delete_by_map needs condition dict")
        wh = " AND ".join("{} = ?".format(k) for k in condition)
        sql = "DELETE FROM {} WHERE {}".format(cls.__table__, wh)
        return await db.exec(sql, list(condition.values()))
