//! Python module entry point with GIL-safe operations
mod converters;
mod diff_engine;
mod errors;
mod html_generator;
mod types;

use crate::errors::ReconcilerError;
use crate::html_generator::map_to_json_value;
use converters::{json_to_pyobject, py_dict_to_rust_map};
use diff_engine::DiffEngine;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList}; // REMOVED unused PyTuple
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex}; // REMOVED unused atomic imports
use types::{PatchAction, RustNodeData, RustPatch, RustReconciliationResult}; // REMOVED JsInitializer

#[pyclass]
pub struct Reconciler {
    context_maps: Arc<Mutex<HashMap<String, HashMap<String, RustNodeData>>>>,
}

#[pymethods]
impl Reconciler {
    #[new]
    fn new() -> Self {
        println!("🪄  PyThra Framework | Reconciler Initialized (Rust)");

        let mut context_maps = HashMap::new();
        context_maps.insert("main".to_string(), HashMap::new());

        Reconciler {
            context_maps: Arc::new(Mutex::new(context_maps)),
        }
    }

    fn clear_context(&self, context_key: String) {
        let mut maps = self.context_maps.lock().unwrap();
        maps.remove(&context_key);
    }

    fn clear_all_contexts(&self) {
        let mut maps = self.context_maps.lock().unwrap();
        maps.clear();
        maps.insert("main".to_string(), HashMap::new());
        println!("Reconciler: Clearing all contexts.");
    }

    #[pyo3(signature = (previous_map, new_widget_root, parent_html_id, is_partial_reconciliation=false, old_root_key=None))]
    fn reconcile<'py>(
        &self,
        py: Python<'py>,
        previous_map: Py<PyDict>,
        new_widget_root: Option<Py<PyAny>>,
        parent_html_id: String,
        is_partial_reconciliation: bool,
        old_root_key: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        // FIX: Bind Py<PyDict> to get &Bound<PyDict>
        let previous_map_bound = previous_map.bind(py);
        println!("Reconciler: Starting reconciliation. Previous map size: {}, New widget root: {}, Parent HTML ID: '{}', Partial: {}, Old root key: {:?}",
            previous_map_bound.len(),
            if new_widget_root.is_some() { "Some" } else { "None" },
            parent_html_id,
            is_partial_reconciliation,
            old_root_key,
        );
        
        let old_map = self
        .build_rust_node_map(py, previous_map_bound)
            .map_err(|e| PyValueError::new_err(format!("Failed to parse previous_map: {}", e)))?;

        // Build new tree map
        let mut new_map = HashMap::new();
        if let Some(root) = new_widget_root {
            // FIX: Bind Py<PyAny> to get &Bound<PyAny>
            let root_bound = root.bind(py);
            self.build_new_tree_map(py, root_bound, &parent_html_id, None, &mut new_map)?;
        }

        let mut rust_result = RustReconciliationResult::default();

        // Determine root key
        let root_key = old_root_key
            .or_else(|| {
                old_map
                    .iter()
                    .find(|(_, data)| {
                        data.parent_html_id == parent_html_id && data.parent_key.is_none()
                    })
                    .map(|(k, _)| k.clone())
            })
            .unwrap_or_else(|| "root".to_string());

        // Run diff engine
        let mut engine = DiffEngine::new(py, &old_map, &new_map, &mut rust_result);
        engine.reconcile(Some(&root_key))?;

        // Handle removals for non-partial reconciliation
        if !is_partial_reconciliation {
            let old_keys: HashSet<_> = old_map.keys().collect();
            let new_keys: HashSet<_> = new_map.keys().collect();
            let removed: Vec<_> = old_keys.difference(&new_keys).cloned().collect();

            for key in removed {
                if let Some(data) = old_map.get(key) { // FIX: key is already &String
                    // Dispose stateful widgets
                    if data.widget_type == "StatefulWidget" {
                        if let Some(ref instance) = data.widget_instance {
                            // FIX: Use Python::attach instead of with_gil
                            Python::attach(|py| {
                                if let Ok(state) = instance.getattr(py, "get_state")?.call0(py) {
                                    if !state.is_none(py) {
                                        let _ = state.getattr(py, "dispose")?.call0(py);
                                    }
                                }
                                Ok::<(), PyErr>(())
                            })?;
                        }
                    }
                    rust_result.patches.push(RustPatch {
                        action: PatchAction::Remove,
                        html_id: data.html_id.clone(),
                        data: serde_json::Value::Null,
                    });
                }
            }
        }

        Ok(self.rust_result_to_python(py, rust_result)?)
    }
}

// Private Rust-only helpers not exposed to Python
impl Reconciler {
    fn build_rust_node_map<'py>(
        &self,
        py: Python<'py>,
        py_dict: &Bound<'py, PyDict>,
    ) -> Result<HashMap<String, RustNodeData>, ReconcilerError> {
        let mut map = HashMap::new();

        // FIX: Use iter() instead of items() - PyO3 0.27+ uses iter()
        for item_result in py_dict.iter() {
            let (key_obj, value) = item_result; // FIX: iter() returns tuples, not Results
            let key_str: String = key_obj.extract().map_err(|e| ReconcilerError::KeyError {
                details: format!("Invalid key in previous_map: {}", e),
            })?;

            let data_dict = value
                // FIX: Use cast instead of deprecated downcast
                .cast::<PyDict>()
                .map_err(|e| ReconcilerError::KeyError {
                    details: format!("Value for key '{}' is not a dict: {}", key_str, e),
                })?;

            // extract widget_instance
            let widget_instance = match data_dict.get_item("widget_instance") {
                Ok(Some(v)) => Some(v.clone().into()),
                Ok(None) => None,
                Err(e) => return Err(ReconcilerError::PythonError(e.to_string())),
            };

            // FIX: Store get_item result to avoid temporary value drop
            let props_item = data_dict.get_item("props")?
                .ok_or(ReconcilerError::KeyError {
                    details: "Missing 'props' in node dict".into(),
                })?;
            // FIX: Use cast instead of deprecated downcast
            let props_dict = props_item.cast::<PyDict>()
                .map_err(|e| ReconcilerError::PythonError(e.to_string()))?;
            let props = py_dict_to_rust_map(py, props_dict)?;

            // optional parent_key
            let parent_key = match data_dict.get_item("parent_key") {
                Ok(Some(v)) => Some(v.extract::<String>().map_err(|e| {
                    ReconcilerError::TypeConversionError {
                        expected: "String".into(),
                        actual: e.to_string(),
                    }
                })?),
                Ok(None) => None,
                Err(e) => return Err(ReconcilerError::PythonError(e.to_string())),
            };

            let node = RustNodeData {
                html_id: crate::safe_get!(data_dict, "html_id", String),
                widget_type: crate::safe_get!(data_dict, "widget_type", String),
                key: crate::safe_get!(data_dict, "key", String),
                widget_instance,
                props,
                parent_html_id: crate::safe_get!(data_dict, "parent_html_id", String),
                parent_key,
                children_keys: crate::safe_get!(data_dict, "children_keys", Vec<String>),
            };

            map.insert(key_str, node);
        }

        Ok(map)
    }

    fn build_new_tree_map<'py>(
        &self,
        py: Python<'py>,
        // FIX: Use &Bound<PyAny> instead of &PyAny
        widget: &Bound<'py, PyAny>,
        parent_html_id: &str,
        parent_key: Option<&str>,
        map: &mut HashMap<String, RustNodeData>,
    ) -> PyResult<()> {
        // FIX: Bind and call methods on Bound, not Py<T>
        let widget_key = widget
            .getattr("get_unique_id")?
            .call0()?
            .extract::<String>()?;
        let html_id = types::next_id();

        // Obtain props by calling widget.render_props() on the Python side
        let props_any = widget.getattr("render_props")?.call0()?;
        // FIX: Use cast instead of cast_as (cast_as doesn't exist)
        let props_dict = props_any.cast::<PyDict>().map_err(|e| {
            PyValueError::new_err(format!("render_props did not return a dict: {}", e))
        })?;
        let props = py_dict_to_rust_map(py, props_dict)?;

        let children_any = widget.getattr("get_children")?.call0()?;
        // FIX: Use cast instead of cast_as (cast_as doesn't exist)
        let children_list = children_any.cast::<PyList>().map_err(|e| {
            PyValueError::new_err(format!("get_children did not return a list: {}", e))
        })?;
        let mut children_keys: Vec<String> = Vec::new();
        for child_result in children_list.iter() { // iter() yields Bound, not Result
            let child = child_result;
            let id: String = child.getattr("get_unique_id")?.call0()?.extract()?;
            children_keys.push(id);
        }

        // FIX: get_type() returns Bound<PyType>, call .name() on it
        let widget_type = widget.get_type().name()?.to_string();
        // FIX: widget is already Bound, clone it into Py<PyAny>
        let widget_instance_py: Py<PyAny> = widget.clone().into();

        let node = RustNodeData {
            html_id: html_id.clone(),
            widget_type: widget_type.clone(),
            key: widget_key.clone(),
            widget_instance: Some(widget_instance_py), // Py<PyAny> is thread-safe
            props,
            parent_html_id: parent_html_id.to_string(),
            parent_key: parent_key.map(String::from),
            children_keys,
        };

        map.insert(widget_key.clone(), node);

        // EXACT Python parity: Stateful/Stateless children use parent_html_id
        let child_parent_id = if widget_type == "StatefulWidget" || widget_type == "StatelessWidget"
        {
            parent_html_id
        } else {
            &html_id
        };

        for child_item in children_list.iter() { // iter() yields Bound, not Result
            let child = child_item;
            self.build_new_tree_map(py, &child, child_parent_id, Some(&widget_key), map)?;
        }

        Ok(())
    }

    fn rust_result_to_python<'py>(
        &self,
        py: Python<'py>,
        rust_result: RustReconciliationResult,
    ) -> PyResult<Bound<'py, PyAny>> {
        let result = PyDict::new(py);

        // Convert patches
        let patches_list = PyList::empty(py);
        for patch in rust_result.patches {
            let patch_dict = PyDict::new(py);
            patch_dict.set_item("action", patch.action.to_string())?;
            patch_dict.set_item("html_id", patch.html_id)?;
            patch_dict.set_item("data", json_to_pyobject(py, &patch.data)?)?;
            patches_list.append(patch_dict)?;
        }
        result.set_item("patches", patches_list)?;

        // Convert new_rendered_map
        let rendered_map = PyDict::new(py);
        for (key, node) in rust_result.new_rendered_map {
            let node_dict = PyDict::new(py);
            node_dict.set_item("html_id", node.html_id)?;
            node_dict.set_item("widget_type", node.widget_type)?;
            node_dict.set_item("key", node.key)?;
            node_dict.set_item(
                "widget_instance",
                node.widget_instance.unwrap_or_else(|| py.None().into()),
            )?;
            node_dict.set_item(
                "props",
                json_to_pyobject(
                    py,
                    &serde_json::Value::Object(map_to_json_value(&node.props)),
                )?,
            )?;
            node_dict.set_item("parent_html_id", node.parent_html_id)?;
            node_dict.set_item("parent_key", node.parent_key)?;
            node_dict.set_item("children_keys", node.children_keys)?;
            rendered_map.set_item(key, node_dict)?;
        }
        result.set_item("new_rendered_map", rendered_map)?;

        // Convert active_css_details
        let css_details = PyDict::new(py);
        for (class, (generator, style_key)) in rust_result.active_css_details {
            css_details.set_item(class, (generator.0, style_key.0))?;
        }
        result.set_item("active_css_details", css_details)?;

        // Convert registered_callbacks
        let callbacks = PyDict::new(py);
        for (name, callback) in rust_result.registered_callbacks {
            callbacks.set_item(name, callback.0)?;
        }
        result.set_item("registered_callbacks", callbacks)?;

        // Convert js_initializers
        let initializers = PyList::empty(py);
        for init in rust_result.js_initializers {
            initializers.append(json_to_pyobject(py, &serde_json::to_value(&init).unwrap())?)?;
        }
        result.set_item("js_initializers", initializers)?;

        // FIX: Convert dict to any before returning
        Ok(result.into_any())
    }
}

#[pymodule]
fn rust_reconciler(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    // FIX: m is now &Bound<PyModule>, use add_class/add functions
    m.add_class::<Reconciler>()?;

    // Export patch types as constants
    m.add("INSERT", "INSERT")?;
    m.add("REMOVE", "REMOVE")?;
    m.add("UPDATE", "UPDATE")?;
    m.add("MOVE", "MOVE")?;
    m.add("REPLACE", "REPLACE")?;

    Ok(())
}