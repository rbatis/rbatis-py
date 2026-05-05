# Type stubs for rbatis_py._core (generated from Rust via PyO3)
# This file provides type hints for IDE autocompletion.

from typing import Any, Dict, List, Optional


class RBatis:
    """High-performance async ORM client, backed by rbatis (Rust)."""

    def __init__(self) -> None: ...

    # --- Connection ---
    def link(self, url: str) -> Any:
        """Connect to database (auto-detects driver from URL scheme)."""
        ...

    def link_sqlite(self, url: str) -> Any: ...
    def link_mysql(self, url: str) -> Any: ...
    def link_postgres(self, url: str) -> Any: ...
    def link_mssql(self, url: str) -> Any: ...

    def is_connected(self) -> bool:
        """Check if database is connected."""
        ...

    def ping(self) -> Any:
        """Ping database. Returns awaitable[bool]."""
        ...

    def close(self) -> None:
        """Close the connection."""
        ...

    # --- Pool Configuration ---
    def set_pool_max_size(self, max_size: int) -> Any:
        """Set max connections in the pool."""
        ...

    def set_pool_max_idle(self, max_idle: int) -> Any:
        """Set max idle connections."""
        ...

    def set_pool_connect_timeout(self, timeout_secs: int) -> Any:
        """Set connection timeout in seconds."""
        ...

    def set_pool_max_lifetime(self, lifetime_secs: int) -> Any:
        """Set max connection lifetime in seconds."""
        ...

    def pool_state(self) -> Any:
        """Inspect pool state. Returns awaitable[dict]."""
        ...

    # --- Connection / Transaction ---
    def acquire(self) -> Any:
        """Acquire a raw connection from the pool. Returns awaitable[Connection]."""
        ...

    def begin(self) -> Any:
        """Begin an explicit transaction. Returns awaitable[Transaction]."""
        ...

    def begin_defer(self) -> "DeferredTransaction":
        """Begin an auto transaction (context manager)."""
        ...

    def commit(self) -> Any:
        """Commit the current active transaction."""
        ...

    def rollback(self) -> Any:
        """Rollback the current active transaction."""
        ...

    # --- SQL Execution ---
    def exec(self, sql: str, params: Optional[List[Any]] = None) -> Any:
        """Execute INSERT/UPDATE/DELETE. Returns awaitable[int] (rows affected)."""
        ...

    def exec_decode(
        self, sql: str, params: Optional[List[Any]] = None
    ) -> Any:
        """Execute a query. Returns awaitable[List[Dict]]."""
        ...

    # --- CRUD ---
    def insert(self, table: str, data: Dict[str, Any]) -> Any:
        """Insert one row. Returns awaitable[int]."""
        ...

    def insert_batch(self, table: str, data_list: List[Dict[str, Any]]) -> Any:
        """Batch insert. Returns awaitable[int]."""
        ...

    def select_by_map(
        self, table: str, condition: Dict[str, Any]
    ) -> Any:
        """Select by equality condition. Returns awaitable[List[Dict]]."""
        ...

    def update_by_map(
        self,
        table: str,
        data: Dict[str, Any],
        condition: Dict[str, Any],
    ) -> Any:
        """Update by equality condition. Returns awaitable[int]."""
        ...

    def delete_by_map(
        self, table: str, condition: Dict[str, Any]
    ) -> Any:
        """Delete by equality condition. Returns awaitable[int]."""
        ...


class Transaction:
    """Explicit transaction. Requires manual commit() or rollback()."""

    def get_tx_id(self) -> int: ...

    def exec(self, sql: str, params: Optional[List[Any]] = None) -> Any:
        """Execute SQL within this transaction. Returns awaitable[int]."""
        ...

    def exec_decode(
        self, sql: str, params: Optional[List[Any]] = None
    ) -> Any:
        """Query within this transaction. Returns awaitable[List[Dict]]."""
        ...

    def commit(self) -> Any:
        """Commit the transaction."""
        ...

    def rollback(self) -> Any:
        """Rollback the transaction."""
        ...


class DeferredTransaction:
    """Auto transaction (context manager). Auto-commits or auto-rollbacks."""


class Connection:
    """A raw connection acquired from the pool."""

    def exec(self, sql: str, params: Optional[List[Any]] = None) -> Any:
        """Execute SQL on this connection. Returns awaitable[int]."""
        ...

    def exec_decode(
        self, sql: str, params: Optional[List[Any]] = None
    ) -> Any:
        """Query on this connection. Returns awaitable[List[Dict]]."""
        ...

    def begin(self) -> Any:
        """Begin a transaction on this connection. Returns awaitable[Transaction]."""
        ...

    def close(self) -> None:
        """Release this connection back to the pool."""
        ...
