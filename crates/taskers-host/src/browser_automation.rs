use std::{
    path::PathBuf,
    pin::Pin,
    time::{Duration, Instant},
};

use gtk::{gio, glib, prelude::*};
use serde_json::{Value as JsonValue, json};
use taskers_control::{
    BrowserControlCommand, BrowserGetCommand, BrowserPredicateCommand, BrowserTarget,
    BrowserWaitCondition, ControlError,
};
use webkit6::{
    SnapshotOptions, SnapshotRegion, WebsiteData, WebsiteDataManager, WebsiteDataTypes, prelude::*,
};

use crate::BrowserSurfaceHandle;

const HELPER_SOURCE_URI: &str = "taskers://browser-helper";

impl BrowserSurfaceHandle {
    pub async fn execute(&self, command: BrowserControlCommand) -> Result<JsonValue, ControlError> {
        match command {
            BrowserControlCommand::Navigate { url, .. } => {
                self.navigate(&url);
                Ok(json!({
                    "surface_id": self.surface_id().to_string(),
                    "status": "navigating",
                    "url": url,
                }))
            }
            BrowserControlCommand::Back { .. } => {
                self.go_back();
                Ok(json!({
                    "surface_id": self.surface_id().to_string(),
                    "status": "navigating_back",
                }))
            }
            BrowserControlCommand::Forward { .. } => {
                self.go_forward();
                Ok(json!({
                    "surface_id": self.surface_id().to_string(),
                    "status": "navigating_forward",
                }))
            }
            BrowserControlCommand::Reload { .. } => {
                self.reload();
                Ok(json!({
                    "surface_id": self.surface_id().to_string(),
                    "status": "reloading",
                }))
            }
            BrowserControlCommand::FocusWebview { .. } => {
                self.focus_webview();
                Ok(json!({
                    "surface_id": self.surface_id().to_string(),
                    "focused": true,
                }))
            }
            BrowserControlCommand::IsWebviewFocused { .. } => Ok(json!({
                "surface_id": self.surface_id().to_string(),
                "focused": self.is_webview_focused(),
            })),
            BrowserControlCommand::Snapshot { .. } => self.snapshot_payload().await,
            BrowserControlCommand::Eval { script, .. } => self.eval_script(&script).await,
            BrowserControlCommand::Wait {
                condition,
                timeout_ms,
                poll_interval_ms,
                ..
            } => self.wait_for(condition, timeout_ms, poll_interval_ms).await,
            BrowserControlCommand::Click {
                target,
                snapshot_after,
                ..
            } => {
                self.run_helper_action("click", Some(target), json!({}), snapshot_after)
                    .await
            }
            BrowserControlCommand::Dblclick {
                target,
                snapshot_after,
                ..
            } => {
                self.run_helper_action("dblclick", Some(target), json!({}), snapshot_after)
                    .await
            }
            BrowserControlCommand::Type {
                target,
                text,
                snapshot_after,
                ..
            } => {
                self.run_helper_action(
                    "type",
                    Some(target),
                    json!({ "text": text }),
                    snapshot_after,
                )
                .await
            }
            BrowserControlCommand::Fill {
                target,
                text,
                snapshot_after,
                ..
            } => {
                self.run_helper_action(
                    "fill",
                    Some(target),
                    json!({ "text": text }),
                    snapshot_after,
                )
                .await
            }
            BrowserControlCommand::Press {
                target,
                key,
                snapshot_after,
                ..
            } => {
                self.run_helper_action("press", target, json!({ "key": key }), snapshot_after)
                    .await
            }
            BrowserControlCommand::Keydown {
                target,
                key,
                snapshot_after,
                ..
            } => {
                self.run_helper_action("keydown", target, json!({ "key": key }), snapshot_after)
                    .await
            }
            BrowserControlCommand::Keyup {
                target,
                key,
                snapshot_after,
                ..
            } => {
                self.run_helper_action("keyup", target, json!({ "key": key }), snapshot_after)
                    .await
            }
            BrowserControlCommand::Hover {
                target,
                snapshot_after,
                ..
            } => {
                self.run_helper_action("hover", Some(target), json!({}), snapshot_after)
                    .await
            }
            BrowserControlCommand::Focus {
                target,
                snapshot_after,
                ..
            } => {
                self.run_helper_action("focus", Some(target), json!({}), snapshot_after)
                    .await
            }
            BrowserControlCommand::Check {
                target,
                snapshot_after,
                ..
            } => {
                self.run_helper_action("check", Some(target), json!({}), snapshot_after)
                    .await
            }
            BrowserControlCommand::Uncheck {
                target,
                snapshot_after,
                ..
            } => {
                self.run_helper_action("uncheck", Some(target), json!({}), snapshot_after)
                    .await
            }
            BrowserControlCommand::Select {
                target,
                values,
                snapshot_after,
                ..
            } => {
                self.run_helper_action(
                    "select",
                    Some(target),
                    json!({ "values": values }),
                    snapshot_after,
                )
                .await
            }
            BrowserControlCommand::Scroll {
                target,
                dx,
                dy,
                snapshot_after,
                ..
            } => {
                self.run_helper_action(
                    "scroll",
                    target,
                    json!({ "dx": dx, "dy": dy }),
                    snapshot_after,
                )
                .await
            }
            BrowserControlCommand::ScrollIntoView {
                target,
                snapshot_after,
                ..
            } => {
                self.run_helper_action("scroll_into_view", Some(target), json!({}), snapshot_after)
                    .await
            }
            BrowserControlCommand::Get { query, .. } => self.run_get(query).await,
            BrowserControlCommand::Is { query, .. } => self.run_predicate(query).await,
            BrowserControlCommand::Screenshot {
                path,
                full_document,
                ..
            } => self.screenshot(path, full_document).await,
            BrowserControlCommand::ClearData {
                origin_filter,
                reload,
                ..
            } => self.clear_data(origin_filter, reload).await,
        }
    }

    async fn snapshot_payload(&self) -> Result<JsonValue, ControlError> {
        let snapshot = self
            .run_helper(json!({
                "action": "snapshot",
            }))
            .await?;
        Ok(json!({
            "surface_id": self.surface_id().to_string(),
            "url": self.url(),
            "title": self.title(),
            "loading": self.is_loading(),
            "snapshot": snapshot,
        }))
    }

    async fn eval_script(&self, script: &str) -> Result<JsonValue, ControlError> {
        let body = format!(
            r#"
const __taskersEval = {};
return await Promise.resolve((0, eval)(__taskersEval));
"#,
            serde_json::to_string(script)
                .map_err(|error| ControlError::invalid_params(error.to_string()))?
        );
        let value = self
            .webview()
            .call_async_javascript_function_future(
                &body,
                None::<&glib::Variant>,
                None,
                Some(HELPER_SOURCE_URI),
            )
            .await
            .map_err(map_webkit_error)?;
        Ok(json!({
            "surface_id": self.surface_id().to_string(),
            "result": jsc_value_to_json(&value)?,
        }))
    }

    async fn wait_for(
        &self,
        condition: BrowserWaitCondition,
        timeout_ms: u64,
        poll_interval_ms: u64,
    ) -> Result<JsonValue, ControlError> {
        let timeout = Duration::from_millis(timeout_ms.max(1));
        let poll = Duration::from_millis(poll_interval_ms.max(10));
        let deadline = Instant::now() + timeout;

        loop {
            if self.wait_condition_satisfied(&condition).await? {
                return Ok(json!({
                    "surface_id": self.surface_id().to_string(),
                    "status": "matched",
                    "condition": encode_wait_condition(&condition),
                }));
            }

            if Instant::now() >= deadline {
                return Err(ControlError::timeout(format!(
                    "browser wait timed out after {} ms",
                    timeout_ms.max(1)
                )));
            }

            glib::timeout_future(poll).await;
        }
    }

    async fn wait_condition_satisfied(
        &self,
        condition: &BrowserWaitCondition,
    ) -> Result<bool, ControlError> {
        match condition {
            BrowserWaitCondition::Delay { duration_ms } => {
                glib::timeout_future(Duration::from_millis(*duration_ms)).await;
                Ok(true)
            }
            BrowserWaitCondition::Selector { selector } => {
                let result = self
                    .run_helper(json!({
                        "action": "probe_selector",
                        "selector": selector,
                    }))
                    .await?;
                Ok(result
                    .get("matched")
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(false))
            }
            BrowserWaitCondition::Text { text } => {
                let result = self
                    .run_helper(json!({
                        "action": "probe_text",
                        "text": text,
                    }))
                    .await?;
                Ok(result
                    .get("matched")
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(false))
            }
            BrowserWaitCondition::UrlMatches { pattern } => Ok(self.url().contains(pattern)),
            BrowserWaitCondition::LoadState { state } => Ok(self.load_state() == Some(*state)),
            BrowserWaitCondition::Function { script } => {
                let result = self
                    .run_helper(json!({
                        "action": "probe_function",
                        "script": script,
                    }))
                    .await?;
                Ok(result
                    .get("matched")
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(false))
            }
        }
    }

    pub(crate) async fn clear_data(
        &self,
        origin_filter: Option<String>,
        reload: bool,
    ) -> Result<JsonValue, ControlError> {
        let manager = self
            .network_session
            .website_data_manager()
            .ok_or_else(|| ControlError::not_supported("browser data manager is unavailable"))?;
        let data_types = WebsiteDataTypes::ALL;
        let website_data = manager
            .fetch_future(data_types)
            .await
            .map_err(map_webkit_error)?;
        let normalized_filter = origin_filter
            .as_deref()
            .and_then(normalize_origin_filter)
            .map(str::to_string);
        let matching = website_data
            .into_iter()
            .filter(|entry| website_data_matches(entry, normalized_filter.as_deref()))
            .collect::<Vec<_>>();
        if !matching.is_empty() {
            website_data_manager_remove_future(&manager, data_types, matching.clone())
                .await
                .map_err(map_webkit_error)?;
        }
        if reload {
            self.reload();
        }
        Ok(json!({
            "surface_id": self.surface_id().to_string(),
            "status": "browser_data_cleared",
            "profile_mode": self.profile_mode,
            "origin_filter": normalized_filter,
            "cleared_entries": matching.len(),
            "reloaded": reload,
        }))
    }

    async fn run_helper_action(
        &self,
        action: &'static str,
        target: Option<BrowserTarget>,
        extra: JsonValue,
        snapshot_after: bool,
    ) -> Result<JsonValue, ControlError> {
        let mut command = json!({
            "action": action,
            "snapshot_after": snapshot_after,
        });
        if let Some(target) = target {
            command["target"] = encode_target(target);
        }
        merge_json_object(&mut command, extra);
        self.run_helper(command).await
    }

    async fn run_get(&self, query: BrowserGetCommand) -> Result<JsonValue, ControlError> {
        if matches!(
            &query,
            BrowserGetCommand::Styles { properties, .. } if properties.is_empty()
        ) {
            return Err(ControlError::invalid_params(
                "browser get styles requires at least one property",
            ));
        }
        let command = match query {
            BrowserGetCommand::Url => {
                return Ok(json!({
                    "surface_id": self.surface_id().to_string(),
                    "kind": "url",
                    "value": self.url(),
                }));
            }
            BrowserGetCommand::Title => {
                return Ok(json!({
                    "surface_id": self.surface_id().to_string(),
                    "kind": "title",
                    "value": self.title(),
                }));
            }
            BrowserGetCommand::Text { target } => {
                json!({"action": "get_text", "target": encode_target(target)})
            }
            BrowserGetCommand::Html { target } => {
                json!({"action": "get_html", "target": encode_target(target)})
            }
            BrowserGetCommand::Value { target } => {
                json!({"action": "get_value", "target": encode_target(target)})
            }
            BrowserGetCommand::Attr { target, name } => {
                json!({"action": "get_attr", "target": encode_target(target), "name": name})
            }
            BrowserGetCommand::Count { selector } => {
                json!({"action": "get_count", "selector": selector})
            }
            BrowserGetCommand::Box { target } => {
                json!({"action": "get_box", "target": encode_target(target)})
            }
            BrowserGetCommand::Styles { target, properties } => {
                json!({
                    "action": "get_styles",
                    "target": encode_target(target),
                    "properties": properties,
                })
            }
        };
        self.run_helper(command).await
    }

    async fn run_predicate(
        &self,
        query: BrowserPredicateCommand,
    ) -> Result<JsonValue, ControlError> {
        let command = match query {
            BrowserPredicateCommand::Visible { target } => {
                json!({"action": "is_visible", "target": encode_target(target)})
            }
            BrowserPredicateCommand::Enabled { target } => {
                json!({"action": "is_enabled", "target": encode_target(target)})
            }
            BrowserPredicateCommand::Checked { target } => {
                json!({"action": "is_checked", "target": encode_target(target)})
            }
        };
        self.run_helper(command).await
    }

    async fn screenshot(
        &self,
        path: Option<String>,
        full_document: bool,
    ) -> Result<JsonValue, ControlError> {
        let region = if full_document {
            SnapshotRegion::FullDocument
        } else {
            SnapshotRegion::Visible
        };
        let texture = self
            .webview()
            .snapshot_future(region, SnapshotOptions::NONE)
            .await
            .map_err(map_webkit_error)?;
        let output_path = screenshot_path(path)?;
        texture
            .save_to_png(&output_path)
            .map_err(|error| ControlError::internal(error.to_string()))?;
        Ok(json!({
            "surface_id": self.surface_id().to_string(),
            "path": output_path.display().to_string(),
            "width": texture.width(),
            "height": texture.height(),
            "full_document": full_document,
        }))
    }

    async fn run_helper(&self, command: JsonValue) -> Result<JsonValue, ControlError> {
        let body = format!(
            "{}\nreturn await globalThis.__taskersBrowserHelper.run({});",
            helper_bootstrap_source(),
            serde_json::to_string(&command)
                .map_err(|error| ControlError::invalid_params(error.to_string()))?,
        );
        let value = self
            .webview()
            .call_async_javascript_function_future(
                &body,
                None::<&glib::Variant>,
                None,
                Some(HELPER_SOURCE_URI),
            )
            .await
            .map_err(map_webkit_error)?;
        let payload = jsc_value_to_json(&value)?;
        decode_helper_response(payload)
    }
}

fn encode_target(target: BrowserTarget) -> JsonValue {
    match target {
        BrowserTarget::Ref { value } => json!({ "kind": "ref", "value": value }),
        BrowserTarget::Selector { value } => json!({ "kind": "selector", "value": value }),
    }
}

fn encode_wait_condition(condition: &BrowserWaitCondition) -> JsonValue {
    match condition {
        BrowserWaitCondition::Selector { selector } => {
            json!({ "kind": "selector", "selector": selector })
        }
        BrowserWaitCondition::Text { text } => json!({ "kind": "text", "text": text }),
        BrowserWaitCondition::UrlMatches { pattern } => {
            json!({ "kind": "url_matches", "pattern": pattern })
        }
        BrowserWaitCondition::LoadState { state } => {
            json!({ "kind": "load_state", "state": format!("{state:?}").to_lowercase() })
        }
        BrowserWaitCondition::Function { script } => {
            json!({ "kind": "function", "script": script })
        }
        BrowserWaitCondition::Delay { duration_ms } => {
            json!({ "kind": "delay", "duration_ms": duration_ms })
        }
    }
}

fn decode_helper_response(payload: JsonValue) -> Result<JsonValue, ControlError> {
    let Some(ok) = payload.get("ok").and_then(JsonValue::as_bool) else {
        return Err(ControlError::internal(
            "browser helper returned an invalid response",
        ));
    };
    if ok {
        Ok(payload.get("result").cloned().unwrap_or(JsonValue::Null))
    } else {
        let code = payload
            .get("code")
            .and_then(JsonValue::as_str)
            .unwrap_or("internal");
        let message = payload
            .get("message")
            .and_then(JsonValue::as_str)
            .unwrap_or("browser helper failed");
        Err(match code {
            "invalid_params" => ControlError::invalid_params(message),
            "not_found" => ControlError::not_found(message),
            "timeout" => ControlError::timeout(message),
            "invalid_state" => ControlError::invalid_state(message),
            "not_supported" => ControlError::not_supported(message),
            _ => ControlError::internal(message),
        })
    }
}

fn jsc_value_to_json(value: &webkit6::javascriptcore::Value) -> Result<JsonValue, ControlError> {
    if let Some(payload) = value.to_json(0) {
        serde_json::from_str(payload.as_str())
            .map_err(|error| ControlError::internal(error.to_string()))
    } else if value.is_boolean() {
        Ok(json!(value.to_boolean()))
    } else if value.is_number() {
        Ok(json!(value.to_double()))
    } else if value.is_string() {
        Ok(json!(value.to_str().to_string()))
    } else {
        Ok(json!({
            "result_type": "string",
            "text": value.to_str().to_string(),
        }))
    }
}

fn map_webkit_error(error: glib::Error) -> ControlError {
    ControlError::invalid_state(error.to_string())
}

fn merge_json_object(target: &mut JsonValue, extra: JsonValue) {
    let Some(target_object) = target.as_object_mut() else {
        return;
    };
    let Some(extra_object) = extra.as_object() else {
        return;
    };
    for (key, value) in extra_object {
        target_object.insert(key.clone(), value.clone());
    }
}

fn screenshot_path(path: Option<String>) -> Result<PathBuf, ControlError> {
    match path {
        Some(path) => Ok(PathBuf::from(path)),
        None => {
            let timestamp = glib::DateTime::now_local()
                .map_err(|error| ControlError::internal(error.to_string()))?
                .format("%Y%m%d-%H%M%S")
                .map_err(|error| ControlError::internal(error.to_string()))?;
            Ok(std::env::temp_dir().join(format!("taskers-browser-{}.png", timestamp)))
        }
    }
}

fn website_data_manager_remove_future(
    manager: &WebsiteDataManager,
    data_types: WebsiteDataTypes,
    website_data: Vec<WebsiteData>,
) -> Pin<Box<dyn std::future::Future<Output = Result<(), glib::Error>> + 'static>> {
    let manager = manager.clone();
    Box::pin(gio::GioFuture::new(
        &manager,
        move |obj, cancellable, send| {
            let refs = website_data.iter().collect::<Vec<_>>();
            obj.remove(data_types, &refs, Some(cancellable), move |res| {
                send.resolve(res);
            });
        },
    ))
}

fn normalize_origin_filter(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed);
    let host = without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .split(':')
        .next()
        .unwrap_or(without_scheme)
        .trim_matches('.');
    (!host.is_empty()).then_some(host)
}

fn website_data_matches(entry: &WebsiteData, origin_filter: Option<&str>) -> bool {
    if origin_filter.is_none() {
        return true;
    }
    let Some(name) = entry.name() else {
        return false;
    };
    website_data_name_matches(&name, origin_filter)
}

fn website_data_name_matches(name: &str, origin_filter: Option<&str>) -> bool {
    let Some(origin_filter) = origin_filter.map(|value| value.to_ascii_lowercase()) else {
        return true;
    };
    let name = name.to_ascii_lowercase();
    name == origin_filter || name.ends_with(&format!(".{origin_filter}"))
}

fn helper_bootstrap_source() -> &'static str {
    r#"
if (!globalThis.__taskersBrowserHelper) {
  globalThis.__taskersBrowserHelper = (() => {
    let refCounter = 0;
    let refTable = new Map();

    function ok(result) {
      return { ok: true, result };
    }

    function fail(code, message) {
      return { ok: false, code, message };
    }

    function resetRefs() {
      refCounter = 0;
      refTable = new Map();
    }

    function nextRef(element) {
      const ref = `@e${++refCounter}`;
      refTable.set(ref, element);
      return ref;
    }

    function roleFor(element) {
      if (!(element instanceof Element)) {
        return "node";
      }
      const explicit = element.getAttribute("role");
      if (explicit) {
        return explicit;
      }
      const tag = element.tagName.toLowerCase();
      if (tag === "a" && element.hasAttribute("href")) return "link";
      if (tag === "button") return "button";
      if (tag === "input") {
        const type = (element.getAttribute("type") || "text").toLowerCase();
        if (type === "checkbox") return "checkbox";
        if (type === "radio") return "radio";
        if (type === "submit" || type === "button") return "button";
        return "textbox";
      }
      if (tag === "textarea") return "textbox";
      if (tag === "select") return "combobox";
      if (tag === "option") return "option";
      if (tag === "img") return "img";
      if (tag === "form") return "form";
      return tag;
    }

    function isVisible(element) {
      if (!(element instanceof Element)) {
        return false;
      }
      const style = window.getComputedStyle(element);
      if (style.display === "none" || style.visibility === "hidden" || Number(style.opacity) === 0) {
        return false;
      }
      const rect = element.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0;
    }

    function isEnabled(element) {
      return !(element instanceof HTMLButtonElement
        || element instanceof HTMLInputElement
        || element instanceof HTMLSelectElement
        || element instanceof HTMLTextAreaElement)
        ? element.getAttribute("aria-disabled") !== "true"
        : !element.disabled;
    }

    function elementName(element) {
      if (!(element instanceof Element)) {
        return "";
      }
      const labelledBy = element.getAttribute("aria-labelledby");
      if (labelledBy) {
        const label = labelledBy
          .split(/\s+/)
          .map((id) => document.getElementById(id))
          .filter(Boolean)
          .map((node) => node.textContent || "")
          .join(" ")
          .trim();
        if (label) return label;
      }
      const ariaLabel = element.getAttribute("aria-label");
      if (ariaLabel) return ariaLabel.trim();
      if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement || element instanceof HTMLSelectElement) {
        if (element.labels && element.labels.length > 0) {
          const labelText = Array.from(element.labels)
            .map((label) => label.textContent || "")
            .join(" ")
            .trim();
          if (labelText) return labelText;
        }
      }
      return (
        element.getAttribute("title") ||
        element.getAttribute("alt") ||
        element.getAttribute("placeholder") ||
        element.textContent ||
        ""
      ).trim();
    }

    function nodeValue(element) {
      if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement || element instanceof HTMLSelectElement) {
        return element.value;
      }
      return null;
    }

    function nodeText(element) {
      if (!(element instanceof Element)) {
        return "";
      }
      return (element.innerText || element.textContent || "").trim();
    }

    function bounds(element) {
      const rect = element.getBoundingClientRect();
      return {
        x: Math.round(rect.x),
        y: Math.round(rect.y),
        width: Math.round(rect.width),
        height: Math.round(rect.height),
      };
    }

    function snapshotNode(element) {
      const ref = nextRef(element);
      return {
        ref,
        role: roleFor(element),
        name: elementName(element) || null,
        text: nodeText(element) || null,
        value: nodeValue(element),
        checked: "checked" in element ? Boolean(element.checked) : null,
        selected: "selected" in element ? Boolean(element.selected) : null,
        disabled: !isEnabled(element),
        visible: isVisible(element),
        bounds: bounds(element),
        children: Array.from(element.children).map(snapshotNode),
      };
    }

    function resolveTarget(target) {
      if (!target || typeof target !== "object") {
        throw fail("invalid_params", "browser action requires a target");
      }
      if (target.kind === "ref") {
        const element = refTable.get(target.value);
        if (!element) {
          throw fail("invalid_state", `browser ref ${target.value} is no longer valid`);
        }
        return element;
      }
      if (target.kind === "selector") {
        const element = document.querySelector(target.value);
        if (!element) {
          throw fail("not_found", `selector not found: ${target.value}`);
        }
        return element;
      }
      throw fail("invalid_params", "unknown browser target");
    }

    function resolveOptionalTarget(target) {
      if (!target) {
        return document.activeElement || document.body || document.documentElement;
      }
      return resolveTarget(target);
    }

    function ensureEditable(element) {
      if (
        element instanceof HTMLInputElement ||
        element instanceof HTMLTextAreaElement ||
        element instanceof HTMLSelectElement ||
        element.isContentEditable
      ) {
        return element;
      }
      throw fail("invalid_state", "target element is not editable");
    }

    function dispatchInput(element) {
      element.dispatchEvent(new Event("input", { bubbles: true, cancelable: true }));
      element.dispatchEvent(new Event("change", { bubbles: true, cancelable: true }));
    }

    function writeText(element, text, append) {
      ensureEditable(element);
      element.focus();
      if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
        element.value = append ? `${element.value}${text}` : text;
        dispatchInput(element);
        return;
      }
      if (element instanceof HTMLSelectElement) {
        element.value = text;
        dispatchInput(element);
        return;
      }
      if (element.isContentEditable) {
        element.textContent = append ? `${element.textContent || ""}${text}` : text;
        dispatchInput(element);
        return;
      }
      throw fail("invalid_state", "target element cannot accept text");
    }

    function dispatchKeyboard(element, type, key) {
      element.dispatchEvent(new KeyboardEvent(type, {
        key,
        bubbles: true,
        cancelable: true,
      }));
    }

    function maybeInsertKey(element, key) {
      if (key.length !== 1) {
        return;
      }
      if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
        element.value = `${element.value}${key}`;
        dispatchInput(element);
        return;
      }
      if (element.isContentEditable) {
        element.textContent = `${element.textContent || ""}${key}`;
        dispatchInput(element);
      }
    }

    function actionResult(snapshotAfter) {
      if (!snapshotAfter) {
        return {};
      }
      return { post_action_snapshot: snapshot() };
    }

    function snapshot() {
      resetRefs();
      const root = document.body || document.documentElement;
      return snapshotNode(root);
    }

    async function run(command) {
      try {
        switch (command.action) {
          case "snapshot":
            return ok(snapshot());
          case "probe_selector":
            return ok({ matched: Boolean(document.querySelector(command.selector)) });
          case "probe_text": {
            const haystack = (document.body?.innerText || document.documentElement?.innerText || "");
            return ok({ matched: haystack.includes(command.text) });
          }
          case "probe_function": {
            const result = await Promise.resolve((0, eval)(command.script));
            return ok({ matched: Boolean(result) });
          }
          case "click": {
            const element = resolveTarget(command.target);
            element.click();
            return ok(actionResult(command.snapshot_after));
          }
          case "dblclick": {
            const element = resolveTarget(command.target);
            element.dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true }));
            return ok(actionResult(command.snapshot_after));
          }
          case "hover": {
            const element = resolveTarget(command.target);
            element.dispatchEvent(new MouseEvent("mouseover", { bubbles: true, cancelable: true }));
            element.dispatchEvent(new MouseEvent("mousemove", { bubbles: true, cancelable: true }));
            return ok(actionResult(command.snapshot_after));
          }
          case "focus": {
            const element = resolveTarget(command.target);
            element.focus();
            return ok(actionResult(command.snapshot_after));
          }
          case "type": {
            const element = resolveTarget(command.target);
            writeText(element, command.text, true);
            return ok(actionResult(command.snapshot_after));
          }
          case "fill": {
            const element = resolveTarget(command.target);
            writeText(element, command.text, false);
            return ok(actionResult(command.snapshot_after));
          }
          case "press":
          case "keydown":
          case "keyup": {
            const element = resolveOptionalTarget(command.target);
            element.focus();
            if (command.action === "press" || command.action === "keydown") {
              dispatchKeyboard(element, "keydown", command.key);
            }
            if (command.action === "press") {
              maybeInsertKey(element, command.key);
              dispatchKeyboard(element, "keyup", command.key);
            }
            if (command.action === "keyup") {
              dispatchKeyboard(element, "keyup", command.key);
            }
            return ok(actionResult(command.snapshot_after));
          }
          case "check":
          case "uncheck": {
            const element = resolveTarget(command.target);
            if (!(element instanceof HTMLInputElement) || (element.type !== "checkbox" && element.type !== "radio")) {
              throw fail("invalid_state", "target element is not a checkbox or radio input");
            }
            element.checked = command.action === "check";
            dispatchInput(element);
            return ok(actionResult(command.snapshot_after));
          }
          case "select": {
            const element = resolveTarget(command.target);
            if (!(element instanceof HTMLSelectElement)) {
              throw fail("invalid_state", "target element is not a select");
            }
            const wanted = new Set(command.values || []);
            let matched = false;
            for (const option of Array.from(element.options)) {
              const shouldSelect = wanted.has(option.value) || wanted.has(option.text);
              option.selected = shouldSelect;
              matched = matched || shouldSelect;
            }
            if (!matched && wanted.size > 0) {
              throw fail("not_found", "no matching select option found");
            }
            dispatchInput(element);
            return ok(actionResult(command.snapshot_after));
          }
          case "scroll": {
            if (command.target) {
              const element = resolveTarget(command.target);
              element.scrollBy(command.dx || 0, command.dy || 0);
            } else {
              window.scrollBy(command.dx || 0, command.dy || 0);
            }
            return ok(actionResult(command.snapshot_after));
          }
          case "scroll_into_view": {
            const element = resolveTarget(command.target);
            element.scrollIntoView({ block: "center", inline: "center" });
            return ok(actionResult(command.snapshot_after));
          }
          case "get_text": {
            const element = resolveTarget(command.target);
            return ok({ value: nodeText(element) });
          }
          case "get_html": {
            const element = resolveTarget(command.target);
            return ok({ value: element.outerHTML });
          }
          case "get_value": {
            const element = resolveTarget(command.target);
            return ok({ value: nodeValue(element) });
          }
          case "get_attr": {
            const element = resolveTarget(command.target);
            return ok({ value: element.getAttribute(command.name) });
          }
          case "get_count":
            return ok({ value: document.querySelectorAll(command.selector).length });
          case "get_box": {
            const element = resolveTarget(command.target);
            return ok({ value: bounds(element) });
          }
          case "get_styles": {
            const element = resolveTarget(command.target);
            const computed = window.getComputedStyle(element);
            const styles = {};
            for (const property of command.properties || []) {
              styles[property] = computed.getPropertyValue(property);
            }
            return ok({ value: styles });
          }
          case "is_visible": {
            const element = resolveTarget(command.target);
            return ok({ value: isVisible(element) });
          }
          case "is_enabled": {
            const element = resolveTarget(command.target);
            return ok({ value: isEnabled(element) });
          }
          case "is_checked": {
            const element = resolveTarget(command.target);
            return ok({ value: Boolean("checked" in element ? element.checked : false) });
          }
          default:
            return fail("not_supported", `unsupported browser helper action: ${command.action}`);
        }
      } catch (error) {
        if (error && typeof error === "object" && "ok" in error && error.ok === false) {
          return error;
        }
        const message = error instanceof Error ? error.message : String(error);
        return fail("internal", message);
      }
    }

    return { run };
  })();
}

#[cfg(test)]
mod tests {
    use super::{normalize_origin_filter, website_data_name_matches};

    #[test]
    fn normalize_origin_filter_strips_scheme_path_port_and_dots() {
        assert_eq!(
            normalize_origin_filter("https://foo.example.com:8443/path/to/page/"),
            Some("foo.example.com")
        );
        assert_eq!(normalize_origin_filter("example.com."), Some("example.com"));
        assert_eq!(normalize_origin_filter("   "), None);
    }

    #[test]
    fn website_data_name_matches_only_exact_host_or_subdomains() {
        assert!(website_data_name_matches("foo.example.com", Some("example.com")));
        assert!(website_data_name_matches("bar.foo.example.com", Some("foo.example.com")));
        assert!(website_data_name_matches("foo.example.com", Some("foo.example.com")));
        assert!(!website_data_name_matches("example.com", Some("foo.example.com")));
        assert!(!website_data_name_matches("evil-example.com", Some("example.com")));
        assert!(website_data_name_matches("Anything", None));
    }
}
"#
}
