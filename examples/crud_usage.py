"""示例2: CRUD 用法 — 定义表结构体 + 内置 CRUD 函数

对应 Rust rbatis 的 ``crud!`` 宏：

    struct BizActivity { id, name, age, create_time }
    crud!(BizActivity {});

类型转换（py_to_rbs / rbs_to_py 自动处理）:
    Python datetime.datetime  <->  rbs Value::Ext("DateTime", ...)  <->  rbdc::DateTime
    Python datetime.date      <->  rbs Value::Ext("Date", ...)      <->  rbdc::Date
    Python decimal.Decimal    <->  rbs Value::Ext("Decimal", ...)   <->  rbdc::Decimal
    Python uuid.UUID          <->  rbs Value::Ext("Uuid", ...)      <->  rbdc::Uuid

在 Python 中，继承 ``Model`` 并定义 ``__table__`` 即可。

运行:
    cd rbatis-py/
    uv run python examples/crud_usage.py
"""

import asyncio
from datetime import datetime
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
# Python 类型         rbs 序列化             数据库类型
#   datetime.datetime  -->  Ext("DateTime", ...)  -->  rbdc::DateTime
#   datetime.date      -->  Ext("Date", ...)       -->  rbdc::Date
#   decimal.Decimal    -->  Ext("Decimal", ...)    -->  rbdc::Decimal
#   uuid.UUID          -->  Ext("Uuid", ...)       -->  rbdc::Uuid
# ============================================================
class User(Model):
    """用户表"""
    __table__ = "user"
    id: int | None = None
    name: str | None = None
    age: int | None = None
    # Python datetime.datetime  <->  rbs Ext("DateTime")  <->  rbdc::DateTime
    create_time: datetime | None = None


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
    # insert — 插入单条（Python datetime -> rbs Ext("DateTime") -> rbdc::DateTime）
    # ============================================================
    now = datetime.now()
    affected = await User.insert(db, {"name": "Alice", "age": 30, "create_time": now})
    print(f"User.insert: {affected} row(s)")

    affected = await User.insert(db, {"name": "Bob", "age": 25, "create_time": now})
    print(f"User.insert: {affected} row(s)")

    # ============================================================
    # insert_batch — 批量插入
    # ============================================================
    users = [
        {"name": "Charlie", "age": 35, "create_time": now},
        {"name": "David", "age": 28, "create_time": now},
        {"name": "Eve", "age": 22, "create_time": now},
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

    rows = await db.exec_decode("SELECT * FROM user")
    print(f"\nAll users ({len(rows)}):")
    for r in rows:
        print(f"  {r}")

    db.close()
    print(f"\nDone. Connected: {db.is_connected()}")


if __name__ == "__main__":
    asyncio.run(main())
