//! Zero-panic conversion utilities with explicit error handling
use crate::errors::ReconcilerError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule};
use std::collections::HashMap;

/// Convert Python dict to Rust HashMap with detailed errors
pub fn py_dict_to_rust_map<'py>(
    py: Python<'py>,
    obj: &Bound<'py, PyAny>,
) -> Result<HashMap<String, serde_json::Value>, ReconcilerError> {
    let v = python_to_json(py, obj).map_err(|e| ReconcilerError::TypeConversionError {
        expected: "dict".into(),
        actual: format!("failed to serialize object to JSON: {}", e),
    })?;

    if let serde_json::Value::Object(map) = v {
        Ok(map.into_iter().collect())
    } else {
        Err(ReconcilerError::TypeConversionError {
            expected: "dict".into(),
            actual: format!("value is not a dict/json object (was {})", v),
        })
    }
}

/// Convert Python object to JSON with full type support
pub fn python_to_json<'py>(
    py: Python<'py>, 
    obj: &Bound<'py, PyAny>
) -> Result<serde_json::Value, ReconcilerError> {
    let json_mod = PyModule::import(py, "json").map_err(|e: PyErr| ReconcilerError::PythonError(e.to_string()))?;
    let dumps = json_mod.getattr("dumps").map_err(|e: PyErr| ReconcilerError::PythonError(e.to_string()))?;

    let dumped = dumps.call1((obj,)).map_err(|e: PyErr| ReconcilerError::PythonError(e.to_string()))?;
    
    let s: String = dumped.extract().map_err(|e: PyErr| ReconcilerError::PythonError(e.to_string()))?;
    serde_json::from_str(&s).map_err(|e| ReconcilerError::TypeConversionError {
        expected: "JSON-serializable type".into(),
        actual: e.to_string(),
    })
}

/// Convert JSON back to Python with proper type mapping
pub fn json_to_pyobject<'py>(
    py: Python<'py>, 
    value: &serde_json::Value
) -> PyResult<Bound<'py, PyAny>> {
    match value {
        serde_json::Value::Null => Ok(py.None().into_bound(py).into_any()),
        // FIX: Use .to_owned() to convert Borrowed to Bound for &bool
        serde_json::Value::Bool(b) => Ok((*b).into_pyobject(py)?.to_owned().into_any()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.into_any())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)?.into_any())
            } else {
                Ok(n.to_string().into_pyobject(py)?.into_any())
            }
        }
        // FIX: Use as_str() to get &str which returns Bound directly
        serde_json::Value::String(s) => Ok(s.as_str().into_pyobject(py)?.into_any()),
        serde_json::Value::Array(arr) => {
            let list = PyList::empty(py);
            for v in arr {
                let item = json_to_pyobject(py, v)?;
                list.append(item)?;
            }
            Ok(list.into_any())
        }
        serde_json::Value::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                let pyv = json_to_pyobject(py, v)?;
                dict.set_item(k, pyv)?;
            }
            Ok(dict.into_any())
        }
    }
}