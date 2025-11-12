//! Zero-panic conversion utilities with explicit error handling
use crate::errors::ReconcilerError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
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
    use serde_json::Value;

    // Recursive conversion from Python object to serde_json::Value
    fn convert<'py>(py: Python<'py>, obj: &Bound<'py, PyAny>) -> Result<Value, ReconcilerError> {
        // None
        if obj.is_none() {
            return Ok(Value::Null);
        }

        // Primitives
        if let Ok(b) = obj.extract::<bool>() {
            return Ok(Value::Bool(b));
        }

        if let Ok(i) = obj.extract::<i64>() {
            return Ok(Value::Number(serde_json::Number::from(i)));
        }

        if let Ok(f) = obj.extract::<f64>() {
            if let Some(n) = serde_json::Number::from_f64(f) {
                return Ok(Value::Number(n));
            } else {
                return Ok(Value::Null);
            }
        }

        if let Ok(s) = obj.extract::<String>() {
            return Ok(Value::String(s));
        }

        // Lists / tuples / sequences
        if let Ok(list) = obj.cast::<PyList>() {
            let mut vec = Vec::with_capacity(list.len());
            for item in list.iter() {
                vec.push(convert(py, &item)?);
            }
            return Ok(Value::Array(vec));
        }

        // Dicts
        if let Ok(dict) = obj.cast::<PyDict>() {
            let mut map = serde_json::Map::new();
            for (k, v) in dict {
                // stringify key
                let key = match k.str() {
                    Ok(pystr) => pystr.to_str().map(|s| s.to_string()).unwrap_or_else(|_| format!("{}", k.repr().map(|r| r.to_string()).unwrap_or_default())),
                    Err(_) => format!("{}", k.repr().map(|r| r.to_string()).unwrap_or_default()),
                };
                let val = convert(py, &v)?;
                map.insert(key, val);
            }
            return Ok(Value::Object(map));
        }

        // Callables / functions / methods -> treat as null (like Python None)
        // Check if the object itself is callable, not just if it has __call__
        if obj.is_callable() {
            return Ok(Value::Null);
        }

        // If it's a PyAny that didn't match above, try to coerce via str()
        match obj.str() {
            Ok(s) => match s.to_str() {
                Ok(st) => Ok(Value::String(st.to_string())),
                Err(_) => Ok(Value::String("<non-utf8-str>".to_string())),
            },
            Err(e) => Err(ReconcilerError::PythonError(e.to_string())),
        }
    }

    convert(py, obj)
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