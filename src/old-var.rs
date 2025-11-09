/* use indexmap::IndexMap;
// use pyo3::prelude::*;
// use pyo3::types::PyDict;
// use serde::Deserialize;
// use std::collections::{HashMap, HashSet};

// // Data structures are unchanged and correct
// #[derive(Deserialize, Debug, Clone, PartialEq, Default)]
// struct PropValue(serde_json::Value);

// #[derive(Deserialize, Debug, Clone)]
// struct WidgetNode {
//     key: String,
//     #[serde(rename = "type")]
//     widget_type: String,
//     #[serde(default)]
//     props: IndexMap<String, PropValue>,
//     #[serde(default)]
//     children: Vec<String>,
// }

// #[pyclass]
// #[derive(Debug)]
// struct Patch {
//     #[pyo3(get)]
//     action: String,
//     #[pyo3(get)]
//     target_id: String,
//     #[pyo3(get)]
//     data: PyObject,
// }

// impl Clone for Patch {
//     fn clone(&self) -> Self {
//         Python::with_gil(|py| Self {
//             action: self.action.clone(),
//             target_id: self.target_id.clone(),
//             data: self.data.clone_ref(py),
//         })
//     }
// }

// type WidgetTreeMap = HashMap<String, WidgetNode>;

// // ============================================================================
// //  The Reconciler Engine (Corrected Design)
// // ============================================================================
// struct Reconciler<'a> {
//     py: Python<'a>,
//     old_tree: &'a WidgetTreeMap,
//     new_tree: &'a WidgetTreeMap,
//     patches: Vec<Patch>,
// }

// impl<'a> Reconciler<'a> {
//     fn diff_node(&mut self, key: &str) {
//         let old_node = self.old_tree.get(key);
//         let new_node = self.new_tree.get(key);

//         match (old_node, new_node) {
//             // Node is new. All its children are also conceptually new.
//             // The actual INSERT patch is handled by the parent's `diff_children`.
//             (None, Some(new_node)) => {
//                 self.diff_children(None, &new_node.children);
//             }
//             // Node exists in both, compare them.
//             (Some(old_node), Some(new_node)) => {
//                 if old_node.widget_type != new_node.widget_type {
//                     self.add_patch("REPLACE", key, None);
//                     // After replacing, all new children are treated as new insertions.
//                     self.diff_children(None, &new_node.children);
//                     return;
//                 }
//                 if old_node.props != new_node.props {
//                     let data = PyDict::new_bound(self.py);
//                     self.add_patch("UPDATE", key, Some(data.into()));
//                 }
//                 // Always diff the children.
//                 self.diff_children(Some(&old_node.children), &new_node.children);
//             }
//             // Other cases (removal, etc.) are handled by the caller (`diff_children`).
//             _ => {}
//         }
//     }

//     fn diff_children(
//         &mut self,
//         old_children_opt: Option<&'a Vec<String>>,
//         new_children: &'a Vec<String>,
//     ) {
//         let empty_vec;
//         let old_children = match old_children_opt {
//             Some(children) => children,
//             None => {
//                 empty_vec = Vec::new();
//                 &empty_vec
//             }
//         };

//         // --- Step 1: Handle REMOVALS first ---
//         let new_keys_set: HashSet<&str> = new_children.iter().map(String::as_str).collect();
//         for old_key in old_children {
//             if !new_keys_set.contains(old_key.as_str()) {
//                 self.add_patch("REMOVE", old_key, None);
//             }
//         }

//         if new_children.is_empty() {
//             return;
//         }

//         // --- Step 2: Handle MOVES and INSERTS ---
//         let old_key_to_idx: HashMap<&str, usize> = old_children
//             .iter()
//             .enumerate()
//             .map(|(i, key)| (key.as_str(), i))
//             .collect();

//         let mut new_to_old_idx_map = Vec::new();
//         let mut sequence_for_lis = Vec::new();

//         for new_key in new_children {
//             if let Some(&old_idx) = old_key_to_idx.get(new_key.as_str()) {
//                 new_to_old_idx_map.push(Some(old_idx));
//                 sequence_for_lis.push(old_idx);
//             } else {
//                 new_to_old_idx_map.push(None);
//             }
//         }

//         let lis_indices_in_seq = lis::longest_increasing_subsequence(&sequence_for_lis);
//         let lis_old_indices: HashSet<usize> = lis_indices_in_seq
//             .into_iter()
//             .map(|i_in_seq| sequence_for_lis[i_in_seq])
//             .collect();

//         for (i, new_key) in new_children.iter().enumerate() {
//             let before_id = new_children.get(i + 1).map(|s| s.as_str());

//             if let Some(old_idx) = new_to_old_idx_map[i] {
//                 // This node existed before.
//                 if !lis_old_indices.contains(&old_idx) {
//                     self.add_move_patch(new_key, before_id);
//                 }
//                 // We must still recurse to check for updates on this existing node.
//                 self.diff_node(new_key);
//             } else {
//                 // This is a new node.
//                 self.add_insert_patch(new_key, before_id);
//                 // We also recurse on new nodes to process their children.
//                 self.diff_node(new_key);
//             }
//         }
//     }

//     fn add_patch(&mut self, action: &str, target_id: &str, data: Option<PyObject>) {
//         let patch_data = data.unwrap_or_else(|| self.py.None().into_py(self.py));
//         self.patches.push(Patch {
//             action: action.to_string(),
//             target_id: target_id.to_string(),
//             data: patch_data,
//         });
//     }

//     fn add_move_patch(&mut self, target_id: &str, before_id: Option<&str>) {
//         let data = PyDict::new_bound(self.py);
//         if let Some(id) = before_id {
//             data.set_item("before_id", id).unwrap();
//         }
//         self.add_patch("MOVE", target_id, Some(data.into()));
//     }

//     fn add_insert_patch(&mut self, target_id: &str, before_id: Option<&str>) {
//         let data = PyDict::new_bound(self.py);
//         if let Some(id) = before_id {
//             data.set_item("before_id", id).unwrap();
//         }
//         self.add_patch("INSERT", target_id, Some(data.into()));
//     }
// }

// // ============================================================================
// //  The Main Python Module
// // ============================================================================

// #[pyfunction]
// fn reconcile(
//     py: Python,
//     old_tree_py: &Bound<PyDict>,
//     new_tree_py: &Bound<PyDict>,
// ) -> PyResult<Vec<Patch>> {
//     let old_tree: WidgetTreeMap =
//         serde_json::from_str(&pyo3_convert_pydict_to_json_str(old_tree_py)?)
//             .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
//     let new_tree: WidgetTreeMap =
//         serde_json::from_str(&pyo3_convert_pydict_to_json_str(new_tree_py)?)
//             .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;

//     let mut reconciler = Reconciler {
//         py,
//         old_tree: &old_tree,
//         new_tree: &new_tree,
//         patches: Vec::new(),
//     };

//     // The simplified entry point. Start at the root and recurse from there.
//     if new_tree.contains_key("root") {
//         reconciler.diff_node("root");
//     }

//     Ok(reconciler.patches)
// }

// fn pyo3_convert_pydict_to_json_str(dict: &Bound<PyDict>) -> PyResult<String> {
//     let py = dict.py();
//     let json_module = py.import_bound("json")?;
//     json_module
//         .call_method1("dumps", (dict,))?
//         .extract::<String>()
// }

// #[pymodule]
// fn rust_reconciler(m: &Bound<PyModule>) -> PyResult<()> {
//     m.add_function(wrap_pyfunction!(reconcile, m)?)?;
//     m.add_class::<Patch>()?;
//     Ok(())
// }

*/

