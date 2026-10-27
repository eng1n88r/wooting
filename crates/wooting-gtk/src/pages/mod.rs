pub mod actuation;
pub mod advanced;
pub mod gamepad;
pub mod profiles;
pub mod quick;
pub mod rapid;
pub mod remap;
pub mod rgb;
pub mod settings;

use crate::app::Ctx;
use gtk::prelude::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Shared loader for the actuation/rapid-trigger overlays: fills per-key
/// values ("1.60") and marks keys with rapid trigger on.
pub async fn load_values_overlay(ctx: &Ctx) -> Option<(Value, Value, Value)> {
    let settings = ctx.call("settings.get", json!({})).await?;
    let per_key = ctx.call("actuation.keys.get", json!({})).await?;
    let rt = ctx.call("rapid_trigger.keys.get", json!({})).await?;
    let global = settings["actuation_mm"].as_f64().unwrap_or(1.6);
    let rt_all = settings["rapid_trigger"].as_bool().unwrap_or(false);
    let overrides: HashMap<String, f64> = per_key["keys"]
        .as_object()
        .map(|m| m.iter().filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f))).collect())
        .unwrap_or_default();
    let rt_keys: HashSet<String> = rt["keys"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_owned)).collect();
    let mut st = ctx.view.state.borrow_mut();
    st.values.clear();
    st.marked.clear();
    if let Some(layout) = st.layout.clone() {
        for k in &layout.keys {
            let mm = overrides.get(&k.id).copied().unwrap_or(global);
            st.values.insert(k.id.clone(), format!("{mm:.2}"));
            if rt_all || rt_keys.contains(&k.id) {
                st.marked.insert(k.id.clone());
            }
        }
    }
    drop(st);
    ctx.view.area.queue_draw();
    Some((settings, per_key, rt))
}
