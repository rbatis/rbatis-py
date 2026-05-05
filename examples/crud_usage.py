"""示例2: CRUD 用法 — 定义表结构体 + 内置 CRUD 函数

对应 Rust rbatis 的 ``crud!`` 宏：

    struct BizActivity { id, name, create_time }
    crud!(BizActivity {});

在 Python 中，继承 ``Model`` 并定义 ``__table__`` 即可。

运行:
    cd rbatis-py/
    uv run python examples/crud_usage.py
"""

import asyncio
from datetime import datetime as dt
from rbatis_py import RBatis, Model

DB_URL = "sqlite://target/rbatis_crud.db"


# ============================================================
# 定义表结构体（对应 Rust 的 struct + crud! 宏）
#
# Rust 版:
#   #[derive(Serialize, Deserialize)]
#   struct User { id: Option<i64>, name: Option<String>, age: Option<i32>, create_time: Option<DateTime> }
#   crud!(User {});
#
# Python 版:
#   字段类型标注仅用于提示，rbdc 类型会与 Python 原生类型自动转换:
#     rbdc::DateTime  <->  datetime.datetime
#     rbdc::Date      <->  datetime.date
#     rbdc::Decimal   <->  decimal.Decimal
#     rbdc::Uuid      <->  uuid.UUID
# ============================================================
class User(Model):
    """用户表"""
    __table__ = "user"
    id: int | None = None
    name: str | None = None
    age: int | None = None
    create_time: dt | None = None


async def main():
    db = RBatis()
    await db.link(DB_URL)

    # 建表
    await db.exec("DROP TABLE IF EXISTS user")
    await db.exec(
        "CREATE TABLE IF NOT EXISTS user ("
        "  id INTEGER PRIMARY KEY AUTOINCREMENT,"
        "  name TEXT NOT NULL,"
        "  age INTEGER,"
        "  create_time TEXT"
        ")"
    )

    # ============================================================
    # insert — 插入单条（Python datetime 自动转 rbdc::DateTime）
    # ============================================================
    affected = await User.insert(db, {
        "name": "Alice",
        "age": 30,
        "create_time": dt.now(),
    })
    print(f"User.insert: {affected} row(s)")

    affected = await User.insert(db, {
        "name": "Bob",
        "age": 25,
        "create_time": dt.now(),
    })
    print(f"User.insert: {affected} row(s)")

    # ============================================================
    # insert_batch — 批量插入
    # ============================================================
    users = [
        {"name": "Charlie", "age": 35, "create_time": dt.now()},
        {"name": "David", "age": 28, "create_time": dt.now()},
        {"name": "Eve", "age": 22, "create_time": dt.now()},
    ]
    affected = await User.insert_batch(db, users)
    print(f"\nUser.insert_batch ({len(users)} items): {affected} row(s)")

    # ============================================================
    # select_by_map — 条件查询（datetime 自动转回 Python datetime）
    # ============================================================
    rows = await User.select_by_map(db, {"name": "Alice"})
    print(f"\nUser.select_by_map(name='Alice'):")
    for r in rows:
        print(f"  {r}")
        # create_time 是 Python datetime 对象
        if r.get("create_time"):
            print(f"    create_time type: {type(r['create_time']).__name__}")

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

    rows = await db.exec_decode("SELECT * FROM user")
    print(f"\nAll users ({len(rows)}):")
    for r in rows:
        print(f"  id={r['id']} name={r['name']} age={r['age']} create_time={r.get('create_time')}")

    db.close()
    print(f"\nDone. Connected: {db.is_connected()}")


if __name__ == "__main__":
    asyncio.run(main())
