"""
Transaction examples for rbatis-py.

Tests explicit commit/rollback, auto_commit guard, and begin_defer.
"""

import asyncio
import os
import sys

from rbatis_py import RBatis

DB_URL = os.environ.get(
    "RBATIS_DB_URL",
    "sqlite://target/tx_example.db",
)


async def setup(db: RBatis):
    await db.exec("CREATE TABLE IF NOT EXISTS tx_test (id INTEGER PRIMARY KEY, val TEXT)")


async def cleanup(db: RBatis):
    await db.exec("DELETE FROM tx_test")


async def test_explicit_commit(db: RBatis):
    """Explicit begin() + commit()"""
    await cleanup(db)
    tx = await db.begin()
    try:
        await tx.exec("INSERT INTO tx_test (id, val) VALUES (?, ?)", [1, "explicit"])
        await tx.commit()
    except Exception:
        await tx.rollback()
        raise
    rows = await db.exec_decode("SELECT id, val FROM tx_test WHERE id = ?", [1])
    assert len(rows) == 1, f"expected 1 row, got {len(rows)}"
    assert rows[0]["val"] == "explicit"
    print("[PASS] test_explicit_commit")


async def test_explicit_rollback(db: RBatis):
    """Explicit begin() + rollback() — no data persisted"""
    await cleanup(db)
    tx = await db.begin()
    await tx.exec("INSERT INTO tx_test (id, val) VALUES (?, ?)", [2, "rolled"])
    await tx.rollback()
    rows = await db.exec_decode("SELECT id, val FROM tx_test WHERE id = ?", [2])
    assert len(rows) == 0, f"expected 0 rows (rolled back), got {len(rows)}"
    print("[PASS] test_explicit_rollback")


async def test_auto_commit_success(db: RBatis):
    """auto_commit() with guard.exec() — no error → commit"""
    await cleanup(db)
    tx = await db.begin()
    async with tx.auto_commit() as g:
        await g.exec("INSERT INTO tx_test (id, val) VALUES (?, ?)", [3, "auto_ok"])
    rows = await db.exec_decode("SELECT id, val FROM tx_test WHERE id = ?", [3])
    assert len(rows) == 1, f"expected 1 row, got {len(rows)}"
    assert rows[0]["val"] == "auto_ok"
    print("[PASS] test_auto_commit_success")


async def test_auto_commit_error(db: RBatis):
    """auto_commit() with guard.exec() — exception → rollback"""
    await cleanup(db)
    tx = await db.begin()
    try:
        async with tx.auto_commit() as g:
            await g.exec("INSERT INTO tx_test (id, val) VALUES (?, ?)", [4, "auto_err"])
            raise ValueError("oops")
    except ValueError:
        pass
    rows = await db.exec_decode("SELECT id, val FROM tx_test WHERE id = ?", [4])
    assert len(rows) == 0, f"expected 0 rows (rolled back), got {len(rows)}"
    print("[PASS] test_auto_commit_error")


async def test_auto_commit_explicit_commit(db: RBatis):
    """auto_commit() with explicit commit inside — guard no-ops"""
    await cleanup(db)
    tx = await db.begin()
    async with tx.auto_commit() as g:
        await g.exec("INSERT INTO tx_test (id, val) VALUES (?, ?)", [5, "explicit_then_auto"])
        await g.commit()  # explicit commit on guard
    rows = await db.exec_decode("SELECT id, val FROM tx_test WHERE id = ?", [5])
    assert len(rows) == 1, f"expected 1 row, got {len(rows)}"
    print("[PASS] test_auto_commit_explicit_commit")


async def test_auto_commit_explicit_rollback(db: RBatis):
    """auto_commit() with explicit rollback inside — guard no-ops"""
    await cleanup(db)
    tx = await db.begin()
    async with tx.auto_commit() as g:
        await g.exec("INSERT INTO tx_test (id, val) VALUES (?, ?)", [6, "rolled_in_block"])
        await g.rollback()  # explicit rollback on guard
    rows = await db.exec_decode("SELECT id, val FROM tx_test WHERE id = ?", [6])
    assert len(rows) == 0, f"expected 0 rows (rolled back), got {len(rows)}"
    print("[PASS] test_auto_commit_explicit_rollback")


async def test_begin_defer_success(db: RBatis):
    """begin_defer() — no error → commit"""
    await cleanup(db)
    async with db.begin_defer() as g:
        await g.exec("INSERT INTO tx_test (id, val) VALUES (?, ?)", [7, "defer_ok"])
    rows = await db.exec_decode("SELECT id, val FROM tx_test WHERE id = ?", [7])
    assert len(rows) == 1, f"expected 1 row, got {len(rows)}"
    assert rows[0]["val"] == "defer_ok"
    print("[PASS] test_begin_defer_success")


async def test_begin_defer_error(db: RBatis):
    """begin_defer() — exception → rollback"""
    await cleanup(db)
    try:
        async with db.begin_defer() as g:
            await g.exec("INSERT INTO tx_test (id, val) VALUES (?, ?)", [8, "defer_err"])
            raise RuntimeError("fail")
    except RuntimeError:
        pass
    rows = await db.exec_decode("SELECT id, val FROM tx_test WHERE id = ?", [8])
    assert len(rows) == 0, f"expected 0 rows (rolled back), got {len(rows)}"
    print("[PASS] test_begin_defer_error")


async def test_conn_auto_commit(db: RBatis):
    """Transaction from a raw Connection with auto_commit()"""
    await cleanup(db)
    conn = await db.acquire()
    try:
        tx = await conn.begin()
        async with tx.auto_commit() as g:
            await g.exec("INSERT INTO tx_test (id, val) VALUES (?, ?)", [9, "conn_tx"])
    finally:
        conn.close()
    rows = await db.exec_decode("SELECT id, val FROM tx_test WHERE id = ?", [9])
    assert len(rows) == 1, f"expected 1 row, got {len(rows)}"
    assert rows[0]["val"] == "conn_tx"
    print("[PASS] test_conn_auto_commit")


async def main():
    db = RBatis()
    await db.link(DB_URL)

    await setup(db)

    tests = [
        test_explicit_commit,
        test_explicit_rollback,
        test_auto_commit_success,
        test_auto_commit_error,
        test_auto_commit_explicit_commit,
        test_auto_commit_explicit_rollback,
        test_begin_defer_success,
        test_begin_defer_error,
        test_conn_auto_commit,
    ]

    passed = 0
    failed = 0
    for test in tests:
        try:
            await test(db)
            passed += 1
        except Exception as e:
            print(f"[FAIL] {test.__name__}: {e}")
            import traceback
            traceback.print_exc()
            failed += 1

    await cleanup(db)
    print(f"\n{'='*40}\n{passed} passed, {failed} failed")
    if failed:
        sys.exit(1)


if __name__ == "__main__":
    asyncio.run(main())
