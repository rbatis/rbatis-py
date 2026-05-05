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

    # CRUD via Model
    await User.insert(db, {"name": "Bob"})
    rows = await User.select_by_map(db, {"name": "Bob"})

    # transaction
    async with db.begin():
        await db.exec("UPDATE user SET name = ? WHERE id = ?", ["new", 1])
"""

from typing import Any, Dict, List, Optional, TypeVar

from rbatis_py._core import RBatis as _CoreRBatis
from rbatis_py._core import Transaction as _Transaction
from rbatis_py._core import __version__

__all__ = [
    "RBatis",
    "Transaction",
    "Model",
    "__version__",
]


# Re-export core classes
RBatis = _CoreRBatis
Transaction = _Transaction


class Model:
    """Base class for table models (like rbatis ``crud!`` macro).

    Subclasses must define ``__table__``:

        class User(Model):
            __table__ = "user"

    Then use classmethods for CRUD:

        await User.insert(db, {"name": "Alice", "age": 30})
        rows = await User.select_by_map(db, {"name": "Alice"})
    """

    __table__ = ""

    @classmethod
    async def insert(cls, db: RBatis, data: Dict[str, Any]) -> int:
        """Insert a record. Returns rows affected."""
        return await db.insert(cls.__table__, data)

    @classmethod
    async def insert_batch(cls, db: RBatis, data: List[Dict[str, Any]]) -> int:
        """Batch insert records. Returns rows affected."""
        return await db.insert_batch(cls.__table__, data)

    @classmethod
    async def select_by_map(
        cls, db: RBatis, condition: Dict[str, Any]
    ) -> List[Dict[str, Any]]:
        """Select records by condition. Returns list of dicts."""
        return await db.select_by_map(cls.__table__, condition)

    @classmethod
    async def update_by_map(
        cls,
        db: RBatis,
        data: Dict[str, Any],
        condition: Dict[str, Any],
    ) -> int:
        """Update records by condition. Returns rows affected."""
        return await db.update_by_map(cls.__table__, data, condition)

    @classmethod
    async def delete_by_map(cls, db: RBatis, condition: Dict[str, Any]) -> int:
        """Delete records by condition. Returns rows affected."""
        return await db.delete_by_map(cls.__table__, condition)
