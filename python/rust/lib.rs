use std::cell::UnsafeCell;
use std::str::FromStr;

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};

use sparkid::{IdGenerator, SparkId};

/// Allocate a Python string and write the SparkId directly into it.
///
/// On CPython, uses `PyUnicode_New(21, 127)` for a compact ASCII string, then
/// calls `SparkId::encode_utf8` to fill the buffer in place — no intermediate
/// copy. On PyPy, falls back to `PyString::new` (PyPy lacks the CPython
/// unicode internals API).
#[cfg(not(PyPy))]
#[inline(always)]
fn sparkid_to_pystring<'py>(py: Python<'py>, id: SparkId) -> Bound<'py, PyString> {
    use pyo3::ffi;
    const ID_LENGTH: isize = 21;
    const ID_LENGTH_USIZE: usize = 21;
    unsafe {
        let ptr = ffi::PyUnicode_New(ID_LENGTH, 127);
        debug_assert!(!ptr.is_null());
        let data = ffi::PyUnicode_DATA(ptr) as *mut [u8; ID_LENGTH_USIZE];
        id.encode_utf8(&mut *data);
        Bound::from_owned_ptr(py, ptr).downcast_into_unchecked()
    }
}

#[cfg(PyPy)]
#[inline(always)]
fn sparkid_to_pystring<'py>(py: Python<'py>, id: SparkId) -> Bound<'py, PyString> {
    let id_str = id.as_str();
    PyString::new(py, &*id_str)
}

/// Maximum encodable timestamp: 58^8 - 1
const MAX_TIMESTAMP: u64 = 128_063_081_718_015;

thread_local! {
    static LOCAL_GEN: UnsafeCell<IdGenerator> = UnsafeCell::new(IdGenerator::new());
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
        sparkid_to_pystring(py, id)
    }

    fn generate_at<'py>(&mut self, py: Python<'py>, timestamp_ms: i64) -> PyResult<Bound<'py, PyString>> {
        let ts = validate_timestamp(timestamp_ms)?;
        let id = self.inner.next_id_at(ts);
        Ok(sparkid_to_pystring(py, id))
    }

    fn reset(&mut self) {
        self.inner = IdGenerator::new();
    }
}

#[pyfunction]
fn generate_id(py: Python<'_>) -> Bound<'_, PyString> {
    LOCAL_GEN.with(|gen| {
        // SAFETY: thread_local guarantees single-thread access; no re-entrant calls.
        let gen = unsafe { &mut *gen.get() };
        let id = gen.next_id();
        sparkid_to_pystring(py, id)
    })
}

#[pyfunction]
fn generate_id_at(py: Python<'_>, timestamp_ms: i64) -> PyResult<Bound<'_, PyString>> {
    let ts = validate_timestamp(timestamp_ms)?;
    LOCAL_GEN.with(|gen| {
        // SAFETY: thread_local guarantees single-thread access; no re-entrant calls.
        let gen = unsafe { &mut *gen.get() };
        let id = gen.next_id_at(ts);
        Ok(sparkid_to_pystring(py, id))
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
    Ok(sparkid_to_pystring(py, spark_id))
}

#[pyfunction]
fn reset_thread_local() {
    LOCAL_GEN.with(|gen| {
        // SAFETY: thread_local guarantees single-thread access; no re-entrant calls.
        let gen = unsafe { &mut *gen.get() };
        *gen = IdGenerator::new();
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
