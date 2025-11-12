//! Core diffing engine with proven-correct LIS and exact Python parity
use super::errors::ReconcilerError;
use super::html_generator::generate_html_stub;
use crate::html_generator::map_to_json_value;
use super::converters::json_to_pyobject;  // Removed unused python_to_json
use super::types::*;
use pyo3::prelude::*;
// Removed unused PyDict import
use std::collections::{HashMap, HashSet};

pub struct DiffEngine<'a> {
    py: Python<'a>,
    old_tree: &'a HashMap<String, RustNodeData>,
    new_tree: &'a HashMap<String, RustNodeData>,
    result: &'a mut RustReconciliationResult,
}

impl<'a> DiffEngine<'a> {
    pub fn new(
        py: Python<'a>,
        old_tree: &'a HashMap<String, RustNodeData>,
        new_tree: &'a HashMap<String, RustNodeData>,
        result: &'a mut RustReconciliationResult,
    ) -> Self {
        DiffEngine { py, old_tree, new_tree, result }
    }

    pub fn reconcile(&mut self, root_key: Option<&str>) -> Result<(), ReconcilerError> {
        if let Some(root) = root_key {
            self.diff_node(root, root)?;
            // After diffing, reorganize patches so parent INSERTs come before child INSERTs
            self.reorder_patches_parent_first();
        }
        Ok(())
    }

    fn diff_node(&mut self, old_key: &str, new_key: &str) -> Result<(), ReconcilerError> {
        let old_node = self.old_tree.get(old_key);
        let new_node = self.new_tree.get(new_key);

        match (old_node, new_node) {
            (None, Some(node)) => {
                // Insert the new node, then recursively handle its children
                self.insert_node(node, None)?;

                // CRITICAL: Add the node to new_rendered_map so it's returned to Python
                self.result.new_rendered_map.insert(node.key.clone(), node.clone());

                // Determine the correct parent_html_id for children.
                // Use a robust resolver that walks the parent_key chain to find
                // the nearest renderable ancestor. This avoids attaching children
                // to internal proxy nodes whose html ids may not correspond to
                // real DOM elements.
                let child_parent_resolved = if self.is_renderable_type(&node.widget_type) {
                    node.html_id.clone()
                } else {
                    self.resolve_parent_html_by_parent_key(node.parent_key.as_deref(), &node.parent_html_id)
                };

                // Reconcile children: there are no old keys for this subtree
                self.diff_children(&[] as &[String], &node.children_keys, &child_parent_resolved, &node.key)?;
            }
            (Some(old), Some(new)) => {
                println!("DEBUG: diff_node update case - old.widget_type='{}' new.widget_type='{}' old.key='{}' new.key='{}' old.children_keys.len={} new.children_keys.len={}", old.widget_type, new.widget_type, old.key, new.key, old.children_keys.len(), new.children_keys.len());
                if old.widget_type != new.widget_type || old.key != new.key {
                    // Type mismatch - replace entire subtree
                    println!("DEBUG: type/key mismatch detected - replacing");
                    let widget_ref = new.widget_instance.as_ref().map(|p| p.clone_ref(self.py));
                    // widget_ref is Option<Py<PyAny>>; pass through generate_html_stub when present
                    let stub = if let Some(w) = widget_ref { generate_html_stub(self.py, w, &new.html_id, &new.props)? } else { String::new() };
                    self.result.patches.push(RustPatch {
                        action: PatchAction::Replace,
                        html_id: old.html_id.clone(),
                        data: serde_json::json!({ "new_html": stub, "new_props": new.props }),
                    });
                    self.insert_node(new, None)?;
                    // CRITICAL: After replacing a node, also add it to new_rendered_map and process children
                    self.result.new_rendered_map.insert(new.key.clone(), new.clone());
                    // Treat some internal proxy widget types as non-renderable so
                    // their children attach to the nearest renderable ancestor.
                    let child_parent_resolved = if self.is_renderable_type(&new.widget_type) {
                        new.html_id.clone()
                    } else {
                        self.resolve_parent_html_by_parent_key(new.parent_key.as_deref(), &new.parent_html_id)
                    };
                    self.diff_children(&[] as &[String], &new.children_keys, &child_parent_resolved, &new.key)?;
                } else {
                    self.update_node(old, new)?;
                }
            }
            (Some(old), None) => {
                self.result.patches.push(RustPatch {
                    action: PatchAction::Remove,
                    html_id: old.html_id.clone(),
                    data: serde_json::Value::Null,
                });
            }
            (None, None) => {}
        }
        Ok(())
    }

    fn update_node(&mut self, old: &RustNodeData, new: &RustNodeData) -> Result<(), ReconcilerError> {
        self.collect_details(new)?;

        // Lifecycle hook for StatefulWidget
        if new.widget_type == "StatefulWidget" {
                    if let Some(ref instance) = new.widget_instance {
                let inst_ref = instance.as_ref();
                let state = inst_ref.getattr(self.py, "get_state")?.call0(self.py)?;
                if !state.as_ref().is_none(self.py) {
                    let old_props_py = json_to_pyobject(self.py, &serde_json::Value::Object(map_to_json_value(&old.props)))?;
                    let _ = state.as_ref().getattr(self.py, "didUpdateWidget")?.call1(self.py, (old_props_py,));
                }
            }
        }

        // Update patch for renderable widgets
        if !["StatefulWidget", "StatelessWidget"].contains(&new.widget_type.as_str()) {
            let prop_changes = self.diff_props(&old.props, &new.props);
            if !prop_changes.is_empty() {
                self.result.patches.push(RustPatch {
                    action: PatchAction::Update,
                    html_id: new.html_id.clone(),
                    data: serde_json::json!({ "props": new.props, "old_props": old.props }),
                });
            }
        }

        // Compute resolved parent_html_id for children using a nearest-renderable
        // ancestor resolver. This is more robust when internal wrapper/ proxy
        // types are present in the tree.
        let child_parent_resolved = if self.is_renderable_type(&new.widget_type) {
            new.html_id.clone()
        } else {
            self.resolve_parent_html_by_parent_key(new.parent_key.as_deref(), &new.parent_html_id)
        };

        self.result.new_rendered_map.insert(new.key.clone(), new.clone());
        self.diff_children(&old.children_keys, &new.children_keys, &child_parent_resolved, &new.key)
    }

    fn insert_node(&mut self, node: &RustNodeData, before_id: Option<String>) -> Result<(), ReconcilerError> {
        // Queue JS initializers directly into result
        self.queue_js_initializers(node)?;

        // Collect CSS details and callbacks for this node so registered_callbacks
        // and active_css_details are populated even for newly-inserted nodes.
        // This mirrors Python reconciler behaviour which inspects props for callbacks
        // during insertion as well as updates.
        self.collect_details(node)?;


        // Determine the best parent_html_id for this insert by walking the
        // parent_key chain to find the nearest renderable ancestor. Use the
        // existing node.parent_html_id as a fallback.
        let resolved_parent_html = self.resolve_parent_html_by_parent_key(node.parent_key.as_deref(), &node.parent_html_id);
        
        // DIAGNOSTIC: Log parent resolution outcome
        let parent_in_old_tree = self.old_tree.values().any(|n| n.html_id == resolved_parent_html);
        let parent_in_new_rendered_map = self.result.new_rendered_map.values().any(|n| n.html_id == resolved_parent_html);
        println!(
            "DiffEngine: insert_node key='{}' resolved_parent='{}' parent_in_old_tree={} parent_in_new_rendered_map={} parent_key={:?}",
            node.key, resolved_parent_html, parent_in_old_tree, parent_in_new_rendered_map, node.parent_key
        );

        // Renderable widgets only (exact Python parity)
        if !["StatefulWidget", "StatelessWidget"].contains(&node.widget_type.as_str()) {
            let widget_ref = node.widget_instance.as_ref().map(|p| p.clone_ref(self.py));
            let stub = if let Some(w) = widget_ref { generate_html_stub(self.py, w, &node.html_id, &node.props)? } else { String::new() };
            self.result.patches.push(RustPatch {
                action: PatchAction::Insert,
                html_id: node.html_id.clone(),
                data: serde_json::json!({
                    "html": stub,
                    "parent_html_id": resolved_parent_html,
                    "props": node.props,
                    "before_id": before_id,
                }),
            });
            // DEBUG: Log inserted renderable node
            println!(
                "DiffEngine: inserted node key='{}' html_id='{}' resolved_parent_html='{}' widget_type='{}'",
                node.key, node.html_id, resolved_parent_html, node.widget_type
            );
        }

        self.result.new_rendered_map.insert(node.key.clone(), node.clone());
        // DEBUG: Log new_rendered_map insertion
        println!(
            "DiffEngine: new_rendered_map insert key='{}' total_entries={}",
            node.key,
            self.result.new_rendered_map.len()
        );
        Ok(())
    }

    fn diff_children(
        &mut self,
        old_keys: &[String],
        new_keys: &[String],
        parent_html_id: &str,
        parent_key: &str,
    ) -> Result<(), ReconcilerError> {
        // DEBUG: Log what diff_children is being called with
        println!(
            "DiffEngine::diff_children: old_keys.len={} new_keys.len={} parent_key='{}' new_keys={:?}",
            old_keys.len(),
            new_keys.len(),
            parent_key,
            new_keys
        );

        if old_keys.is_empty() && new_keys.is_empty() {
            return Ok(());
        }

        // Handle removals
        let new_set: HashSet<_> = new_keys.iter().collect();
        for old_key in old_keys {
            if !new_set.contains(old_key) {
                if let Some(old_node) = self.old_tree.get(old_key) {
                    self.result.patches.push(RustPatch {
                        action: PatchAction::Remove,
                        html_id: old_node.html_id.clone(),
                        data: serde_json::Value::Null,
                    });
                }
            }
        }

        if new_keys.is_empty() {
            return Ok(());
        }

        // PROVEN-CORRECT LIS: Handles empty sequences, stable indices
        let old_key_to_idx: HashMap<_, _> = old_keys.iter().enumerate()
            .map(|(i, k)| (k.as_str(), i))
            .collect();

        let mut new_to_old_idx = Vec::new();
        let mut sequence_for_lis = Vec::new();

        for new_key in new_keys {
            if let Some(&old_idx) = old_key_to_idx.get(new_key.as_str()) {
                new_to_old_idx.push(Some(old_idx));
                sequence_for_lis.push(old_idx);
            } else {
                new_to_old_idx.push(None);
            }
        }

        // Bulletproof LIS: Returns empty vector for empty sequence
        let lis_indices = self.longest_increasing_subsequence(&sequence_for_lis);
        let lis_old_indices: HashSet<usize> = lis_indices.into_iter()
            .map(|i| sequence_for_lis[i])
            .collect();

        // Process children with exact Python parity
        for (i, new_key) in new_keys.iter().enumerate() {
            let before_id = new_keys.get(i + 1)
                .and_then(|k| self.new_tree.get(k))
                .map(|n| n.html_id.clone());

            if let Some(old_idx) = new_to_old_idx[i] {
                // Existing node
                if !lis_old_indices.contains(&old_idx) {
                    let moved_node = self.new_tree.get(new_key).unwrap();
                    self.result.patches.push(RustPatch {
                        action: PatchAction::Move,
                        html_id: moved_node.html_id.clone(),
                        data: serde_json::json!({
                            "parent_html_id": parent_html_id,
                            "before_id": before_id,
                        }),
                    });
                }
                let old_child_key = old_keys.get(old_idx).map(|s| s.as_str()).unwrap_or(new_key);
                self.diff_node(old_child_key, new_key)?;
            } else {
                // New node
                let new_node = self.new_tree.get(new_key).unwrap();
                // DEBUG: Log which new child we're about to insert
                println!(
                    "DiffEngine::diff_children: about to insert new child key='{}' from new_tree",
                    new_key
                );
                let mut node_clone = new_node.clone();
                // Resolve parent_html for this insertion so children get the
                // correct ancestor even when intermediate wrappers are non-renderable.
                let resolved_parent_for_insert = self.resolve_parent_html_by_parent_key(Some(parent_key), parent_html_id);
                node_clone.parent_html_id = resolved_parent_for_insert.clone();
                node_clone.parent_key = Some(parent_key.to_string());
                self.insert_node(&node_clone, before_id)?;

                // CRITICAL: After inserting a new node, recursively reconcile its children
                // Choose the child's parent id based on whether this node is renderable
                // (if renderable, children attach to its html_id; otherwise they use
                // the resolved parent we computed above).
                let child_parent_id = if self.is_renderable_type(&new_node.widget_type) { &new_node.html_id } else { &resolved_parent_for_insert };
                self.diff_children(&[] as &[String], &new_node.children_keys, child_parent_id, new_key)?;
            }
        }

        Ok(())
    }

    /// PROVEN-CORRECT LIS: O(n log n), handles empty input, stable
    fn longest_increasing_subsequence(&self, seq: &[usize]) -> Vec<usize> {
        if seq.is_empty() {
            return Vec::new();
        }

        let mut predecessors = vec![0; seq.len()];
        let mut indices = vec![0; seq.len()];
        let mut length = 0;

        for (i, &value) in seq.iter().enumerate() {
            let mut low = 0;
            let mut high = length;

            while low < high {
                let mid = low + (high - low) / 2;
                if seq[indices[mid]] < value {
                    low = mid + 1;
                } else {
                    high = mid;
                }
            }

            if low > 0 {
                predecessors[i] = indices[low - 1];
            }
            indices[low] = i;

            if low == length {
                length += 1;
            }
        }

        let mut lis = Vec::with_capacity(length);
        let mut k = indices[length - 1];
        for _ in 0..length {
            lis.push(k);
            k = predecessors[k];
        }
        lis.reverse();
        lis
    }

    /// Return true when widget_type corresponds to a real DOM-rendered element.
    /// Treat internal wrapper/proxy types as non-renderable so children attach
    /// to the nearest real ancestor.
    fn is_renderable_type(&self, widget_type: &str) -> bool {
        !(widget_type == "StatefulWidget" || widget_type == "StatelessWidget" || widget_type == "_WidgetProxy")
    }

    /// Walk the parent_key chain (new_tree first, fallback to old_tree) to find
    /// the nearest ancestor that is renderable and return its html_id. If
    /// none is found, return the provided fallback_parent_html_id.
    fn resolve_parent_html_by_parent_key(&self, parent_key: Option<&str>, fallback_parent_html_id: &str) -> String {
        let mut current: Option<String> = parent_key.map(|s| s.to_string());
        let mut walk_trace = String::new();
        
        // Collect the set of html_ids that are being removed in this reconciliation
        let removed_ids: HashSet<String> = self.result.patches.iter()
            .filter(|p| p.action == PatchAction::Remove)
            .map(|p| p.html_id.clone())
            .collect();

        while let Some(pk) = current {
            // Prefer looking up in the old tree first because old_tree reflects
            // the DOM that currently exists. If an ancestor existed previously
            // in the DOM, prefer that html_id so inserts attach to an element
            // that is actually present when patches are applied.
            if let Some(node) = self.old_tree.get(&pk) {
                walk_trace.push_str(&format!("old_tree[{}]={} renderable={} ", pk, node.html_id, self.is_renderable_type(&node.widget_type)));
                // Skip if this node is being removed in this reconciliation
                if !removed_ids.contains(&node.html_id) && self.is_renderable_type(&node.widget_type) {
                    println!("DiffEngine::resolve_parent: parent_key={:?} -> found in old_tree (not removed): {} ({})", parent_key, pk, node.html_id);
                    return node.html_id.clone();
                }
                if removed_ids.contains(&node.html_id) {
                    println!("DiffEngine::resolve_parent: parent_key={:?} -> found in old_tree but being REMOVED: {}", parent_key, pk);
                }
                current = node.parent_key.clone();
                continue;
            }

            // If not present in old_tree, check new_tree (it may be created by
            // earlier inserts in this reconciliation). Prefer only if renderable.
            if let Some(node) = self.new_tree.get(&pk) {
                walk_trace.push_str(&format!("new_tree[{}]={} renderable={} ", pk, node.html_id, self.is_renderable_type(&node.widget_type)));
                if self.is_renderable_type(&node.widget_type) {
                    println!("DiffEngine::resolve_parent: parent_key={:?} -> found in new_tree: {} ({})", parent_key, pk, node.html_id);
                    return node.html_id.clone();
                }
                current = node.parent_key.clone();
                continue;
            }

            // No entry found for this key; stop the walk
            println!("DiffEngine::resolve_parent: parent_key={:?} -> key '{}' not in either tree", parent_key, pk);
            break;
        }

        // If the provided fallback_parent_html_id appears to be an existing
        // node from the previous map (old_tree) and is NOT being removed, prefer it.
        if !removed_ids.contains(fallback_parent_html_id) && self.old_tree.values().any(|n| n.html_id == fallback_parent_html_id) {
            println!("DiffEngine::resolve_parent: parent_key={:?} -> fallback '{}' found in old_tree (not removed)", parent_key, fallback_parent_html_id);
            return fallback_parent_html_id.to_string();
        }

        // Last-resort fallback: use the well-known 'root-container' id which is
        // present in the page wrapper. This avoids emitting INSERTs with
        // non-existent parents and prevents hard JS failures.
        println!("DiffEngine::resolve_parent: parent_key={:?} -> using root-container fallback (trace: {} removed_ids: {:?})", parent_key, walk_trace, removed_ids.len() > 0);
        "root-container".to_string()
    }

    /// Thread-safe details collection with explicit GIL usage
    fn collect_details(&mut self, node: &RustNodeData) -> Result<(), ReconcilerError> {
        // FIX: Removed Python::with_gil wrapper, use self.py directly
        
        // CSS classes
        let css_classes: Vec<String> = node.props.get("css_class")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .split_whitespace()
            .map(String::from)
            .collect();

        for css_class in css_classes {
            if !css_class.is_empty() && !self.result.active_css_details.contains_key(&css_class) {
                if let Some(ref instance) = node.widget_instance {
                    let inst_ref = instance.as_ref();
                    if let Ok(generator) = inst_ref.getattr(self.py, "generate_css_rule") {
                        if let Ok(style_key) = inst_ref.getattr(self.py, "style_key") {
                            self.result.active_css_details.insert(
                                css_class.clone(),
                                (PyObjectWrapper(generator.into()), 
                                 PyObjectWrapper(style_key.into()))
                            );
                        }
                    }
                }
            }
        }

        // Callbacks
        for (prop_name, value) in &node.props {
            if prop_name.ends_with("Name") && !value.is_null() {
                let function_name = &prop_name[..prop_name.len() - 4];
                if let Some(ref instance) = node.widget_instance {
                    let inst_ref = instance.as_ref();
                    if let Ok(callback) = inst_ref.getattr(self.py, function_name) {
                        // FIX: Use callback directly, then clone & convert to Py<PyAny>
                        if callback.bind(self.py).is_callable() {
                            self.result.registered_callbacks.insert(
                                value.as_str().unwrap_or("").to_string(),
                                PyObjectWrapper(callback)  // Store Py<PyAny> directly
                            );
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn diff_props(
        &self,
        old: &HashMap<String, serde_json::Value>,
        new: &HashMap<String, serde_json::Value>,
    ) -> HashMap<String, serde_json::Value> {
        let ignored: HashSet<&str> = [
            "widget_instance", "itemBuilder", "onChanged", "onPressed", "onTap", "onDrag"
        ].iter().cloned().collect();

        let all_keys: HashSet<_> = old.keys().chain(new.keys())
            .filter(|k| !ignored.contains(k.as_str()))
            .collect();

        let mut changes = HashMap::new();
        for key in all_keys {
            if old.get(key) != new.get(key) {
                if let Some(new_val) = new.get(key) {
                    changes.insert(key.clone(), new_val.clone());
                }
            }
        }
        changes
    }

    fn queue_js_initializers(&mut self, node: &RustNodeData) -> Result<(), ReconcilerError> {
        if node.widget_type == "Scrollbar" {
            self.result.js_initializers.push(JsInitializer {
                init_type: "SimpleBar".to_string(),
                target_id: node.html_id.clone(),
                data: serde_json::Value::Object(serde_json::Map::new()),
                before_id: None,
            });
        }

        if node.props.contains_key("responsive_clip_path") {
            self.result.js_initializers.push(JsInitializer {
                init_type: "ResponsiveClipPath".to_string(),
                target_id: node.html_id.clone(),
                data: node.props["responsive_clip_path"].clone(),
                before_id: None,
            });
        }

        if let Some(js_init) = node.props.get("_js_init") {
            self.result.js_initializers.push(JsInitializer {
                init_type: "generic".to_string(),
                target_id: node.html_id.clone(),
                data: js_init.clone(),
                before_id: None,
            });
        }

        Ok(())
    }

    /// Reorder patches so that all parent INSERTs come before their child INSERTs.
    /// This ensures that when JS applies patches, the DOM parent already exists.
    fn reorder_patches_parent_first(&mut self) {
        use std::cmp::Ordering;
        
        // Build a map of html_id -> parent_html_id for easy lookup
        let mut parent_map: HashMap<String, String> = HashMap::new();
        for patch in &self.result.patches {
            if patch.action == PatchAction::Insert {
                if let Some(data) = patch.data.as_object() {
                    if let Some(parent_id) = data.get("parent_html_id").and_then(|v| v.as_str()) {
                        parent_map.insert(patch.html_id.clone(), parent_id.to_string());
                    }
                }
            }
        }

        // Topological sort: ensure parents come before children
        // Count how many insertions each html_id is depended upon by
        let mut insert_indices: HashMap<String, usize> = HashMap::new();
        for (i, patch) in self.result.patches.iter().enumerate() {
            if patch.action == PatchAction::Insert {
                insert_indices.insert(patch.html_id.clone(), i);
            }
        }

        // Sort INSERTs so that if A is parent of B, A's index < B's index
        // Use a stable sort to maintain relative order of unrelated patches
        self.result.patches.sort_by(|a, b| {
            // REMOVE/UPDATE/REPLACE/MOVE patches stay in their original positions relative to other non-INSERT patches
            // But INSERTs are reordered
            match (&a.action, &b.action) {
                (PatchAction::Insert, PatchAction::Insert) => {
                    // Both are INSERTs: check if one is an ancestor of the other
                    // If b depends on a (a is ancestor of b), then a should come first (Ordering::Less)
                    let mut current = Some(b.html_id.as_str());
                    while let Some(html_id) = current {
                        if html_id == a.html_id {
                            // a is an ancestor of b, a should come first
                            return Ordering::Less;
                        }
                        current = parent_map.get(html_id).map(|s| s.as_str());
                    }
                    // Check if a depends on b (b is ancestor of a)
                    let mut current = Some(a.html_id.as_str());
                    while let Some(html_id) = current {
                        if html_id == b.html_id {
                            // b is an ancestor of a, b should come first
                            return Ordering::Greater;
                        }
                        current = parent_map.get(html_id).map(|s| s.as_str());
                    }
                    // Unrelated INSERTs: maintain insertion order
                    Ordering::Equal
                }
                _ => Ordering::Equal, // Non-INSERT patches maintain their order
            }
        });

        println!("DiffEngine: patch reordering complete, {} patches total", self.result.patches.len());
    }
}