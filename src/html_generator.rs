//! Complete HTML generation with consistent escaping and zero panics
use crate::errors::ReconcilerError;
use super::converters::json_to_pyobject;
use pyo3::prelude::*;
use pyo3::types::{PyString, PyList};
use std::collections::HashMap;
use phf::phf_map;

// Compile-time widget tag lookup (zero allocation)
static WIDGET_TAGS: phf::Map<&'static str, &'static str> = phf_map! {
    "Text" => "p",
    "Image" => "img",
    "Icon" => "i",
    "Spacer" => "div",
    "SizedBox" => "div",
    "TextButton" => "button",
    "ElevatedButton" => "button",
    "IconButton" => "button",
    "FloatingActionButton" => "button",
    "SnackBarAction" => "button",
    "ListTile" => "div",
    "Divider" => "div",
    "Dialog" => "div",
    "AspectRatio" => "div",
    "ClipPath" => "div",
    "Positioned" => "div",
};

/// Consistent HTML attribute escaping
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Generate HTML stub with comprehensive error handling
pub fn generate_html_stub<'py>(
    py: Python<'py>,
    widget: pyo3::Py<pyo3::PyAny>,
    html_id: &str,
    props: &HashMap<String, serde_json::Value>,
) -> Result<String, ReconcilerError> {
    let widget_bound = widget.bind(py);
    
    let widget_type_name = match widget_bound.get_type().name() {
        Ok(s) => s.to_string(),
        Err(_) => "unknown".to_string(),
    };
    
    if let Ok(generator) = widget_bound.get_type().getattr("_generate_html_stub") {
        let html_id_py = PyString::new(py, html_id);
        let props_py = json_to_pyobject(py, &serde_json::Value::Object(map_to_json_value(props)))?;
        return generator.call1((widget_bound, html_id_py, props_py))?
            .extract::<String>()
            .map_err(|e| ReconcilerError::HtmlGenerationError {
                widget_type: widget_type_name.clone(),
                details: e.to_string(),
            });
    }

    generate_generic_stub(py, widget, html_id, props)
}

/// Generic HTML stub generator with all widget logic
fn generate_generic_stub<'py>(
    py: Python<'py>,
    widget: pyo3::Py<pyo3::PyAny>,
    html_id: &str,
    props: &HashMap<String, serde_json::Value>,
) -> Result<String, ReconcilerError> {
    let widget_bound = widget.bind(py);
    
    let widget_type = match widget_bound.get_type().name() {
        Ok(s) => s.to_string(),
        Err(_) => "unknown".to_string(),
    };
    
    let tag = WIDGET_TAGS.get(widget_type.as_str()).unwrap_or(&"div");
    
    // Build classes string
    let mut classes = props.get("css_class")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Add required CSS classes
    if let Ok(method) = widget_bound.getattr("get_required_css_classes") {
        if let Ok(additional_any) = method.call0() {
            // FIX: Use .cast() instead of .cast_as()
            if let Ok(list) = additional_any.cast::<PyList>() {
                for item in list.iter() {
                    if !classes.is_empty() {
                        classes.push(' ');
                    }
                    classes.push_str(&item.extract::<String>().map_err(|e| ReconcilerError::TypeConversionError { expected: "String".into(), actual: e.to_string() })?);
                }
            }
        }
    }

    // FIX: Move variable declarations BEFORE the match
    let mut attrs = String::new();
    let mut inline_styles = Vec::new();

    // ===== WIDGET-SPECIFIC LOGIC =====
    match widget_type.as_str() {
        "Icon" => {
            if let Some(icon_name) = props.get("data").and_then(|v| v.as_str()) {
                if let Some(render_type) = props.get("render_type").and_then(|v| v.as_str()) {
                    if render_type == "img" {
                        let src = props.get("custom_icon_src")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        attrs.push_str(&format!(r#" src="{}""#, html_escape(src)));
                        return Ok(format!(r#"<img id="{}" class="{}" alt="{}">"#, 
                            html_id, classes, html_escape(icon_name)));
                    }
                }
                // Font Awesome
                return Ok(format!(r#"<i id="{}" class="{} {}"></i>"#, 
                    html_id, classes, html_escape(icon_name)));
            }
        }
        
        "Text" => {
            let text = props.get("data")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            return Ok(format!(r#"<p id="{}" class="{}">{}</p>"#, 
                html_id, classes, html_escape(text)));
        }
        
        "Image" => {
            if let Some(src) = props.get("src").and_then(|v| v.as_str()) {
                attrs.push_str(&format!(r#" src="{}""#, html_escape(src)));
            }
            attrs.push_str(r#" alt="""#);
        }
        
        "ClipPath" => {
            if let Some(width) = props.get("width").and_then(|v| v.as_str()) {
                inline_styles.push(format!("width: {}", width));
            }
            if let Some(height) = props.get("height").and_then(|v| v.as_str()) {
                inline_styles.push(format!("height: {}", height));
            }
            if let Some(clip_path) = props.get("clip_path_string").and_then(|v| v.as_str()) {
                inline_styles.push(format!("clip-path: {}", clip_path));
            }
            if let Some(ratio) = props.get("aspectRatio").and_then(|v| v.as_str()) {
                inline_styles.push(format!("aspect-ratio: {}", ratio));
            }
        }
        
        "SizedBox" => {
            if let Some(w) = props.get("width") {
                let width = if let Some(num) = w.as_f64() {
                    format!("{}px", num)
                } else {
                    w.as_str().unwrap_or("").to_string()
                };
                inline_styles.push(format!("width: {}", width));
            }
            if let Some(h) = props.get("height") {
                let height = if let Some(num) = h.as_f64() {
                    format!("{}px", num)
                } else {
                    h.as_str().unwrap_or("").to_string()
                };
                inline_styles.push(format!("height: {}", height));
            }
        }
        
        "Divider" => {
            inline_styles.push("width: 100%".to_string());
            if let Some(h) = props.get("height").and_then(|v| v.as_f64()) {
                inline_styles.push(format!("height: {}px", h));
            }
            if let Some(color) = props.get("color").and_then(|v| v.as_str()) {
                inline_styles.push(format!("background-color: {}", color));
            }
            if let Some(margin) = props.get("margin").and_then(|v| v.as_str()) {
                inline_styles.push(format!("margin: {}", margin));
            }
        }
        
        "AspectRatio" => {
            if let Some(ratio) = props.get("aspectRatio").and_then(|v| v.as_str()) {
                inline_styles.push(format!("aspect-ratio: {}", ratio));
            }
        }
        
        "Positioned" => {
            for prop in ["top", "bottom", "left", "right", "width", "height"] {
                if let Some(val) = props.get(prop).and_then(|v| v.as_str()) {
                    inline_styles.push(format!("{}: {}", prop, val));
                }
            }
        }
        
        _ => {}
    }

    // Generic style handling
    if let Some(style_dict) = props.get("style").and_then(|v| v.as_object()) {
        for (key, value) in style_dict {
            let css_key = key.replace('_', "-");
            let css_value = match value.as_str() {
                Some(s) => s.to_string(),
                None => value.to_string(),
            };
            inline_styles.push(format!("{}: {}", css_key, css_value));
        }
    }

    if let Some(pos) = props.get("position_type").and_then(|v| v.as_str()) {
        inline_styles.push(format!("position: {}", pos));
    }

    // Build style attribute
    if !inline_styles.is_empty() {
        attrs.push_str(&format!(r#" style="{}""#, inline_styles.join("; ")));
    }

    // Generic attributes
    if let Some(attr_dict) = props.get("attributes").and_then(|v| v.as_object()) {
        for (key, value) in attr_dict {
            attrs.push_str(&format!(
                r#" {}="{}""#,
                html_escape(key),
                html_escape(value.as_str().unwrap_or(&value.to_string()))
            ));
        }
    }

    // Event handlers
    if props.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true) {
        if let Some(cb_name) = props.get("onPressedName").and_then(|v| v.as_str()) {
            let has_args = props.get("onPressedArgs")
                .and_then(|v| v.as_array())
                .map(|arr| !arr.is_empty())
                .unwrap_or(false);
            
            if has_args {
                let args = props.get("onPressedArgs").unwrap();
                attrs.push_str(&format!(
                    r#" onclick="handleClickWithArgs('{}', '{}')""#,
                    html_escape(cb_name),
                    html_escape(&serde_json::to_string(args).unwrap_or_else(|_| "[]".to_string()))
                ));
            } else {
                attrs.push_str(&format!(
                    r#" onclick="handleClick('{}')""#,
                    html_escape(cb_name)
                ));
            }
        }
    }

    // Tooltip
    if let Some(tooltip) = props.get("tooltip").and_then(|v| v.as_str()) {
        attrs.push_str(&format!(
            r#" title="{}""#,
            html_escape(tooltip)
        ));
    }

    let is_void_element = ["img", "hr", "br"].contains(&tag);
    if is_void_element {
        Ok(format!(r#"<{tag} id="{id}" class="{classes}"{attrs}>"#,
            tag = tag,
            id = html_id,
            classes = classes,
            attrs = attrs
        ))
    } else {
        let inner_html = props.get("inner_html")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        Ok(format!(r#"<{tag} id="{id}" class="{classes}"{attrs}>{inner}</{tag}>"#,
            tag = tag,
            id = html_id,
            classes = classes,
            attrs = attrs,
            inner = html_escape(inner_html)
        ))
    }
}

pub(crate) fn map_to_json_value(map: &HashMap<String, serde_json::Value>) -> serde_json::Map<String, serde_json::Value> {
        map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}