"""示例2: CRUD 用法 — 定义表结构体 + 内置 CRUD 函数

对应 Rust rbatis 的 ``crud!`` 宏：

    struct BizActivity { id, name, ... }
    crud!(BizActivity {});

在 Python 中，继承 ``Model`` 并定义 ``__table__`` 即可。

运行:
    cd rbatis-py/
    uv run python examples/crud_usage.py
"""

import asyncio
from rbatis_py import RBatis, Model

DB_URL = "sqlite://target/rbatis_crud.db"


# ============================================================
# 定义表结构体（对应 Rust 的 struct + crud! 宏）
#
# Rust 版:
#   #[derive(Serialize, Deserialize)]
#   struct User { id: Option<i64>, name: Option<String>, ... }
#   crud!(User {});
# ============================================================
class User(Model):
    """用户表"""
    __table__ = "user"
    id: int | None = None
    name: str | None = None
    age: int | None = None


class Article(Model):
    """文章表"""
    __table__ = "article"
    id: int | None = None
    title: str | None = None
    user_id: int | None = None


async def main():
    db = RBatis()
    await db.link(DB_URL)

    # ---------- 建表 ----------
    await db.exec("DROP TABLE IF EXISTS article")
    await db.exec("DROP TABLE IF EXISTS user")
    await db.exec(
        "CREATE TABLE IF NOT EXISTS user ("
        "  id INTEGER PRIMARY KEY AUTOINCREMENT,"
        "  name TEXT NOT NULL,"
        "  age INTEGER"
        ")"
    )
    await db.exec(
        "CREATE TABLE IF NOT EXISTS article ("
        "  id INTEGER PRIMARY KEY AUTOINCREMENT,"
        "  title TEXT NOT NULL,"
        "  user_id INTEGER"
        ")"
    )

    # ============================================================
    # insert — 插入单条
    # ============================================================
    affected = await User.insert(db, {"name": "Alice", "age": 30})
    print(f"User.insert: {affected} row(s)")

    affected = await User.insert(db, {"name": "Bob", "age": 25})
    print(f"User.insert: {affected} row(s)")

    affected = await Article.insert(db, {"title": "Hello Rust", "user_id": 1})
    print(f"Article.insert: {affected} row(s)")

    # ============================================================
    # insert_batch — 批量插入
    # ============================================================
    users = [
        {"name": "Charlie", "age": 35},
        {"name": "David", "age": 28},
        {"name": "Eve", "age": 22},
    ]
    affected = await User.insert_batch(db, users)
    print(f"\nUser.insert_batch ({len(users)} items): {affected} row(s)")

    # ============================================================
    # select_by_map — 条件查询
    # ============================================================
    rows = await User.select_by_map(db, {"name": "Alice"})
    print(f"\nUser.select_by_map(name='Alice'): {rows}")

    rows = await User.select_by_map(db, {"age": 28})
    print(f"User.select_by_map(age=28): {rows}")

    # ============================================================
    # update_by_map — 条件更新
    # ============================================================
    affected = await User.update_by_map(
        db,
        {"age": 31},          # SET
        {"name": "Alice"},    # WHERE
    )
    print(f"\nUser.update_by_map: {affected} row(s)")

    rows = await User.select_by_map(db, {"name": "Alice"})
    print(f"After update: {rows}")

    # ============================================================
    # delete_by_map — 条件删除
    # ============================================================
    affected = await User.delete_by_map(db, {"name": "Eve"})
    print(f"\nUser.delete_by_map: {affected} row(s)")

    rows = await User.select_by_map(db, {"age": 22})
    print(f"Remaining age=22: {rows}")

    # ============================================================
    # 完整 CRUD 组合
    # ============================================================
    print("\n--- Article CRUD ---")

    # 批量插入文章
    await Article.insert_batch(db, [
        {"title": "Rbatis Intro", "user_id": 1},
        {"title": "Async Rust", "user_id": 2},
        {"title": "PyO3 Guide", "user_id": 3},
    ])

    # 查询某用户的文章
    rows = await Article.select_by_map(db, {"user_id": 1})
    print(f"Articles by user 1: {rows}")

    # 更新
    await Article.update_by_map(
        db,
        {"title": "Rbatis Guide"},
        {"title": "Rbatis Intro"},
    )

    # 查询全部
    rows = await db.exec_decode("SELECT * FROM article")
    print(f"All articles ({len(rows)}):")
    for r in rows:
        print(f"  {r}")

    db.close()
    print(f"\nDone. Connected: {db.is_connected()}")


if __name__ == "__main__":
    asyncio.run(main())
