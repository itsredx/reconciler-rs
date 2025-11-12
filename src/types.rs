//! Thread-safe types with explicit GIL management
use pyo3::prelude::*;
use pyo3::Python;
use crate::errors::ReconcilerError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Thread-safe wrapper for Python objects (Py<PyAny> is Send + Sync)
pub struct PyObjectWrapper(pub Py<PyAny>);

impl std::fmt::Debug for PyObjectWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PyObjectWrapper(<py-object>)")
    }
}

impl Clone for PyObjectWrapper {
    fn clone(&self) -> Self {
        // Py<PyAny> doesn't implement Clone directly; use the GIL-aware clone_ref
        // FIX: Use Python::attach instead of deprecated with_gil
        Python::attach(|py| PyObjectWrapper(self.0.clone_ref(py)))
    }
}

/// JS initializer with sanitized data
#[derive(Serialize, Deserialize, Clone)]
pub struct JsInitializer {
    #[serde(rename = "type")]
    pub init_type: String,
    pub target_id: String,
    pub data: serde_json::Value,
    pub before_id: Option<String>,
}

/// Patch action enum
#[derive(Debug, Clone, PartialEq)]
pub enum PatchAction {
    Insert,
    Remove,
    Update,
    Move,
    Replace,
}

impl ToString for PatchAction {
    fn to_string(&self) -> String {
        match self {
            PatchAction::Insert => "INSERT".to_string(),
            PatchAction::Remove => "REMOVE".to_string(),
            PatchAction::Update => "UPDATE".to_string(),
            PatchAction::Move => "MOVE".to_string(),
            PatchAction::Replace => "REPLACE".to_string(),
        }
    }
}

/// Native patch representation (zero-GIL processing)
#[derive(Debug, Clone)]
pub struct RustPatch {
    pub action: PatchAction,
    pub html_id: String,
    pub data: serde_json::Value,
}

/// Thread-safe node data with proper Py<PyAny> storage
pub struct RustNodeData {
    pub html_id: String,
    pub html: String,
    pub widget_type: String,
    pub key: String,
    pub widget_instance: Option<Py<PyAny>>,  // Thread-safe: Py<PyAny> is Send
    pub props: HashMap<String, serde_json::Value>,
    pub parent_html_id: String,
    pub parent_key: Option<String>,
    pub children_keys: Vec<String>,
}

impl Clone for RustNodeData {
    fn clone(&self) -> Self {
        let widget_instance = if let Some(ref p) = self.widget_instance {
            // clone_ref requires GIL
            // FIX: Use Python::attach instead of deprecated with_gil
            Some(Python::attach(|py| p.clone_ref(py)))
        } else { None };

        RustNodeData {
            html_id: self.html_id.clone(),
            html: self.html.clone(),
            widget_type: self.widget_type.clone(),
            key: self.key.clone(),
            widget_instance,
            props: self.props.clone(),
            parent_html_id: self.parent_html_id.clone(),
            parent_key: self.parent_key.clone(),
            children_keys: self.children_keys.clone(),
        }
    }
}

impl RustNodeData {
    /// Safely extract a property with detailed error
    pub fn get_prop(&self, key: &str) -> Result<&serde_json::Value, ReconcilerError> {
        self.props.get(key).ok_or_else(|| ReconcilerError::PropError {
            property: key.to_string(),
            details: "Property not found".to_string(),
        })
    }
}

/// Complete reconciliation result in native types
#[derive(Default)]
pub struct RustReconciliationResult {
    pub patches: Vec<RustPatch>,
    pub new_rendered_map: HashMap<String, RustNodeData>,
    pub active_css_details: HashMap<String, (PyObjectWrapper, PyObjectWrapper)>,
    pub registered_callbacks: HashMap<String, PyObjectWrapper>,
    pub js_initializers: Vec<JsInitializer>,
}

/// Global ID generator (lock-free, atomic)
static ID_COUNTER: Lazy<AtomicUsize> = Lazy::new(|| {
    AtomicUsize::new(0)
});

pub fn next_id() -> String {
    let id = ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("fw_id_{}", id)
}