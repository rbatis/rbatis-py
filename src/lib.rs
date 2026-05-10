use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use rbatis::RBatis;
use rbdc_turso::TursoDriver;
use rbdc_duckdb::DuckDbDriver;
use rbs::value::map::ValueMap;
use rbs::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::runtime::Runtime;

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
/// Without this, `exec_decode::<Vec<Value>>` skips serde and returns raw
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
    // Create a concurrent.futures.Future (thread-safe, can be resolved from any thread)
    let cf_mod = py.import_bound("concurrent.futures")?;
    let cf: Bound<'_, PyAny> = cf_mod.call_method0("Future")?;

    // Clone for the spawned task
    let cf_clone: Py<PyAny> = cf.clone().unbind();

    // Spawn work on tokio runtime
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

    // Wrap concurrent.futures.Future into an asyncio-compatible awaitable
    let asyncio = py.import_bound("asyncio")?;
    let awaitable = asyncio.call_method1("wrap_future", (cf,))?;
    Ok(awaitable.unbind())
}

// ============================================================
//  Rbatis Python Class
// ============================================================

#[pyclass(name = "RBatis")]
pub struct RbatisPy {
    rb: RBatis,
    runtime: Runtime,
    connected: Arc<AtomicBool>,
    tx: Arc<Mutex<Option<rbatis::RBatisTxExecutor>>>,
}

#[pymethods]
impl RbatisPy {
    #[new]
    pub fn new() -> PyResult<Self> {
        let runtime =
            Runtime::new().map_err(|e| PyRuntimeError::new_err(format!("Runtime: {}", e)))?;
        Ok(RbatisPy {
            rb: RBatis::new(),
            runtime,
            connected: Arc::new(AtomicBool::new(false)),
            tx: Arc::new(Mutex::new(None)),
        })
    }

    // ---------- Connection ----------

    pub fn link_sqlite<'py>(&self, py: Python<'py>, url: &str) -> PyResult<Py<PyAny>> {
        let rb = self.rb.clone();
        let connected = self.connected.clone();
        let url = url.to_string();
        spawn_async(py, &self.runtime.handle(), async move {
            rb.link(rbdc_sqlite::driver::SqliteDriver {}, &url)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("SQLite connect failed: {}", e)))?;
            connected.store(true, Ordering::Relaxed);
            Ok(())
        })
    }

    pub fn link_mysql<'py>(&self, py: Python<'py>, url: &str) -> PyResult<Py<PyAny>> {
        let rb = self.rb.clone();
        let connected = self.connected.clone();
        let url = url.to_string();
        spawn_async(py, &self.runtime.handle(), async move {
            rb.link(rbdc_mysql::driver::MysqlDriver {}, &url)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("MySQL connect failed: {}", e)))?;
            connected.store(true, Ordering::Relaxed);
            Ok(())
        })
    }

    pub fn link_postgres<'py>(&self, py: Python<'py>, url: &str) -> PyResult<Py<PyAny>> {
        let rb = self.rb.clone();
        let connected = self.connected.clone();
        let url = url.to_string();
        spawn_async(py, &self.runtime.handle(), async move {
            rb.link(rbdc_pg::driver::PgDriver {}, &url)
                .await
                .map_err(|e| {
                    PyRuntimeError::new_err(format!("PostgreSQL connect failed: {}", e))
                })?;
            connected.store(true, Ordering::Relaxed);
            Ok(())
        })
    }

    pub fn link_mssql<'py>(&self, py: Python<'py>, url: &str) -> PyResult<Py<PyAny>> {
        let rb = self.rb.clone();
        let connected = self.connected.clone();
        let url = url.to_string();
        spawn_async(py, &self.runtime.handle(), async move {
            rb.link(rbdc_mssql::driver::MssqlDriver {}, &url)
                .await
                .map_err(|e| {
                    PyRuntimeError::new_err(format!("MSSQL connect failed: {}", e))
                })?;
            connected.store(true, Ordering::Relaxed);
            Ok(())
        })
    }

    pub fn link_turso<'py>(&self, py: Python<'py>, url: &str) -> PyResult<Py<PyAny>> {
        let rb = self.rb.clone();
        let connected = self.connected.clone();
        let url = url.to_string();
        spawn_async(py, &self.runtime.handle(), async move {
            rb.link(TursoDriver {}, &url)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Turso connect failed: {}", e)))?;
            connected.store(true, Ordering::Relaxed);
            Ok(())
        })
    }

    pub fn link_duckdb<'py>(&self, py: Python<'py>, url: &str) -> PyResult<Py<PyAny>> {
        let rb = self.rb.clone();
        let connected = self.connected.clone();
        let url = url.to_string();
        spawn_async(py, &self.runtime.handle(), async move {
            rb.link(DuckDbDriver {}, &url)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("DuckDB connect failed: {}", e)))?;
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

    /// Set the maximum number of connections the pool may establish.
    pub fn set_pool_max_size<'py>(
        &self,
        py: Python<'py>,
        max_size: u64,
    ) -> PyResult<Py<PyAny>> {
        let rb = self.rb.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let pool = rb
                .get_pool()
                .map_err(|e| PyRuntimeError::new_err(format!("Pool not initialized: {}", e)))?;
            pool.set_max_open_conns(max_size).await;
            Ok(())
        })
    }

    /// Set the maximum idle connections kept in the pool.
    pub fn set_pool_max_idle<'py>(
        &self,
        py: Python<'py>,
        max_idle: u64,
    ) -> PyResult<Py<PyAny>> {
        let rb = self.rb.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let pool = rb
                .get_pool()
                .map_err(|e| PyRuntimeError::new_err(format!("Pool not initialized: {}", e)))?;
            pool.set_max_idle_conns(max_idle).await;
            Ok(())
        })
    }

    /// Set the connection timeout in seconds (timeout waiting for a connection from the pool).
    pub fn set_pool_connect_timeout<'py>(
        &self,
        py: Python<'py>,
        timeout_secs: u64,
    ) -> PyResult<Py<PyAny>> {
        let rb = self.rb.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let pool = rb
                .get_pool()
                .map_err(|e| PyRuntimeError::new_err(format!("Pool not initialized: {}", e)))?;
            pool.set_timeout(Some(Duration::from_secs(timeout_secs)))
                .await;
            Ok(())
        })
    }

    /// Set the maximum lifetime of a connection in seconds. Connections older
    /// than this will be closed and replaced.
    pub fn set_pool_max_lifetime<'py>(
        &self,
        py: Python<'py>,
        lifetime_secs: u64,
    ) -> PyResult<Py<PyAny>> {
        let rb = self.rb.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let pool = rb
                .get_pool()
                .map_err(|e| PyRuntimeError::new_err(format!("Pool not initialized: {}", e)))?;
            pool.set_conn_max_lifetime(Some(Duration::from_secs(lifetime_secs)))
                .await;
            Ok(())
        })
    }

    /// Inspect pool state. Returns a dict with keys:
    /// `max_open`, `connections`, `in_use`, `idle`, `waits`, `connecting`, `checking`.
    pub fn pool_state<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let rb = self.rb.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let pool = rb
                .get_pool()
                .map_err(|e| PyRuntimeError::new_err(format!("Pool not initialized: {}", e)))?;
            let state = pool.state().await;
            Ok(Python::with_gil(|py| {
                rbs_to_py(&state, py).map(|v| v.unbind().into_any())
            })?)
        })
    }

    pub fn ping<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let rb = self.rb.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            rb.exec("SELECT 1", vec![])
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
        let rb = self.rb.clone();
        let tx_arc = self.tx.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let maybe_tx = tx_arc.lock().unwrap().clone();
            if let Some(tx) = maybe_tx {
                let r = tx
                    .exec(&sql, args)
                    .await
                    .map_err(|e| PyRuntimeError::new_err(format!("SQL exec failed: {}", e)))?;
                Ok(r.rows_affected as i64)
            } else {
                let r = rb
                    .exec(&sql, args)
                    .await
                    .map_err(|e| PyRuntimeError::new_err(format!("SQL exec failed: {}", e)))?;
                Ok(r.rows_affected as i64)
            }
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
        let rb = self.rb.clone();
        let tx_arc = self.tx.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let rows: Vec<Value> = {
                let maybe_tx = tx_arc.lock().unwrap().clone();
                if let Some(tx) = maybe_tx {
                    tx.exec_decode(&sql, args).await.map_err(|e| {
                        PyRuntimeError::new_err(format!("SQL query failed: {}", e))
                    })?
                } else {
                    rb.exec_decode(&sql, args).await.map_err(|e| {
                        PyRuntimeError::new_err(format!("SQL query failed: {}", e))
                    })?
                }
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
        let rb = self.rb.clone();
        let tx_arc = self.tx.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let maybe_tx = tx_arc.lock().unwrap().clone();
            if let Some(tx) = maybe_tx {
                let r = tx.exec(&sql, values).await.map_err(|e| {
                    PyRuntimeError::new_err(format!("Insert failed: {}", e))
                })?;
                Ok(r.rows_affected as i64)
            } else {
                let r = rb.exec(&sql, values).await.map_err(|e| {
                    PyRuntimeError::new_err(format!("Insert failed: {}", e))
                })?;
                Ok(r.rows_affected as i64)
            }
        })
    }

    #[pyo3(signature = (table, data))]
    pub fn insert_batch<'py>(
        &self,
        py: Python<'py>,
        table: &str,
        data: &Bound<'_, PyList>,
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

        let mut all_vals = Vec::new();
        let mut groups = Vec::new();
        for (cols, vals) in &rows {
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

        let sql = format!("INSERT INTO {} ({}) VALUES {}", table, cols_str, groups.join(","));
        let rb = self.rb.clone();
        let tx_arc = self.tx.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let maybe_tx = tx_arc.lock().unwrap().clone();
            if let Some(tx) = maybe_tx {
                let r = tx.exec(&sql, all_vals).await.map_err(|e| {
                    PyRuntimeError::new_err(format!("Batch insert failed: {}", e))
                })?;
                Ok(r.rows_affected as i64)
            } else {
                let r = rb.exec(&sql, all_vals).await.map_err(|e| {
                    PyRuntimeError::new_err(format!("Batch insert failed: {}", e))
                })?;
                Ok(r.rows_affected as i64)
            }
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
        let set = sc.iter().map(|c| format!("{} = ?", c)).collect::<Vec<_>>().join(",");
        let wh = wc.iter().map(|c| format!("{} = ?", c)).collect::<Vec<_>>().join(" AND ");
        let sql = format!("UPDATE {} SET {} WHERE {}", table, set, wh);
        sv.extend(wv);
        let rb = self.rb.clone();
        let tx_arc = self.tx.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let maybe_tx = tx_arc.lock().unwrap().clone();
            if let Some(tx) = maybe_tx {
                let r = tx.exec(&sql, sv).await.map_err(|e| {
                    PyRuntimeError::new_err(format!("Update failed: {}", e))
                })?;
                Ok(r.rows_affected as i64)
            } else {
                let r = rb.exec(&sql, sv).await.map_err(|e| {
                    PyRuntimeError::new_err(format!("Update failed: {}", e))
                })?;
                Ok(r.rows_affected as i64)
            }
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
        let wh = wc.iter().map(|c| format!("{} = ?", c)).collect::<Vec<_>>().join(" AND ");
        let sql = format!("SELECT * FROM {} WHERE {}", table, wh);
        let rb = self.rb.clone();
        let tx_arc = self.tx.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let rows: Vec<Value> = {
                let maybe_tx = tx_arc.lock().unwrap().clone();
                if let Some(tx) = maybe_tx {
                    tx.exec_decode(&sql, wv).await.map_err(|e| {
                        PyRuntimeError::new_err(format!("Select failed: {}", e))
                    })?
                } else {
                    rb.exec_decode(&sql, wv).await.map_err(|e| {
                        PyRuntimeError::new_err(format!("Select failed: {}", e))
                    })?
                }
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
        let wh = wc.iter().map(|c| format!("{} = ?", c)).collect::<Vec<_>>().join(" AND ");
        let sql = format!("DELETE FROM {} WHERE {}", table, wh);
        let rb = self.rb.clone();
        let tx_arc = self.tx.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let maybe_tx = tx_arc.lock().unwrap().clone();
            if let Some(tx) = maybe_tx {
                let r = tx.exec(&sql, wv).await.map_err(|e| {
                    PyRuntimeError::new_err(format!("Delete failed: {}", e))
                })?;
                Ok(r.rows_affected as i64)
            } else {
                let r = rb.exec(&sql, wv).await.map_err(|e| {
                    PyRuntimeError::new_err(format!("Delete failed: {}", e))
                })?;
                Ok(r.rows_affected as i64)
            }
        })
    }

    // ---------- Connection / Transaction ----------

    /// Acquire a raw connection from the pool.
    /// Returns a `Connection` object with `exec()`, `exec_decode()`, `begin()`, and `close()`.
    ///
    /// Usage:
    /// ```python
    /// conn = await db.acquire()
    /// rows = await conn.exec_decode("SELECT * FROM user")
    /// await conn.close()
    /// ```
    pub fn acquire<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let rb = self.rb.clone();
        let handle = self.runtime.handle().clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let conn_executor = rb
                .acquire()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Acquire failed: {}", e)))?;
            let py_conn = ConnectionPy {
                inner: Arc::new(Mutex::new(Some(conn_executor))),
                handle: handle.clone(),
            };
            Python::with_gil(|py| {
                Bound::new(py, py_conn).map(|b| b.unbind().into_any())
            })
        })
    }

    /// Explicit transaction: acquire a transaction and return a `Transaction` object.
    /// The caller is responsible for calling `await tx.commit()` or `await tx.rollback()`.
    ///
    /// Usage:
    /// ```python
    /// tx = await db.begin()
    /// await tx.exec("INSERT ...")
    /// await tx.commit()
    /// ```
    pub fn begin<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let rb = self.rb.clone();
        let tx_arc = self.tx.clone();
        let handle = self.runtime.handle().clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let tx = rb
                .acquire_begin()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Begin transaction failed: {}", e)))?;
            let tx_id = tx.tx_id;
            *tx_arc.lock().unwrap() = Some(tx);
            let tx_clone = tx_arc.clone();
            let handle_clone = handle.clone();
            // Return a Transaction object (unbounded, Send)
            Python::with_gil(|py| {
                let tx_obj = Bound::new(py, TransactionPy {
                    tx_id,
                    tx: tx_clone,
                    handle: handle_clone,
                })?;
                Ok(tx_obj.unbind().into_any())
            })
        })
    }

    /// Auto transaction (context manager): enter via `async with db.begin_defer():`.
    /// Automatically commits on success, rolls back on exception.
    ///
    /// Usage:
    /// ```python
    /// async with db.begin_defer():
    ///     await db.exec("INSERT ...")
    ///     # auto commit or rollback
    /// ```
    pub fn begin_defer<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, DeferredTransactionPy>> {
        Bound::new(py, DeferredTransactionPy {
            tx: self.tx.clone(),
            rb: self.rb.clone(),
            handle: self.runtime.handle().clone(),
        })
    }

    /// Commit the active transaction (if any) on this DB instance.
    pub fn commit<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let tx_arc = self.tx.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let inner = tx_arc.lock().unwrap().take();
            if let Some(t) = inner {
                t.commit().await
                    .map_err(|e| PyRuntimeError::new_err(format!("Commit failed: {}", e)))?;
                Ok(())
            } else {
                Err(PyRuntimeError::new_err("No active transaction"))
            }
        })
    }

    /// Rollback the active transaction (if any) on this DB instance.
    pub fn rollback<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let tx_arc = self.tx.clone();
        spawn_async(py, &self.runtime.handle(), async move {
            let inner = tx_arc.lock().unwrap().take();
            if let Some(t) = inner {
                t.rollback().await
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

#[pyclass(name = "Transaction")]
pub struct TransactionPy {
    tx_id: i64,
    tx: Arc<Mutex<Option<rbatis::RBatisTxExecutor>>>,
    handle: tokio::runtime::Handle,
}

#[pymethods]
impl TransactionPy {
    #[new]
    fn new() -> Self {
        TransactionPy {
            tx_id: 0,
            tx: Arc::new(Mutex::new(None)),
            handle: tokio::runtime::Handle::current(),
        }
    }

    fn get_tx_id(&self) -> i64 {
        self.tx_id
    }

    /// Execute SQL (INSERT/UPDATE/DELETE) within this transaction.
    #[pyo3(signature = (sql, params=None))]
    pub fn exec<'py>(
        &self,
        py: Python<'py>,
        sql: &str,
        params: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let args = collect_params(params)?;
        let sql = sql.to_string();
        let tx_arc = self.tx.clone();
        let handle = self.handle.clone();
        spawn_async(py, &handle, async move {
            let tx = {
                let guard = tx_arc.lock().unwrap();
                guard.as_ref()
                    .ok_or_else(|| PyRuntimeError::new_err("Transaction already committed or rolled back"))?
                    .clone()
            };
            let r = tx
                .exec(&sql, args)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("SQL exec failed: {}", e)))?;
            Ok(r.rows_affected as i64)
        })
    }

    /// Query within this transaction. Returns List[Dict].
    #[pyo3(signature = (sql, params=None))]
    pub fn exec_decode<'py>(
        &self,
        py: Python<'py>,
        sql: &str,
        params: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let args = collect_params(params)?;
        let sql = sql.to_string();
        let tx_arc = self.tx.clone();
        let handle = self.handle.clone();
        spawn_async(py, &handle, async move {
            let tx = {
                let guard = tx_arc.lock().unwrap();
                guard.as_ref()
                    .ok_or_else(|| PyRuntimeError::new_err("Transaction already committed or rolled back"))?
                    .clone()
            };
            let rows: Vec<Value> = tx
                .exec_decode(&sql, args)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("SQL query failed: {}", e)))?;
            Python::with_gil(|py| {
                value_vec_to_pylist(&rows, py).map(|l| l.unbind().into_any())
            })
        })
    }

    pub fn commit<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let tx_arc = self.tx.clone();
        let handle = self.handle.clone();
        spawn_async(py, &handle, async move {
            let inner = tx_arc.lock().unwrap().take();
            if let Some(t) = inner {
                t.commit().await
                    .map_err(|e| PyRuntimeError::new_err(format!("Commit failed: {}", e)))?;
                Ok(())
            } else {
                Err(PyRuntimeError::new_err("No active transaction"))
            }
        })
    }

    pub fn rollback<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let tx_arc = self.tx.clone();
        let handle = self.handle.clone();
        spawn_async(py, &handle, async move {
            let inner = tx_arc.lock().unwrap().take();
            if let Some(t) = inner {
                t.rollback().await
                    .map_err(|e| PyRuntimeError::new_err(format!("Rollback failed: {}", e)))?;
                Ok(())
            } else {
                Err(PyRuntimeError::new_err("No active transaction"))
            }
        })
    }
}

// ============================================================
//  DeferredTransaction (Auto / Context Manager) Python Class
// ============================================================

#[pyclass(name = "DeferredTransaction")]
pub struct DeferredTransactionPy {
    tx: Arc<Mutex<Option<rbatis::RBatisTxExecutor>>>,
    rb: RBatis,
    handle: tokio::runtime::Handle,
}

#[pymethods]
impl DeferredTransactionPy {
    #[new]
    fn new() -> Self {
        DeferredTransactionPy {
            tx: Arc::new(Mutex::new(None)),
            rb: RBatis::new(),
            handle: tokio::runtime::Handle::current(),
        }
    }

    /// Acquire the transaction and set it on the DB. Called when entering `async with`.
    fn __aenter__<'py>(slf: Bound<'py, Self>, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let tx_arc = slf.borrow().tx.clone();
        let rb = slf.borrow().rb.clone();
        let handle = slf.borrow().handle.clone();
        spawn_async(py, &handle, async move {
            let tx = rb
                .acquire_begin()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Begin transaction failed: {}", e)))?;
            *tx_arc.lock().unwrap() = Some(tx);
            Ok(())
        })
    }

    /// Auto-commit or auto-rollback on exit.
    #[pyo3(signature = (exc_type=None, _exc_val=None, _exc_tb=None))]
    fn __aexit__<'py>(
        &self,
        py: Python<'py>,
        exc_type: Option<&Bound<'py, PyAny>>,
        _exc_val: Option<&Bound<'py, PyAny>>,
        _exc_tb: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let has_err = exc_type.is_some();
        let tx_arc = self.tx.clone();
        let handle = self.handle.clone();
        spawn_async(py, &handle, async move {
            let inner = tx_arc.lock().unwrap().take();
            if let Some(t) = inner {
                if has_err {
                    let _ = t.rollback().await;
                } else {
                    t.commit().await
                        .map_err(|e| PyRuntimeError::new_err(format!("Commit failed: {}", e)))?;
                }
            }
            Ok(false)
        })
    }
}

// ============================================================
//  Connection Python Class
// ============================================================

#[pyclass(name = "Connection")]
pub struct ConnectionPy {
    inner: Arc<Mutex<Option<rbatis::executor::RBatisConnExecutor>>>,
    handle: tokio::runtime::Handle,
}

#[pymethods]
impl ConnectionPy {
    /// Execute SQL (INSERT/UPDATE/DELETE) on this connection.
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
            let conn_executor = {
                let guard = inner.lock().unwrap();
                guard.as_ref()
                    .ok_or_else(|| PyRuntimeError::new_err("Connection closed"))?
                    .clone()
            };
            let r = conn_executor
                .exec(&sql, args)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("SQL exec failed: {}", e)))?;
            Ok(r.rows_affected as i64)
        })
    }

    /// Query on this connection. Returns List[Dict].
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
            let conn_executor = {
                let guard = inner.lock().unwrap();
                guard.as_ref()
                    .ok_or_else(|| PyRuntimeError::new_err("Connection closed"))?
                    .clone()
            };
            let rows: Vec<Value> = conn_executor
                .exec_decode(&sql, args)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("SQL query failed: {}", e)))?;
            Python::with_gil(|py| {
                value_vec_to_pylist(&rows, py).map(|l| l.unbind().into_any())
            })
        })
    }

    /// Begin a transaction on this connection.
    /// Returns a `Transaction` object (explicit commit/rollback required).
    pub fn begin<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let inner = self.inner.clone();
        let handle = self.handle.clone();
        let handle2 = handle.clone();
        spawn_async(py, &handle, async move {
            let conn_executor = inner
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| PyRuntimeError::new_err("Connection closed"))?;
            let tx = conn_executor
                .begin()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Begin transaction failed: {}", e)))?;
            let tx_id = tx.tx_id;
            // Put the tx executor into a shared container so commit/rollback can take it back
            let tx_container: Arc<Mutex<Option<rbatis::RBatisTxExecutor>>> =
                Arc::new(Mutex::new(Some(tx)));
            Python::with_gil(|py| {
                let tx_obj = Bound::new(py, TransactionPy {
                    tx_id,
                    tx: tx_container,
                    handle: handle2.clone(),
                })?;
                Ok(tx_obj.unbind().into_any())
            })
        })
    }

    /// Close / release this connection back to the pool.
    pub fn close(&self) {
        *self.inner.lock().unwrap() = None;
    }
}

// ============================================================
//  PyO3 Module
// ============================================================

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<RbatisPy>()?;
    m.add_class::<TransactionPy>()?;
    m.add_class::<DeferredTransactionPy>()?;
    m.add_class::<ConnectionPy>()?;
    m.add("__version__", "0.1.0")?;
    Ok(())
}
