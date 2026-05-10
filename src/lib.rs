use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use rbs::value::map::ValueMap;
use rbs::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::Mutex as AsyncMutex;

use rbdc::db::Connection;
use rbdc::pool::{ConnectionManager, Pool};
use rbdc_pool_fast::FastPool;

// Helper: iterate over a ValueMap entries
fn value_map_iter(map: &rbs::value::map::ValueMap) -> impl Iterator<Item = (&Value, &Value)> {
    map.0.iter()
}

// ============================================================
//  Type Conversion: Python <-> rbs::Value
// ============================================================

fn py_to_rbs(value: &Bound<'_, PyAny>) -> PyResult<Value> {
    if value.is_none() {
        return Ok(Value::Null);
    }

    let py = value.py();
    if let Ok(datetime_mod) = py.import_bound("datetime") {
        if let Ok(dt_class) = datetime_mod.getattr("datetime") {
            if value.is_instance(&dt_class).unwrap_or(false) {
                let s = value.call_method0("isoformat")?.extract::<String>()?;
                return Ok(Value::Ext("DateTime", Box::new(Value::String(s))));
            }
        }
        if let Ok(d_class) = datetime_mod.getattr("date") {
            if value.is_instance(&d_class).unwrap_or(false) {
                let s = value.call_method0("isoformat")?.extract::<String>()?;
                return Ok(Value::Ext("Date", Box::new(Value::String(s))));
            }
        }
        if let Ok(t_class) = datetime_mod.getattr("time") {
            if value.is_instance(&t_class).unwrap_or(false) {
                let s = value.call_method0("isoformat")?.extract::<String>()?;
                return Ok(Value::Ext("Time", Box::new(Value::String(s))));
            }
        }
    }

    if let Ok(decimal_mod) = py.import_bound("decimal") {
        if let Ok(d_class) = decimal_mod.getattr("Decimal") {
            if value.is_instance(&d_class).unwrap_or(false) {
                let s = value.call_method0("__str__")?.extract::<String>()?;
                return Ok(Value::Ext("Decimal", Box::new(Value::String(s))));
            }
        }
    }

    if let Ok(uuid_mod) = py.import_bound("uuid") {
        if let Ok(u_class) = uuid_mod.getattr("UUID") {
            if value.is_instance(&u_class).unwrap_or(false) {
                let s = value.call_method0("__str__")?.extract::<String>()?;
                return Ok(Value::Ext("Uuid", Box::new(Value::String(s))));
            }
        }
    }

    if let Ok(b) = value.extract::<bool>() {
        return Ok(Value::Bool(b));
    }
    if let Ok(i) = value.extract::<i64>() {
        return Ok(Value::I64(i));
    }
    if let Ok(f) = value.extract::<f64>() {
        return Ok(Value::F64(f));
    }
    if let Ok(s) = value.extract::<String>() {
        return Ok(Value::String(s));
    }
    if let Ok(list) = value.downcast::<PyList>() {
        let mut arr = Vec::with_capacity(list.len());
        for item in list.iter() {
            arr.push(py_to_rbs(&item)?);
        }
        return Ok(Value::Array(arr));
    }
    if let Ok(d) = value.downcast::<PyDict>() {
        let mut map = ValueMap::new();
        for (k, v) in d.iter() {
            let key = k.extract::<String>().unwrap_or_default();
            map.insert(Value::String(key), py_to_rbs(&v)?);
        }
        return Ok(Value::Map(map));
    }
    let s = value.str()?.to_string_lossy().to_string();
    Ok(Value::String(s))
}

fn rbs_to_py<'py>(value: &Value, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    match value {
        Value::Null => Ok(py.None().into_bound(py)),
        Value::Bool(b) => Ok(b.into_py(py).into_bound(py)),
        Value::I32(i) => Ok(i.into_py(py).into_bound(py)),
        Value::I64(i) => Ok(i.into_py(py).into_bound(py)),
        Value::U32(u) => Ok(u.into_py(py).into_bound(py)),
        Value::U64(u) => Ok(u.into_py(py).into_bound(py)),
        Value::F32(f) => Ok(f.into_py(py).into_bound(py)),
        Value::F64(f) => Ok(f.into_py(py).into_bound(py)),
        Value::String(s) => Ok(s.into_py(py).into_bound(py)),
        Value::Binary(b) => Ok(b.clone().into_py(py).into_bound(py)),
        Value::Array(arr) => {
            let py_list = PyList::empty_bound(py);
            for item in arr {
                py_list.append(rbs_to_py(item, py)?)?;
            }
            Ok(py_list.into_any())
        }
        Value::Map(map) => {
            let py_dict = PyDict::new_bound(py);
            for (k, v) in value_map_iter(map) {
                let key_str = match k {
                    Value::String(s) => s.clone(),
                    other => format!("{:?}", other),
                };
                py_dict.set_item(key_str, rbs_to_py(v, py)?)?;
            }
            Ok(py_dict.into_any())
        }
        Value::Ext("DateTime", inner) => {
            if let Value::String(s) = inner.as_ref() {
                let dt_cls = py.import_bound("datetime")?.getattr("datetime")?;
                let r = dt_cls.call_method1("fromisoformat", (s.clone(),))?;
                Ok(r.into_any())
            } else {
                rbs_to_py(inner.as_ref(), py)
            }
        }
        Value::Ext("Date", inner) => {
            if let Value::String(s) = inner.as_ref() {
                let parts: Vec<&str> = s.split('-').collect();
                if parts.len() >= 3 {
                    let y = parts[0].parse().unwrap_or(0);
                    let m: u32 = parts[1].parse().unwrap_or(1);
                    let d: u32 = parts[2].parse().unwrap_or(1);
                    let dt = py.import_bound("datetime")?;
                    let r = dt.call_method1("date", (y, m, d))?;
                    Ok(r.into_any())
                } else {
                    Ok(s.into_py(py).into_bound(py))
                }
            } else {
                rbs_to_py(inner.as_ref(), py)
            }
        }
        Value::Ext("Time", inner) => {
            if let Value::String(s) = inner.as_ref() {
                let t_cls = py.import_bound("datetime")?.getattr("time")?;
                let r = t_cls.call_method1("fromisoformat", (s.clone(),))?;
                Ok(r.into_any())
            } else {
                rbs_to_py(inner.as_ref(), py)
            }
        }
        Value::Ext("Timestamp", inner) => match inner.as_ref() {
            Value::I64(ts) => {
                let dt_cls = py.import_bound("datetime")?.getattr("datetime")?;
                let r = dt_cls.call_method1("fromtimestamp", (*ts as f64 / 1000.0,))?;
                Ok(r.into_any())
            }
            _ => rbs_to_py(inner.as_ref(), py),
        },
        Value::Ext("Decimal", inner) => {
            if let Value::String(s) = inner.as_ref() {
                let dec_cls = py.import_bound("decimal")?.getattr("Decimal")?;
                let r = dec_cls.call1((s.clone(),))?;
                Ok(r.into_any())
            } else {
                rbs_to_py(inner.as_ref(), py)
            }
        }
        Value::Ext("Uuid", inner) => {
            if let Value::String(s) = inner.as_ref() {
                let uuid_cls = py.import_bound("uuid")?.getattr("UUID")?;
                let r = uuid_cls.call1((s.clone(),))?;
                Ok(r.into_any())
            } else {
                rbs_to_py(inner.as_ref(), py)
            }
        }
        Value::Ext("Json", inner) => {
            if let Value::String(s) = inner.as_ref() {
                let json_mod = py.import_bound("json")?;
                let r = json_mod.call_method1("loads", (s.clone(),))?;
                Ok(r.into_any())
            } else {
                rbs_to_py(inner.as_ref(), py)
            }
        }
        Value::Ext(_tag, inner) => rbs_to_py(inner.as_ref(), py),
    }
}

fn collect_params(params: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<Value>> {
    match params {
        None => Ok(Vec::new()),
        Some(p) => {
            if let Ok(list) = p.downcast::<PyList>() {
                let mut result = Vec::with_capacity(list.len());
                for item in list.iter() {
                    result.push(py_to_rbs(&item)?);
                }
                Ok(result)
            } else {
                Ok(vec![py_to_rbs(p)?])
            }
        }
    }
}

fn py_dict_to_columns_values(dict: &Bound<'_, PyDict>) -> PyResult<(Vec<String>, Vec<Value>)> {
    let mut columns = Vec::with_capacity(dict.len());
    let mut values = Vec::with_capacity(dict.len());
    for (k, v) in dict.iter() {
        let col = k.extract::<String>()?;
        values.push(py_to_rbs(&v)?);
        columns.push(col);
    }
    Ok((columns, values))
}

/// Try to convert a raw database value into an rbs Ext type.
/// This mimics what rbatis's serde deserialization does when decoding
/// into a typed struct (e.g. BizActivity with DateTime/Decimal/Uuid fields).
///
/// Without this, `exec_decode` skips serde and returns raw
/// Value::String/Value::I64, losing type info.
fn raw_to_ext(v: &Value) -> Value {
    match v {
        // MySQL returns TEXT columns as Value::Binary
        Value::Binary(bytes) => {
            if let Ok(s) = String::from_utf8(bytes.clone()) {
                let trimmed = s.trim_start();
                if trimmed.starts_with('{') || trimmed.starts_with('[') {
                    if serde_json::from_str::<serde_json::Value>(&s).is_ok() {
                        return Value::Ext("Json", Box::new(Value::String(s)));
                    }
                }
            }
            v.clone()
        }
        Value::String(s) => {
            // Quick check: if it looks like JSON object/array, try JSON first
            let trimmed = s.trim_start();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                if serde_json::from_str::<serde_json::Value>(s).is_ok() {
                    return Value::Ext("Json", Box::new(Value::String(s.clone())));
                }
            }
            // Try each rbdc type in order. Deserialize impls handle Value::String.
            if let Ok(dt) = rbs::from_value::<rbdc::types::datetime::DateTime>(v.clone()) {
                return Value::Ext("DateTime", Box::new(Value::String(dt.to_string())));
            }
            if let Ok(d) = rbs::from_value::<rbdc::types::date::Date>(v.clone()) {
                return Value::Ext("Date", Box::new(Value::String(d.to_string())));
            }
            if let Ok(t) = rbs::from_value::<rbdc::types::time::Time>(v.clone()) {
                return Value::Ext("Time", Box::new(Value::String(t.to_string())));
            }
            if let Ok(d) = rbs::from_value::<rbdc::types::decimal::Decimal>(v.clone()) {
                return Value::Ext("Decimal", Box::new(Value::String(d.to_string())));
            }
            if let Ok(u) = rbs::from_value::<rbdc::types::uuid::Uuid>(v.clone()) {
                // rbdc::Uuid::deserialize accepts ANY string without validation,
                // so we must verify the format before wrapping.
                if u.0.len() == 36 {
                    let bytes = u.0.as_bytes();
                    if bytes[8] == b'-' && bytes[13] == b'-' && bytes[18] == b'-' && bytes[23] == b'-'
                        && bytes.iter().all(|&c| c == b'-' || c.is_ascii_hexdigit())
                    {
                        return Value::Ext("Uuid", Box::new(Value::String(u.0)));
                    }
                }
            }
            v.clone()
        }
        _ => v.clone(),
    }
}

fn value_vec_to_pylist<'py>(vec: &[Value], py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
    let list = PyList::empty_bound(py);
    for row in vec {
        if let Value::Map(ref m) = row {
            let d = PyDict::new_bound(py);
            for (k, v) in value_map_iter(m) {
                let ks = match k {
                    Value::String(s) => s.clone(),
                    o => format!("{:?}", o),
                };
                let converted = raw_to_ext(v);
                d.set_item(ks, rbs_to_py(&converted, py)?)?;
            }
            list.append(d.into_any())?;
        } else {
            let converted = raw_to_ext(row);
            list.append(rbs_to_py(&converted, py)?)?;
        }
    }
    Ok(list)
}

/// Run async work on tokio, bridge result to Python via concurrent.futures + asyncio.wrap_future.
fn spawn_async<'py, F, R>(
    py: Python<'py>,
    handle: &tokio::runtime::Handle,
    work: F,
) -> PyResult<Py<PyAny>>
where
    F: Future<Output = PyResult<R>> + Send + 'static,
    R: IntoPy<Py<PyAny>> + Send + 'static,
{
    let cf_mod = py.import_bound("concurrent.futures")?;
    let cf: Bound<'_, PyAny> = cf_mod.call_method0("Future")?;

    let cf_clone: Py<PyAny> = cf.clone().unbind();

    handle.spawn(async move {
        let result = work.await;
        Python::with_gil(|py| {
            let cf = cf_clone.bind(py);
            match result {
                Ok(val) => {
                    let _ = cf.call_method1("set_result", (val,));
                }
                Err(err) => {
                    let exc = err.into_py(py);
                    let _ = cf.call_method1("set_exception", (exc,));
                }
            }
        });
    });

    let asyncio = py.import_bound("asyncio")?;
    let awaitable = asyncio.call_method1("wrap_future", (cf,))?;
    Ok(awaitable.unbind())
}

/// Execute SQL on a connection, return rows affected.
async fn exec_on_conn(
    conn: &mut Box<dyn Connection>,
    sql: &str,
    args: Vec<Value>,
) -> Result<i64, PyErr> {
    let r = conn
        .exec(sql, args)
        .await
        .map_err(|e| PyRuntimeError::new_err(format!("SQL exec failed: {}", e)))?;
    Ok(r.rows_affected as i64)
}

/// Execute query on a connection, return Vec<Value> (row maps).
async fn query_on_conn(
    conn: &mut Box<dyn Connection>,
    sql: &str,
    args: Vec<Value>,
) -> Result<Vec<Value>, PyErr> {
    let v = conn
        .exec_decode(sql, args)
        .await
        .map_err(|e| PyRuntimeError::new_err(format!("SQL query failed: {}", e)))?;
    match v {
        Value::Array(rows) => Ok(rows),
        other => Ok(vec![other]),
    }
}

/// Get pool from OnceLock, return &dyn Pool or PyErr.
fn get_pool(pool: &OnceLock<Box<dyn Pool>>) -> Result<&dyn Pool, PyErr> {
    pool.get()
        .map(|p| p.as_ref())
        .ok_or_else(|| PyRuntimeError::new_err("Pool not initialized"))
}

/// Acquire a connection from the pool.
async fn acquire_conn(pool: &dyn Pool) -> Result<Box<dyn Connection>, PyErr> {
    pool.get()
        .await
        .map_err(|e| PyRuntimeError::new_err(format!("Acquire connection failed: {}", e)))
}

// ============================================================
//  RBatis Python Class
// ============================================================

#[pyclass(name = "RBatis")]
pub struct RbatisPy {
    pool: Arc<OnceLock<Box<dyn Pool>>>,
    runtime: Runtime,
    connected: Arc<AtomicBool>,
    /// Active transaction connection (if any).
    /// Inner tokio::sync::Mutex allows async locking for SQL operations.
    tx_conn: Arc<Mutex<Option<Arc<AsyncMutex<Box<dyn Connection>>>>>>,
}

#[pymethods]
impl RbatisPy {
    #[new]
    pub fn new() -> PyResult<Self> {
        let runtime =
            Runtime::new().map_err(|e| PyRuntimeError::new_err(format!("Runtime: {}", e)))?;
        Ok(RbatisPy {
            pool: Arc::new(OnceLock::new()),
            runtime,
            connected: Arc::new(AtomicBool::new(false)),
            tx_conn: Arc::new(Mutex::new(None)),
        })
    }

    // ---------- Connection ----------

    fn link_sqlite<'py>(&self, py: Python<'py>, url: &str) -> PyResult<Py<PyAny>> {
        let pool = self.pool.clone();
        let connected = self.connected.clone();
        let url = url.to_string();
        spawn_async(py, &self.runtime.handle(), async move {
            let manager = ConnectionManager::new(rbdc_sqlite::driver::SqliteDriver {}, &url)
                .map_err(|e| PyRuntimeError::new_err(format!("SQLite init error: {}", e)))?;
            let fast_pool =
                FastPool::new(manager)
                    .map_err(|e| PyRuntimeError::new_err(format!(
                        "SQLite pool create failed: {}",
                        e
                    )))?;
            // Test connection
            let _conn = acquire_conn(&fast_pool).await?;
            pool.set(Box::new(fast_pool))
                .map_err(|_| PyRuntimeError::new_err("Pool already initialized"))?;
            connected.store(true, Ordering::Relaxed);
            Ok(())
        })
    }

    fn link_mysql<'py>(&self, py: Python<'py>, url: &str) -> PyResult<Py<PyAny>> {
        let pool = self.pool.clone();
        let connected = self.connected.clone();
        let url = url.to_string();
        spawn_async(py, &self.runtime.handle(), async move {
            let manager = ConnectionManager::new(rbdc_mysql::driver::MysqlDriver {}, &url)
                .map_err(|e| PyRuntimeError::new_err(format!("MySQL init error: {}", e)))?;
            let fast_pool = FastPool::new(manager).map_err(|e| {
                PyRuntimeError::new_err(format!("MySQL pool create failed: {}", e))
            })?;
            let _conn = acquire_conn(&fast_pool).await?;
            pool.set(Box::new(fast_pool))
                .map_err(|_| PyRuntimeError::new_err("Pool already initialized"))?;
            connected.store(true, Ordering::Relaxed);
            Ok(())
        })
    }

    fn link_postgres<'py>(&self, py: Python<'py>, url: &str) -> PyResult<Py<PyAny>> {
        let pool = self.pool.clone();
        let connected = self.connected.clone();
        let url = url.to_string();
        spawn_async(py, &self.runtime.handle(), async move {
            let manager = ConnectionManager::new(rbdc_pg::driver::PgDriver {}, &url)
                .map_err(|e| PyRuntimeError::new_err(format!("PostgreSQL init error: {}", e)))?;
            let fast_pool = FastPool::new(manager).map_err(|e| {
                PyRuntimeError::new_err(format!("PostgreSQL pool create failed: {}", e))
            })?;
            let _conn = acquire_conn(&fast_pool).await?;
            pool.set(Box::new(fast_pool))
                .map_err(|_| PyRuntimeError::new_err("Pool already initialized"))?;
            connected.store(true, Ordering::Relaxed);
            Ok(())
        })
    }

    fn link_mssql<'py>(&self, py: Python<'py>, url: &str) -> PyResult<Py<PyAny>> {
        let pool = self.pool.clone();
        let connected = self.connected.clone();
        let url = url.to_string();
        spawn_async(py, &self.runtime.handle(), async move {
            let manager = ConnectionManager::new(rbdc_mssql::driver::MssqlDriver {}, &url)
                .map_err(|e| PyRuntimeError::new_err(format!("MSSQL init error: {}", e)))?;
            let fast_pool = FastPool::new(manager).map_err(|e| {
                PyRuntimeError::new_err(format!("MSSQL pool create failed: {}", e))
            })?;
            let _conn = acquire_conn(&fast_pool).await?;
            pool.set(Box::new(fast_pool))
                .map_err(|_| PyRuntimeError::new_err("Pool already initialized"))?;
            connected.store(true, Ordering::Relaxed);
            Ok(())
        })
    }

    fn link_turso<'py>(&self, py: Python<'py>, url: &str) -> PyResult<Py<PyAny>> {
        let pool = self.pool.clone();
        let connected = self.connected.clone();
        let url = url.to_string();
        spawn_async(py, &self.runtime.handle(), async move {
            let manager = ConnectionManager::new(rbdc_turso::TursoDriver {}, &url)
                .map_err(|e| PyRuntimeError::new_err(format!("Turso init error: {}", e)))?;
            let fast_pool = FastPool::new(manager).map_err(|e| {
                PyRuntimeError::new_err(format!("Turso pool create failed: {}", e))
            })?;
            let _conn = acquire_conn(&fast_pool).await?;
            pool.set(Box::new(fast_pool))
                .map_err(|_| PyRuntimeError::new_err("Pool already initialized"))?;
            connected.store(true, Ordering::Relaxed);
            Ok(())
        })
    }

    fn link_duckdb<'py>(&self, py: Python<'py>, url: &str) -> PyResult<Py<PyAny>> {
        let pool = self.pool.clone();
        let connected = self.connected.clone();
        let url = url.to_string();
        spawn_async(py, &self.runtime.handle(), async move {
            let manager = ConnectionManager::new(rbdc_duckdb::DuckDbDriver {}, &url)
                .map_err(|e| PyRuntimeError::new_err(format!("DuckDB init error: {}", e)))?;
            let fast_pool = FastPool::new(manager).map_err(|e| {
                PyRuntimeError::new_err(format!("DuckDB pool create failed: {}", e))
            })?;
            let _conn = acquire_conn(&fast_pool).await?;
            pool.set(Box::new(fast_pool))
                .map_err(|_| PyRuntimeError::new_err("Pool already initialized"))?;
            connected.store(true, Ordering::Relaxed);
            Ok(())
        })
    }

    pub fn link<'py>(&self, py: Python<'py>, url: &str) -> PyResult<Py<PyAny>> {
        if url.starts_with("sqlite://") {
            self.link_sqlite(py, url)
        } else if url.starts_with("mysql://") {
            self.link_mysql(py, url)
        } else if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            self.link_postgres(py, url)
        } else if url.starts_with("jdbc:sqlserver://") || url.starts_with("mssql://") {
            self.link_mssql(py, url)
        } else if url.starts_with("turso://") {
            self.link_turso(py, url)
        } else if url.starts_with("duckdb://") {
            self.link_duckdb(py, url)
        } else {
            Err(PyValueError::new_err(format!(
                "Unsupported URL scheme: {}. Supported: sqlite://, mysql://, postgres://, jdbc:sqlserver://, turso://, duckdb://",
                url
            )))
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    // ---------- Connection Pool Configuration ----------

    pub fn set_pool_max_size<'py>(&self, py: Python<'py>, max_size: u64) -> PyResult<Py<PyAny>> {
        let pool = self.pool.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let p = get_pool(&pool)?;
            p.set_max_open_conns(max_size).await;
            Ok(())
        })
    }

    pub fn set_pool_max_idle<'py>(&self, py: Python<'py>, max_idle: u64) -> PyResult<Py<PyAny>> {
        let pool = self.pool.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let p = get_pool(&pool)?;
            p.set_max_idle_conns(max_idle).await;
            Ok(())
        })
    }

    pub fn set_pool_connect_timeout<'py>(
        &self,
        py: Python<'py>,
        timeout_secs: u64,
    ) -> PyResult<Py<PyAny>> {
        let pool = self.pool.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let p = get_pool(&pool)?;
            p.set_timeout(Some(Duration::from_secs(timeout_secs))).await;
            Ok(())
        })
    }

    pub fn set_pool_max_lifetime<'py>(
        &self,
        py: Python<'py>,
        lifetime_secs: u64,
    ) -> PyResult<Py<PyAny>> {
        let pool = self.pool.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let p = get_pool(&pool)?;
            p.set_conn_max_lifetime(Some(Duration::from_secs(lifetime_secs)))
                .await;
            Ok(())
        })
    }

    pub fn pool_state<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let pool = self.pool.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let p = get_pool(&pool)?;
            let state = p.state().await;
            Ok(Python::with_gil(|py| {
                rbs_to_py(&state, py).map(|v| v.unbind().into_any())
            })?)
        })
    }

    pub fn ping<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let pool = self.pool.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let p = get_pool(&pool)?;
            let mut conn = acquire_conn(p).await?;
            conn.ping()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Ping failed: {}", e)))?;
            Ok(true)
        })
    }

    pub fn close(&self) {
        self.connected.store(false, Ordering::Relaxed);
    }

    // ---------- SQL Execution ----------

    #[pyo3(signature = (sql, params=None))]
    pub fn exec<'py>(
        &self,
        py: Python<'py>,
        sql: &str,
        params: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let args = collect_params(params)?;
        let sql = sql.to_string();
        let pool = self.pool.clone();
        let tx_arc = self.tx_conn.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            // Active transaction: use its connection
            let tx_conn_opt = tx_arc
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?
                .clone();
            if let Some(conn_arc) = tx_conn_opt {
                let mut conn = conn_arc.lock().await;
                return exec_on_conn(&mut *conn, &sql, args).await;
            }
            let p = get_pool(&pool)?;
            let mut conn = acquire_conn(p).await?;
            exec_on_conn(&mut conn, &sql, args).await
        })
    }

    #[pyo3(signature = (sql, params=None))]
    pub fn exec_decode<'py>(
        &self,
        py: Python<'py>,
        sql: &str,
        params: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let args = collect_params(params)?;
        let sql = sql.to_string();
        let pool = self.pool.clone();
        let tx_arc = self.tx_conn.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let tx_conn_opt = tx_arc
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?
                .clone();
            let rows = if let Some(conn_arc) = tx_conn_opt {
                let mut conn = conn_arc.lock().await;
                query_on_conn(&mut *conn, &sql, args).await?
            } else {
                let p = get_pool(&pool)?;
                let mut conn = acquire_conn(p).await?;
                query_on_conn(&mut conn, &sql, args).await?
            };
            Python::with_gil(|py| {
                value_vec_to_pylist(&rows, py).map(|l| l.unbind().into_any())
            })
        })
    }

    // ---------- CRUD Operations ----------

    #[pyo3(signature = (table, data))]
    pub fn insert<'py>(
        &self,
        py: Python<'py>,
        table: &str,
        data: &Bound<'_, PyDict>,
    ) -> PyResult<Py<PyAny>> {
        let (columns, values) = py_dict_to_columns_values(data)?;
        if columns.is_empty() {
            return Err(PyValueError::new_err("insert: empty data dict"));
        }
        let cols = columns.join(",");
        let ph = columns.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("INSERT INTO {} ({}) VALUES ({})", table, cols, ph);
        let pool = self.pool.clone();
        let tx_arc = self.tx_conn.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let tx_conn_opt = tx_arc
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?
                .clone();
            if let Some(conn_arc) = tx_conn_opt {
                let mut conn = conn_arc.lock().await;
                return exec_on_conn(&mut *conn, &sql, values).await;
            }
            let p = get_pool(&pool)?;
            let mut conn = acquire_conn(p).await?;
            exec_on_conn(&mut conn, &sql, values).await
        })
    }

    #[pyo3(signature = (table, data, batch_size=None))]
    pub fn insert_batch<'py>(
        &self,
        py: Python<'py>,
        table: &str,
        data: &Bound<'_, PyList>,
        batch_size: Option<usize>,
    ) -> PyResult<Py<PyAny>> {
        if data.is_empty() {
            return spawn_async(py, &self.runtime.handle(), async move { Ok(0i64) });
        }

        let mut rows: Vec<(Vec<String>, Vec<Value>)> = Vec::new();
        for item in data.iter() {
            if let Ok(d) = item.downcast::<PyDict>() {
                let (c, v) = py_dict_to_columns_values(&d)?;
                if !c.is_empty() {
                    rows.push((c, v));
                }
            }
        }
        if rows.is_empty() {
            return spawn_async(py, &self.runtime.handle(), async move { Ok(0i64) });
        }

        let mut columns: Vec<String> = Vec::new();
        for (cols, _) in &rows {
            for c in cols {
                if !columns.contains(c) {
                    columns.push(c.clone());
                }
            }
        }
        let cols_str = columns.join(",");

        let chunk_size = batch_size.unwrap_or(rows.len());
        let table = table.to_string();
        let pool = self.pool.clone();
        let tx_arc = self.tx_conn.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let tx_conn_opt = tx_arc
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?
                .clone();

            let mut total = 0i64;
            for chunk in rows.chunks(chunk_size) {
                let mut all_vals = Vec::new();
                let mut groups = Vec::new();
                for (cols, vals) in chunk {
                    let mut rv = Vec::new();
                    for c in &columns {
                        if let Some(p) = cols.iter().position(|x| x == c) {
                            all_vals.push(vals[p].clone());
                        } else {
                            all_vals.push(Value::Null);
                        }
                        rv.push("?".to_string());
                    }
                    groups.push(format!("({})", rv.join(",")));
                }

                let sql = format!(
                    "INSERT INTO {} ({}) VALUES {}",
                    table,
                    cols_str,
                    groups.join(",")
                );

                let affected = match tx_conn_opt.as_ref() {
                    Some(conn_arc) => {
                        let mut conn = conn_arc.lock().await;
                        exec_on_conn(&mut *conn, &sql, all_vals).await?
                    }
                    None => {
                        let p = get_pool(&pool)?;
                        let mut conn = acquire_conn(p).await?;
                        exec_on_conn(&mut conn, &sql, all_vals).await?
                    }
                };
                total += affected;
            }
            Ok(total)
        })
    }

    #[pyo3(signature = (table, data, condition))]
    pub fn update_by_map<'py>(
        &self,
        py: Python<'py>,
        table: &str,
        data: &Bound<'_, PyDict>,
        condition: &Bound<'_, PyDict>,
    ) -> PyResult<Py<PyAny>> {
        let (sc, mut sv) = py_dict_to_columns_values(data)?;
        let (wc, wv) = py_dict_to_columns_values(condition)?;
        if sc.is_empty() || wc.is_empty() {
            return Err(PyValueError::new_err("update_by_map needs data + condition"));
        }
        let set = sc
            .iter()
            .map(|c| format!("{} = ?", c))
            .collect::<Vec<_>>()
            .join(",");
        let wh = wc
            .iter()
            .map(|c| format!("{} = ?", c))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!("UPDATE {} SET {} WHERE {}", table, set, wh);
        sv.extend(wv);
        let pool = self.pool.clone();
        let tx_arc = self.tx_conn.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let tx_conn_opt = tx_arc
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?
                .clone();
            if let Some(conn_arc) = tx_conn_opt {
                let mut conn = conn_arc.lock().await;
                return exec_on_conn(&mut *conn, &sql, sv).await;
            }
            let p = get_pool(&pool)?;
            let mut conn = acquire_conn(p).await?;
            exec_on_conn(&mut conn, &sql, sv).await
        })
    }

    #[pyo3(signature = (table, condition))]
    pub fn select_by_map<'py>(
        &self,
        py: Python<'py>,
        table: &str,
        condition: &Bound<'_, PyDict>,
    ) -> PyResult<Py<PyAny>> {
        let (wc, wv) = py_dict_to_columns_values(condition)?;
        if wc.is_empty() {
            return Err(PyValueError::new_err("select_by_map needs condition dict"));
        }
        let wh = wc
            .iter()
            .map(|c| format!("{} = ?", c))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!("SELECT * FROM {} WHERE {}", table, wh);
        let pool = self.pool.clone();
        let tx_arc = self.tx_conn.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let tx_conn_opt = tx_arc
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?
                .clone();
            let rows = if let Some(conn_arc) = tx_conn_opt {
                let mut conn = conn_arc.lock().await;
                query_on_conn(&mut *conn, &sql, wv).await?
            } else {
                let p = get_pool(&pool)?;
                let mut conn = acquire_conn(p).await?;
                query_on_conn(&mut conn, &sql, wv).await?
            };
            Python::with_gil(|py| {
                value_vec_to_pylist(&rows, py).map(|l| l.unbind().into_any())
            })
        })
    }

    #[pyo3(signature = (table, condition))]
    pub fn delete_by_map<'py>(
        &self,
        py: Python<'py>,
        table: &str,
        condition: &Bound<'_, PyDict>,
    ) -> PyResult<Py<PyAny>> {
        let (wc, wv) = py_dict_to_columns_values(condition)?;
        if wc.is_empty() {
            return Err(PyValueError::new_err("delete_by_map needs condition dict"));
        }
        let wh = wc
            .iter()
            .map(|c| format!("{} = ?", c))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!("DELETE FROM {} WHERE {}", table, wh);
        let pool = self.pool.clone();
        let tx_arc = self.tx_conn.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let tx_conn_opt = tx_arc
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?
                .clone();
            if let Some(conn_arc) = tx_conn_opt {
                let mut conn = conn_arc.lock().await;
                return exec_on_conn(&mut *conn, &sql, wv).await;
            }
            let p = get_pool(&pool)?;
            let mut conn = acquire_conn(p).await?;
            exec_on_conn(&mut conn, &sql, wv).await
        })
    }

    // ---------- Connection / Transaction ----------

    pub fn acquire<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let pool = self.pool.clone();
        let handle = self.runtime.handle().clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let p = get_pool(&pool)?;
            let conn = acquire_conn(p).await?;
            let py_conn = ConnectionPy {
                inner: Arc::new(AsyncMutex::new(Some(conn))),
                handle: handle.clone(),
            };
            Python::with_gil(|py| Bound::new(py, py_conn).map(|b| b.unbind().into_any()))
        })
    }

    pub fn begin<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let pool = self.pool.clone();
        let tx_arc = self.tx_conn.clone();
        let handle = self.runtime.handle().clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let p = get_pool(&pool)?;
            let mut conn = acquire_conn(p).await?;
            conn.begin()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Begin transaction failed: {}", e)))?;
            let conn_arc = Arc::new(AsyncMutex::new(conn));
            *tx_arc
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))? = Some(conn_arc.clone());
            let tx_clone = tx_arc.clone();
            let handle_clone = handle.clone();
            let done = Arc::new(AtomicBool::new(false));
            Python::with_gil(|py| {
                let tx_obj = Bound::new(
                    py,
                    TransactionPy {
                        conn: conn_arc.clone(),
                        tx_outer: tx_clone,
                        done,
                        handle: handle_clone,
                    },
                )?;
                Ok(tx_obj.unbind().into_any())
            })
        })
    }

    /// Convenience: `begin()` + `auto_commit()` in one step.
    ///
    /// ```python
    /// async with db.begin_defer():
    ///     await db.exec("INSERT ...")
    ///     # auto-commits on success, auto-rollbacks on exception
    /// ```
    pub fn begin_defer<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        // Return the guard directly (no await). __aenter__ will acquire
        // the connection and begin the transaction lazily.
        let guard = AutoCommitGuard {
            pool: Some(self.pool.clone()),
            conn: None,
            tx_outer: self.tx_conn.clone(),
            done: Arc::new(AtomicBool::new(false)),
            handle: self.runtime.handle().clone(),
        };
        Bound::new(py, guard).map(|b| b.unbind().into_any())
    }

    pub fn commit<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let tx_arc = self.tx_conn.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let conn = tx_arc
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?
                .take();
            if let Some(conn_arc) = conn {
                let mut conn = conn_arc.lock().await;
                conn.commit()
                    .await
                    .map_err(|e| PyRuntimeError::new_err(format!("Commit failed: {}", e)))?;
                Ok(())
            } else {
                Err(PyRuntimeError::new_err("No active transaction"))
            }
        })
    }

    pub fn rollback<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let tx_arc = self.tx_conn.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let conn = tx_arc
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?
                .take();
            if let Some(conn_arc) = conn {
                let mut conn = conn_arc.lock().await;
                conn.rollback()
                    .await
                    .map_err(|e| PyRuntimeError::new_err(format!("Rollback failed: {}", e)))?;
                Ok(())
            } else {
                Err(PyRuntimeError::new_err("No active transaction"))
            }
        })
    }
}

// ============================================================
//  Transaction (Explicit) Python Class
// ============================================================

/// Transaction class — only created from Rust (via `begin()`).
/// Python users cannot construct this directly.
#[pyclass(name = "Transaction")]
pub struct TransactionPy {
    conn: Arc<AsyncMutex<Box<dyn Connection>>>,
    /// Shared reference to RbatisPy's tx_conn, so commit/rollback
    /// also clears the outer transaction reference.
    tx_outer: Arc<Mutex<Option<Arc<AsyncMutex<Box<dyn Connection>>>>>>,
    /// Set to true when user explicitly calls commit() or rollback().
    /// Checked by AutoCommitGuard to avoid double-commit.
    done: Arc<AtomicBool>,
    handle: tokio::runtime::Handle,
}

#[pymethods]
impl TransactionPy {
    #[pyo3(signature = (sql, params=None))]
    pub fn exec<'py>(
        &self,
        py: Python<'py>,
        sql: &str,
        params: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let args = collect_params(params)?;
        let sql = sql.to_string();
        let conn = self.conn.clone();
        let handle = self.handle.clone();
        spawn_async(py, &handle, async move {
            let mut guard = conn.lock().await;
            exec_on_conn(&mut *guard, &sql, args).await
        })
    }

    #[pyo3(signature = (sql, params=None))]
    pub fn exec_decode<'py>(
        &self,
        py: Python<'py>,
        sql: &str,
        params: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let args = collect_params(params)?;
        let sql = sql.to_string();
        let conn = self.conn.clone();
        let handle = self.handle.clone();
        spawn_async(py, &handle, async move {
            let mut guard = conn.lock().await;
            let rows = query_on_conn(&mut *guard, &sql, args).await?;
            Python::with_gil(|py| {
                value_vec_to_pylist(&rows, py).map(|l| l.unbind().into_any())
            })
        })
    }

    pub fn commit<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let conn = self.conn.clone();
        let tx_outer = self.tx_outer.clone();
        let handle = self.handle.clone();
        let done = self.done.clone();
        spawn_async(py, &handle, async move {
            let _outer = tx_outer
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?
                .take();
            let mut guard = conn.lock().await;
            guard
                .commit()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Commit failed: {}", e)))?;
            done.store(true, Ordering::Relaxed);
            Ok(())
        })
    }

    pub fn rollback<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let conn = self.conn.clone();
        let tx_outer = self.tx_outer.clone();
        let handle = self.handle.clone();
        let done = self.done.clone();
        spawn_async(py, &handle, async move {
            let _outer = tx_outer
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?
                .take();
            let mut guard = conn.lock().await;
            guard
                .rollback()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Rollback failed: {}", e)))?;
            done.store(true, Ordering::Relaxed);
            Ok(())
        })
    }

    /// Return an async context manager that auto-commits on success
    /// or auto-rollbacks on exception when the block exits.
    ///
    /// Usage:
    /// ```python
    /// tx = await db.begin()
    /// async with tx.auto_commit():
    ///     await tx.exec("INSERT ...")
    ///     await tx.exec("UPDATE ...")
    ///     # auto-commit if no error, auto-rollback on exception
    /// ```
    ///
    /// If you call `await tx.commit()` or `await tx.rollback()` explicitly
    /// inside the block, the guard will do nothing on exit.
    pub fn auto_commit<'py>(slf: Bound<'py, Self>, py: Python<'py>) -> PyResult<Bound<'py, AutoCommitGuard>> {
        let this = slf.borrow();
        Bound::new(
            py,
            AutoCommitGuard {
                pool: None,
                conn: Some(this.conn.clone()),
                tx_outer: this.tx_outer.clone(),
                done: this.done.clone(),
                handle: this.handle.clone(),
            },
        )
    }
}

// ============================================================
//  AutoCommitGuard — async context manager for Transaction
// ============================================================

/// Handles auto-commit/rollback on context exit.
///
/// Two usage patterns:
///
/// 1. `auto_commit()` — connection already exists (from TransactionPy).
///    `__aenter__` is a no-op.
///
/// 2. `begin_defer()` — lazy acquire+begin in `__aenter__`.
///    `pool` field is set, `tx_outer` starts empty.
#[pyclass(name = "AutoCommitGuard")]
pub struct AutoCommitGuard {
    /// Pool reference for lazy initialization (begin_defer pattern).
    pool: Option<Arc<OnceLock<Box<dyn Pool>>>>,
    /// Direct connection reference (auto_commit pattern).
    /// When Some, exec/commit/rollback uses this directly.
    /// When None, falls back to tx_outer (begin_defer pattern).
    conn: Option<Arc<AsyncMutex<Box<dyn Connection>>>>,
    /// Shared reference to RbatisPy's tx_conn / Transaction.tx_outer.
    /// Used for clearing outer state and as fallback connection source.
    tx_outer: Arc<Mutex<Option<Arc<AsyncMutex<Box<dyn Connection>>>>>>,
    /// Set to true when transaction is explicitly committed/rolled back,
    /// or when __aexit__ has handled it. Prevents double-commit and Drop race.
    done: Arc<AtomicBool>,
    handle: tokio::runtime::Handle,
}

/// Get the connection from self.conn (auto_commit) or tx_outer (begin_defer).
fn guard_get_conn(
    self_conn: &Option<Arc<AsyncMutex<Box<dyn Connection>>>>,
    tx_outer: &Arc<Mutex<Option<Arc<AsyncMutex<Box<dyn Connection>>>>>>,
) -> Result<Arc<AsyncMutex<Box<dyn Connection>>>, PyErr> {
    if let Some(c) = self_conn.clone() {
        return Ok(c);
    }
    tx_outer
        .lock()
        .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?
        .clone()
        .ok_or_else(|| PyRuntimeError::new_err("No active transaction"))
}

#[pymethods]
impl AutoCommitGuard {
    fn __aenter__<'py>(slf: Bound<'py, Self>, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let has_pool = slf.borrow().pool.is_some();
        if !has_pool {
            // auto_commit pattern — connection already available.
            // Must return an awaitable that resolves to the guard.
            let handle = slf.borrow().handle.clone();
            let guard = slf.unbind().into_any();
            return spawn_async(py, &handle, async move { Ok(guard) });
        }
        // begin_defer pattern — acquire connection and begin transaction
        let pool = slf
            .borrow()
            .pool
            .clone()
            .ok_or_else(|| PyRuntimeError::new_err("AutoCommitGuard: pool not initialized"))?;
        let tx_outer = slf.borrow().tx_outer.clone();
        let handle = slf.borrow().handle.clone();
        let guard = slf.unbind().into_any();
        spawn_async(py, &handle, async move {
            let p = get_pool(&pool)?;
            let mut conn = acquire_conn(p).await?;
            conn.begin()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Begin transaction failed: {}", e)))?;
            *tx_outer
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))? =
                Some(Arc::new(AsyncMutex::new(conn)));
            Ok(guard)
        })
    }

    /// Execute SQL on the transaction connection.
    #[pyo3(signature = (sql, params=None))]
    pub fn exec<'py>(
        &self,
        py: Python<'py>,
        sql: &str,
        params: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let args = collect_params(params)?;
        let sql = sql.to_string();
        let conn_opt = self.conn.clone();
        let tx_outer = self.tx_outer.clone();
        let handle = self.handle.clone();
        spawn_async(py, &handle, async move {
            let conn_arc = guard_get_conn(&conn_opt, &tx_outer)?;
            let mut guard = conn_arc.lock().await;
            exec_on_conn(&mut *guard, &sql, args).await
        })
    }

    /// Query on the transaction connection.
    #[pyo3(signature = (sql, params=None))]
    pub fn exec_decode<'py>(
        &self,
        py: Python<'py>,
        sql: &str,
        params: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let args = collect_params(params)?;
        let sql = sql.to_string();
        let conn_opt = self.conn.clone();
        let tx_outer = self.tx_outer.clone();
        let handle = self.handle.clone();
        spawn_async(py, &handle, async move {
            let conn_arc = guard_get_conn(&conn_opt, &tx_outer)?;
            let mut guard = conn_arc.lock().await;
            let rows = query_on_conn(&mut *guard, &sql, args).await?;
            Python::with_gil(|py| {
                value_vec_to_pylist(&rows, py).map(|l| l.unbind().into_any())
            })
        })
    }

    /// Commit the transaction explicitly. Guard will no-op on exit.
    pub fn commit<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let conn_opt = self.conn.clone();
        let tx_outer = self.tx_outer.clone();
        let done = self.done.clone();
        let handle = self.handle.clone();
        spawn_async(py, &handle, async move {
            let conn_arc = guard_get_conn(&conn_opt, &tx_outer)?;
            // Clear tx_outer if using self.conn (auto_commit pattern)
            let _outer = tx_outer.lock().map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?.take();
            let mut guard = conn_arc.lock().await;
            guard
                .commit()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Commit failed: {}", e)))?;
            done.store(true, Ordering::Relaxed);
            Ok(())
        })
    }

    /// Rollback the transaction explicitly. Guard will no-op on exit.
    pub fn rollback<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let conn_opt = self.conn.clone();
        let tx_outer = self.tx_outer.clone();
        let done = self.done.clone();
        let handle = self.handle.clone();
        spawn_async(py, &handle, async move {
            let conn_arc = guard_get_conn(&conn_opt, &tx_outer)?;
            let _outer = tx_outer.lock().map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?.take();
            let mut guard = conn_arc.lock().await;
            guard
                .rollback()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Rollback failed: {}", e)))?;
            done.store(true, Ordering::Relaxed);
            Ok(())
        })
    }

    #[pyo3(signature = (exc_type=None, _exc_val=None, _exc_tb=None))]
    fn __aexit__<'py>(
        &self,
        py: Python<'py>,
        exc_type: Option<&Bound<'py, PyAny>>,
        _exc_val: Option<&Bound<'py, PyAny>>,
        _exc_tb: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        if self.done.load(Ordering::Relaxed) {
            return spawn_async(py, &self.handle, async move { Ok(false) });
        }
        let has_err = exc_type.is_some();
        let conn_opt = self.conn.clone();
        let tx_outer = self.tx_outer.clone();
        let done = self.done.clone();
        let handle = self.handle.clone();
        spawn_async(py, &handle, async move {
            done.store(true, Ordering::Relaxed);
            let conn_arc = match guard_get_conn(&conn_opt, &tx_outer) {
                Ok(c) => c,
                Err(_) => return Ok(false),
            };
            let _outer = tx_outer.lock().map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?.take();
            let mut guard = conn_arc.lock().await;
            if has_err {
                let _ = guard.rollback().await;
            } else {
                guard
                    .commit()
                    .await
                    .map_err(|e| PyRuntimeError::new_err(format!("Commit failed: {}", e)))?;
            }
            Ok(false)
        })
    }
}

impl AutoCommitGuard {
    fn drop_take_conn(&mut self) -> Option<Arc<AsyncMutex<Box<dyn Connection>>>> {
        // Prefer self.conn (auto_commit pattern), then tx_outer (begin_defer)
        self.conn
            .take()
            .or_else(|| self.tx_outer.lock().unwrap_or_else(|e| e.into_inner()).take())
    }
}

/// Safety net: rollback if the guard was dropped without __aexit__
/// being called (e.g. the Python object was garbage collected
/// without entering the context).
impl Drop for AutoCommitGuard {
    fn drop(&mut self) {
        if !self.done.load(Ordering::Relaxed) {
            self.done.store(true, Ordering::Relaxed);
            if let Some(conn_arc) = self.drop_take_conn() {
                let handle = self.handle.clone();
                handle.spawn(async move {
                    let mut guard = conn_arc.lock().await;
                    let _ = guard.rollback().await;
                });
            }
        }
    }
}

// ============================================================
//  Connection Python Class
// ============================================================

#[pyclass(name = "Connection")]
pub struct ConnectionPy {
    inner: Arc<AsyncMutex<Option<Box<dyn Connection>>>>,
    handle: tokio::runtime::Handle,
}

#[pymethods]
impl ConnectionPy {
    #[pyo3(signature = (sql, params=None))]
    pub fn exec<'py>(
        &self,
        py: Python<'py>,
        sql: &str,
        params: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let args = collect_params(params)?;
        let sql = sql.to_string();
        let inner = self.inner.clone();
        let handle = self.handle.clone();
        spawn_async(py, &handle, async move {
            let mut guard = inner.lock().await;
            let conn = guard
                .as_mut()
                .ok_or_else(|| PyRuntimeError::new_err("Connection closed"))?;
            exec_on_conn(conn, &sql, args).await
        })
    }

    #[pyo3(signature = (sql, params=None))]
    pub fn exec_decode<'py>(
        &self,
        py: Python<'py>,
        sql: &str,
        params: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let args = collect_params(params)?;
        let sql = sql.to_string();
        let inner = self.inner.clone();
        let handle = self.handle.clone();
        spawn_async(py, &handle, async move {
            let mut guard = inner.lock().await;
            let conn = guard
                .as_mut()
                .ok_or_else(|| PyRuntimeError::new_err("Connection closed"))?;
            let rows = query_on_conn(conn, &sql, args).await?;
            Python::with_gil(|py| {
                value_vec_to_pylist(&rows, py).map(|l| l.unbind().into_any())
            })
        })
    }

    pub fn begin<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let inner = self.inner.clone();
        let handle = self.handle.clone();
        let handle2 = handle.clone();
        spawn_async(py, &handle, async move {
            let mut guard = inner.lock().await;
            let mut conn = guard
                .take()
                .ok_or_else(|| PyRuntimeError::new_err("Connection closed"))?;
            conn.begin()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Begin transaction failed: {}", e)))?;
            let conn_arc = Arc::new(AsyncMutex::new(conn));
            let done = Arc::new(AtomicBool::new(false));
            Python::with_gil(|py| {
                let tx_obj = Bound::new(
                    py,
                    TransactionPy {
                        conn: conn_arc,
                        tx_outer: Arc::new(Mutex::new(None)),
                        done,
                        handle: handle2.clone(),
                    },
                )?;
                Ok(tx_obj.unbind().into_any())
            })
        })
    }

    pub fn close(&self) {
        let inner = self.inner.clone();
        let handle = self.handle.clone();
        // Take the connection so it's dropped (returned to pool)
        handle.spawn(async move {
            let mut guard = inner.lock().await;
            let _conn = guard.take();
        });
    }
}

// ============================================================
//  PyO3 Module
// ============================================================

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<RbatisPy>()?;
    m.add_class::<TransactionPy>()?;
    m.add_class::<AutoCommitGuard>()?;
    m.add_class::<ConnectionPy>()?;
    m.add("__version__", "0.1.0")?;
    Ok(())
}
