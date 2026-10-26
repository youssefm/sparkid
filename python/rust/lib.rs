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
///
/// Returns `Err(MemoryError)` if CPython cannot allocate the string.
#[cfg(not(PyPy))]
#[inline(always)]
fn sparkid_to_pystring<'py>(py: Python<'py>, id: SparkId) -> PyResult<Bound<'py, PyString>> {
    use pyo3::ffi;
    const ID_LENGTH: isize = 21;
    const ID_LENGTH_USIZE: usize = 21;
    unsafe {
        let ptr = ffi::PyUnicode_New(ID_LENGTH, 127);
        // Must be a real check, not a debug_assert: PyUnicode_New returns NULL
        // (with MemoryError set) on allocation failure, and writing through
        // PyUnicode_DATA(NULL) would be UB. This turns NULL into Err(PyErr)
        // before the pointer is dereferenced.
        let string = Bound::from_owned_ptr_or_err(py, ptr)?;
        let data = ffi::PyUnicode_DATA(string.as_ptr()) as *mut [u8; ID_LENGTH_USIZE];
        id.encode_utf8(&mut *data);
        Ok(string.downcast_into_unchecked())
    }
}

#[cfg(PyPy)]
#[inline(always)]
fn sparkid_to_pystring<'py>(py: Python<'py>, id: SparkId) -> PyResult<Bound<'py, PyString>> {
    let id_str = id.as_str();
    Ok(PyString::new(py, &*id_str))
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

#[pyclass(name = "IdGenerator", subclass)]
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

    fn __call__<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyString>> {
        let id = self.inner.next_id();
        sparkid_to_pystring(py, id)
    }

    // Takes `&Bound<Self>` rather than `&mut self` so the instance is borrowed
    // only after `timestamp_ms` is extracted. With `&mut self`, PyO3 borrows
    // first, and extraction can run arbitrary Python (`__index__`) while the
    // borrow is held: a re-entrant call would get "Already borrowed", and a
    // fork from there would leave the child's `reset()` unable to borrow.
    fn generate_at<'py>(slf: &Bound<'py, Self>, timestamp_ms: i64) -> PyResult<Bound<'py, PyString>> {
        let ts = validate_timestamp(timestamp_ms)?;
        let id = slf.try_borrow_mut()?.inner.next_id_at(ts);
        sparkid_to_pystring(slf.py(), id)
    }

    fn reset(&mut self) {
        self.inner = IdGenerator::new();
    }
}

#[pyfunction]
fn generate_id(py: Python<'_>) -> PyResult<Bound<'_, PyString>> {
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
        sparkid_to_pystring(py, id)
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
    sparkid_to_pystring(py, spark_id)
}

#[pyfunction]
fn reset_thread_local() {
    LOCAL_GEN.with(|gen| {
        // SAFETY: thread_local guarantees single-thread access; no re-entrant calls.
        let gen = unsafe { &mut *gen.get() };
        *gen = IdGenerator::new();
    });
}

/// `gil_used = false`: without it, PyO3 defaults to `true` and importing this
/// module re-enables the GIL process-wide on free-threaded CPython (3.13t+).
/// Running without the GIL is sound here because:
/// - `LOCAL_GEN` is a Rust `thread_local!`, so it was never shared between
///   threads; the GIL wasn't protecting it.
/// - No generator borrow (of `LOCAL_GEN` or of an instance) is held while
///   Python code runs: arguments are extracted before borrowing (see
///   `generate_at`), and the borrowed sections only run Rust code plus
///   `PyUnicode_New` and, if it fails, fetching its `MemoryError`. Neither
///   runs Python (pyo3's `PyErr::take` only calls `str()` on a
///   `PanicException`).
/// - `PyIdGenerator` methods borrow the instance mutably through PyO3's
///   atomic borrow flag, which turns concurrent use of one instance into
///   `RuntimeError` ("Already borrowed") rather than a data race. That fits
///   the documented contract: an instance is not thread-safe, so lock it
///   yourself if you share it.
/// - Fork: CPython stops the world in `PyOS_BeforeFork`. An attached thread
///   only pauses when it detaches or runs Python code, and neither happens
///   while a borrow is held. So no instance borrow can be held at fork time,
///   and the child's `reset()` loop cannot fail with "Already borrowed".
#[pymodule(gil_used = false)]
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
