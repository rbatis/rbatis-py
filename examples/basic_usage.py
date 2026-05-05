"""Example 1: Basic usage — exec, exec_decode, transactions

Corresponds to rbatis Rust API:
    rb.exec(sql, args).await?;
    let rows: Vec<Value> = rb.exec_decode(sql, args).await?;
    let tx = rb.acquire_begin().await?;

Run:
    cd rbatis-py/
    uv run python examples/basic_usage.py
"""

import asyncio
from rbatis_py import RBatis

DB_URL = "sqlite://target/rbatis_example.db"


async def main():
    # ============================================================
    # 1. Connect to database
    # ============================================================
    db = RBatis()
    await db.link(DB_URL)
    print(f"Connected: {db.is_connected()}")

    # Create table
    await db.exec("DROP TABLE IF EXISTS user")
    await db.exec(
        "CREATE TABLE IF NOT EXISTS user ("
        "  id INTEGER PRIMARY KEY AUTOINCREMENT,"
        "  name TEXT NOT NULL,"
        "  age INTEGER"
        ")"
    )

    # ============================================================
    # 2. exec — execute INSERT/UPDATE/DELETE
    #    Rust: rb.exec(sql, args).await?
    # ============================================================
    affected = await db.exec(
        "INSERT INTO user (name, age) VALUES (?, ?)",
        ["Alice", 30],
    )
    print(f"INSERT: {affected} row(s)")

    await db.exec("INSERT INTO user (name, age) VALUES (?, ?)", ["Bob", 25])

    affected = await db.exec(
        "UPDATE user SET age = ? WHERE name = ?",
        [31, "Alice"],
    )
    print(f"UPDATE: {affected} row(s)")

    # ============================================================
    # 3. exec_decode — query returns List[Dict]
    #    Rust: let rows: Vec<Value> = rb.exec_decode(sql, args).await?
    # ============================================================
    rows = await db.exec_decode("SELECT * FROM user")
    print(f"\nSELECT * ({len(rows)} rows):")
    for r in rows:
        print(f"  {r}")

    rows = await db.exec_decode("SELECT * FROM user WHERE age > ?", [20])
    print(f"SELECT age>20 ({len(rows)} rows):")
    for r in rows:
        print(f"  {r}")

    # ============================================================
    # 4. Transactions — two modes
    #    Rust: let tx = rb.acquire_begin().await?;
    # ============================================================

    # --- Mode A: Explicit transaction (manual commit/rollback) ---
    #     Use await db.begin() to get a Transaction,
    #     then call commit() or rollback() yourself
    print("\n--- Explicit Transaction (manual commit) ---")
    tx = await db.begin()
    try:
        await tx.exec("INSERT INTO user (name, age) VALUES (?, ?)", ["Charlie", 28])
        await tx.exec("INSERT INTO user (name, age) VALUES (?, ?)", ["Diana", 22])
        await tx.commit()
        print("Committed")
    except Exception:
        await tx.rollback()
        raise

    # Explicit transaction — manual rollback on error
    print("\n--- Explicit Transaction (manual rollback on error) ---")
    tx = await db.begin()
    try:
        await tx.exec("INSERT INTO user (name, age) VALUES (?, ?)", ["Eve", 26])
        raise RuntimeError("oops")
    except RuntimeError:
        await tx.rollback()
        print("Rolled back (expected)")

    # --- Mode B: Auto transaction (context manager) ---
    #     Use async with db.begin_defer():,
    #     auto commit or rollback
    print("\n--- Auto Transaction (context manager) ---")
    async with db.begin_defer():
        await db.exec("INSERT INTO user (name, age) VALUES (?, ?)", ["Frank", 32])
        await db.exec("INSERT INTO user (name, age) VALUES (?, ?)", ["Grace", 27])
    print("Auto committed")

    # Auto rollback on exception
    try:
        async with db.begin_defer():
            await db.exec("INSERT INTO user (name, age) VALUES (?, ?)", ["Heidi", 29])
            raise RuntimeError("oops2")
    except RuntimeError:
        print("Auto rolled back (expected)")

    rows = await db.exec_decode("SELECT * FROM user")
    print(f"\nFinal rows ({len(rows)}):")
    for r in rows:
        print(f"  {r}")

    # ============================================================
    # 5. acquire — get a connection from the pool
    # ============================================================
    print("\n--- Acquire Connection ---")
    conn = await db.acquire()
    try:
        rows = await conn.exec_decode("SELECT * FROM user WHERE age > ?", [20])
        print(f"Acquired conn, got {len(rows)} rows")

        # Begin a transaction on this connection
        tx = await conn.begin()
        try:
            await tx.exec("INSERT INTO user (name, age) VALUES (?, ?)", ["Ivy", 33])
            await tx.commit()
            print("Connection tx committed")
        except Exception:
            await tx.rollback()
            raise
    finally:
        conn.close()

    # ============================================================
    # 6. ping / close
    # ============================================================
    ok = await db.ping()
    print(f"\nPing: {ok}")
    db.close()
    print(f"Closed: {db.is_connected()}")


if __name__ == "__main__":
    asyncio.run(main())
