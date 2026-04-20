use std::cell::RefCell;
use std::str::FromStr;

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};

use sparkid::{IdGenerator, SparkId};

/// Maximum encodable timestamp: 58^8 - 1
const MAX_TIMESTAMP: u64 = 128_063_081_718_015;

thread_local! {
    static LOCAL_GEN: RefCell<IdGenerator> = RefCell::new(IdGenerator::new());
}

/// Validate timestamp range before passing to Rust (which would panic).
#[inline]
fn validate_timestamp(timestamp_ms: i64) -> PyResult<u64> {
    if timestamp_ms < 0 || timestamp_ms as u64 > MAX_TIMESTAMP {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Timestamp out of range: {} (valid range: 0 to {})",
            timestamp_ms, MAX_TIMESTAMP
        )));
    }
    Ok(timestamp_ms as u64)
}

#[pyclass(name = "IdGenerator")]
struct PyIdGenerator {
    inner: IdGenerator,
}

#[pymethods]
impl PyIdGenerator {
    #[new]
    fn __new__() -> Self {
        PyIdGenerator {
            inner: IdGenerator::new(),
        }
    }

    fn generate<'py>(&mut self, py: Python<'py>) -> Bound<'py, PyString> {
        let id = self.inner.next_id();
        let id_str = id.as_str();
        PyString::new(py, &*id_str)
    }

    fn generate_at<'py>(&mut self, py: Python<'py>, timestamp_ms: i64) -> PyResult<Bound<'py, PyString>> {
        let ts = validate_timestamp(timestamp_ms)?;
        let id = self.inner.next_id_at(ts);
        let id_str = id.as_str();
        Ok(PyString::new(py, &*id_str))
    }

    fn reset(&mut self) {
        self.inner = IdGenerator::new();
    }
}

#[pyfunction]
fn generate_id(py: Python<'_>) -> Bound<'_, PyString> {
    LOCAL_GEN.with(|gen| {
        let id = gen.borrow_mut().next_id();
        let id_str = id.as_str();
        PyString::new(py, &*id_str)
    })
}

#[pyfunction]
fn generate_id_at(py: Python<'_>, timestamp_ms: i64) -> PyResult<Bound<'_, PyString>> {
    let ts = validate_timestamp(timestamp_ms)?;
    LOCAL_GEN.with(|gen| {
        let id = gen.borrow_mut().next_id_at(ts);
        let id_str = id.as_str();
        Ok(PyString::new(py, &*id_str))
    })
}

#[pyfunction]
fn extract_timestamp_ms(id: &str) -> PyResult<u64> {
    let spark_id = SparkId::from_str(id).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(e.to_string())
    })?;
    Ok(spark_id.timestamp_ms())
}

#[pyfunction]
fn to_bytes<'py>(py: Python<'py>, id: &str) -> PyResult<Bound<'py, PyBytes>> {
    let spark_id = SparkId::from_str(id).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(e.to_string())
    })?;
    let bytes = spark_id.to_bytes();
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
fn from_bytes<'py>(py: Python<'py>, data: &[u8]) -> PyResult<Bound<'py, PyString>> {
    if data.len() != 16 {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "invalid binary length: expected 16, got {}",
            data.len()
        )));
    }
    let mut arr = [0u8; 16];
    arr.copy_from_slice(data);
    let spark_id = SparkId::from_bytes(arr).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(e.to_string())
    })?;
    let id_str = spark_id.as_str();
    Ok(PyString::new(py, &*id_str))
}

#[pyfunction]
fn reset_thread_local() {
    LOCAL_GEN.with(|gen| {
        *gen.borrow_mut() = IdGenerator::new();
    });
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyIdGenerator>()?;
    m.add_function(wrap_pyfunction!(generate_id, m)?)?;
    m.add_function(wrap_pyfunction!(generate_id_at, m)?)?;
    m.add_function(wrap_pyfunction!(extract_timestamp_ms, m)?)?;
    m.add_function(wrap_pyfunction!(to_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(from_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(reset_thread_local, m)?)?;
    Ok(())
}
