// /src/errors.rs
//! Robust error handling that never panics and provides clear diagnostic messages
use pyo3::{exceptions::PyValueError, PyErr};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ReconcilerError {
    #[error("Key extraction failed: {details}")]
    KeyError { details: String },
    
    #[error("Property error for '{property}': {details}")]
    PropError { property: String, details: String },
    
    #[error("Type conversion error: expected {expected}, got {actual}")]
    TypeConversionError { expected: String, actual: String },
    
    #[error("HTML generation failed for widget '{widget_type}': {details}")]
    HtmlGenerationError { widget_type: String, details: String },
    
    #[error("Serialization error: {0}")]
    SerdeError(#[from] serde_json::Error),
    
    #[error("Python call failed: {0}")]
    PythonError(String),
}

// Helper macro for safe key extraction
// near the top of src/errors.rs, after the error enum
#[macro_export]
macro_rules! safe_get {
    ($dict:expr, $key:expr, $ty:ty) => {{
        // get_item returns Result<Option<...>, PyErr>, so unwrap the Result first
        let item = ($dict.get_item($key)?) .ok_or_else(|| crate::errors::ReconcilerError::KeyError {
            details: format!("Missing key '{}'", $key),
        })?;
        item.extract::<$ty>().map_err(|e| crate::errors::ReconcilerError::TypeConversionError {
            expected: stringify!($ty).to_string(),
            actual: e.to_string(),
        })?
    }};
    ($dict:expr, $key:expr, $ty:ty, $default:expr) => {{
        match $dict.get_item($key)? {
            Some(val) => val.extract::<$ty>().map_err(|e| crate::errors::ReconcilerError::TypeConversionError {
                expected: stringify!($ty).to_string(),
                actual: e.to_string(),
            }),
            None => Ok($default),
        }?
    }};
}

impl From<ReconcilerError> for PyErr {
    fn from(err: ReconcilerError) -> Self {
        PyValueError::new_err(err.to_string())
    }
}


impl From<PyErr> for ReconcilerError {
    fn from(err: PyErr) -> Self {
        ReconcilerError::PythonError(err.to_string())
    }
}




