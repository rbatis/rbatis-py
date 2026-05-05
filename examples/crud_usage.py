"""示例2: CRUD 用法 — 定义表结构体 + 内置 CRUD 函数

对应 Rust rbatis 的 ``crud!`` 宏：

    struct BizActivity { id, name, age, create_time, salary, user_uuid }
    crud!(BizActivity {});

用法:
    uv run python examples/crud_usage.py sqlite
    uv run python examples/crud_usage.py mysql
    uv run python examples/crud_usage.py postgres

类型转换:
    Python datetime.datetime  <->  rbs Ext("DateTime")  <->  rbdc::DateTime
    Python decimal.Decimal    <->  rbs Ext("Decimal")    <->  rbdc::Decimal
    Python uuid.UUID          <->  rbs Ext("Uuid")       <->  rbdc::Uuid
"""

import asyncio
import sys
from datetime import datetime
from decimal import Decimal
from uuid import uuid4
from rbatis_py import RBatis, Model

DB_URLS = {
    "sqlite": "sqlite://target/rbatis_crud.db",
    "mysql": "mysql://root:123456@localhost:3306/test",
    "postgres": "postgres://postgres:123456@localhost:5432/postgres",
}

DEFAULT_DB = "sqlite"


class AppUser(Model):
    """用户表"""
    __table__ = "app_user"
    id: int | None = None
    name: str | None = None
    age: int | None = None
    create_time: datetime | None = None       # rbdc::DateTime
    salary: Decimal | None = None              # rbdc::Decimal
    user_uuid: str | None = None               # rbdc::Uuid


def create_table_sql(db_type: str) -> str:
    if db_type == "mysql":
        return (
            "CREATE TABLE IF NOT EXISTS app_user ("
            "  id INTEGER AUTO_INCREMENT PRIMARY KEY,"
            "  name VARCHAR(255) NOT NULL,"
            "  age INTEGER,"
            "  create_time DATETIME,"
            "  salary DECIMAL(10,2),"
            "  user_uuid VARCHAR(36)"
            ")"
        )
    elif db_type == "postgres":
        return (
            "CREATE TABLE IF NOT EXISTS app_user ("
            "  id SERIAL PRIMARY KEY,"
            "  name VARCHAR(255) NOT NULL,"
            "  age INTEGER,"
            "  create_time TIMESTAMP,"
            "  salary DECIMAL(10,2),"
            "  user_uuid VARCHAR(36)"
            ")"
        )
    else:
        return (
            "CREATE TABLE IF NOT EXISTS app_user ("
            "  id INTEGER PRIMARY KEY AUTOINCREMENT,"
            "  name TEXT NOT NULL,"
            "  age INTEGER,"
            "  create_time TEXT,"
            "  salary TEXT,"
            "  user_uuid TEXT"
            ")"
        )


async def main():
    db_type = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_DB
    url = DB_URLS.get(db_type)
    if not url:
        print(f"Usage: {sys.argv[0]} [sqlite|mysql|postgres]")
        sys.exit(1)

    db = RBatis()
    await db.link(url)
    print(f"[{db_type}] Connected: {db.is_connected()}")

    # 建表
    await db.exec("DROP TABLE IF EXISTS app_user")
    await db.exec(create_table_sql(db_type))

    # ============================================================
    # insert — 插入各类型数据
    # ============================================================
    now = datetime.now()
    uid = str(uuid4())
    affected = await AppUser.insert(db, {
        "name": "Alice",
        "age": 30,
        "create_time": now,
        "salary": Decimal("12345.67"),
        "user_uuid": uid,
    })
    print(f"\nInsert: {affected} row(s)")
    print(f"  create_time: {now} ({type(now).__name__})")
    print(f"  salary:      {Decimal('12345.67')} ({type(Decimal('12345.67')).__name__})")
    print(f"  user_uuid:   {uid} ({type(uid).__name__})")

    # ============================================================
    # select_by_map — 查询并检查类型
    # ============================================================
    rows = await AppUser.select_by_map(db, {"name": "Alice"})
    print(f"\nselect_by_map(name='Alice'):")
    for r in rows:
        for k, v in r.items():
            print(f"  {k}: {v!r}  ({type(v).__name__})")

    # ============================================================
    # insert_batch
    # ============================================================
    users = [
        {"name": "Bob", "age": 25, "create_time": now, "salary": Decimal("999.99"), "user_uuid": str(uuid4())},
        {"name": "Charlie", "age": 35, "create_time": now, "salary": Decimal("50000"), "user_uuid": str(uuid4())},
    ]
    affected = await AppUser.insert_batch(db, users)
    print(f"\ninsert_batch: {affected} row(s)")

    # ============================================================
    # exec_decode — 原生 SQL 查询
    # ============================================================
    rows = await db.exec_decode("SELECT * FROM user")
    print(f"\nexec_decode ({len(rows)} rows):")
    for r in rows:
        for k, v in r.items():
            print(f"  {k}: {v!r}  ({type(v).__name__})")
        print()

    # ============================================================
    # update_by_map / delete_by_map
    # ============================================================
    affected = await AppUser.update_by_map(db, {"age": 31}, {"name": "Alice"})
    print(f"update_by_map: {affected} row(s)")

    affected = await AppUser.delete_by_map(db, {"name": "Bob"})
    print(f"delete_by_map: {affected} row(s)")

    db.close()
    print(f"\nDone. Connected: {db.is_connected()}")


if __name__ == "__main__":
    asyncio.run(main())
