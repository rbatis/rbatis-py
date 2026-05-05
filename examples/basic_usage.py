"""示例1: 基础用法 — exec, exec_decode, 事务

对应 rbatis Rust API:
    rb.exec(sql, args).await?;
    let rows: Vec<Value> = rb.exec_decode(sql, args).await?;
    let tx = rb.acquire_begin().await?;

运行:
    cd rbatis-py/
    uv run python examples/basic_usage.py
"""

import asyncio
from rbatis_py import RBatis

DB_URL = "sqlite://target/rbatis_example.db"


async def main():
    # ============================================================
    # 1. 连接数据库
    # ============================================================
    db = RBatis()
    await db.link(DB_URL)
    print(f"Connected: {db.is_connected()}")

    # 建表
    await db.exec("DROP TABLE IF EXISTS user")
    await db.exec(
        "CREATE TABLE IF NOT EXISTS user ("
        "  id INTEGER PRIMARY KEY AUTOINCREMENT,"
        "  name TEXT NOT NULL,"
        "  age INTEGER"
        ")"
    )

    # ============================================================
    # 2. exec — 执行 INSERT/UPDATE/DELETE
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
    # 3. exec_decode — 查询返回 List[Dict]
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
    # 4. 事务 — 两种模式
    #    Rust: let tx = rb.acquire_begin().await?;
    # ============================================================

    # --- 模式 A: 显式事务 (手动 commit/rollback) ---
    #    用 await db.begin() 获取 Transaction，
    #    然后自己调用 commit() 或 rollback()
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

    # 显式事务 — 出异常时手动 rollback
    print("\n--- Explicit Transaction (manual rollback on error) ---")
    tx = await db.begin()
    try:
        await tx.exec("INSERT INTO user (name, age) VALUES (?, ?)", ["Eve", 26])
        raise RuntimeError("oops")
    except RuntimeError:
        await tx.rollback()
        print("Rolled back (expected)")

    # --- 模式 B: 自动事务 (context manager) ---
    #    用 async with db.begin_defer():,
    #    自动 commit 或 rollback
    print("\n--- Auto Transaction (context manager) ---")
    async with db.begin_defer():
        await db.exec("INSERT INTO user (name, age) VALUES (?, ?)", ["Frank", 32])
        await db.exec("INSERT INTO user (name, age) VALUES (?, ?)", ["Grace", 27])
    print("Auto committed")

    # 异常时自动 rollback
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
    # 5. acquire — 从连接池获取连接对象
    # ============================================================
    print("\n--- Acquire Connection ---")
    conn = await db.acquire()
    try:
        rows = await conn.exec_decode("SELECT * FROM user WHERE age > ?", [20])
        print(f"Acquired conn, got {len(rows)} rows")

        # 在连接上开启事务
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
