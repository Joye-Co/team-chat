//! The page-to-host bridge: request parsing, capability checks and reply scripts.

use serde_json::{Value, json};

/// What a window lets its page ask for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// `getData`: read the 产出's JSON data.
    pub get_data: bool,
    /// `emitEvent`: pass an event to the host callback.
    pub emit_event: bool,
}

impl Capabilities {
    pub const NONE: Self = Self {
        get_data: false,
        emit_event: false,
    };
    pub const ALL: Self = Self {
        get_data: true,
        emit_event: true,
    };
}

/// Largest accepted bridge message (serialized JSON).
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;
const MAX_EVENT_NAME: usize = 128;

/// The result of one bridge request.
#[derive(Debug, PartialEq)]
pub struct Outcome {
    /// JSON for the page: `{id, result}` or `{id, error: {code, message}}`.
    pub reply: Value,
    /// For `emitEvent`: the event name and payload to give to the host.
    pub event: Option<(String, Value)>,
}

fn err(id: Value, code: &str, message: &str) -> Outcome {
    Outcome {
        reply: json!({ "id": id, "error": { "code": code, "message": message } }),
        event: None,
    }
}

/// Handles one raw message (`{ id, method, params }`) from the page.
pub fn handle(raw: &[u8], caps: Capabilities, data: &Value) -> Outcome {
    if raw.len() > MAX_MESSAGE_BYTES {
        return err(Value::Null, "too_large", "message too large");
    }
    let Ok(Value::Object(msg)) = serde_json::from_slice::<Value>(raw) else {
        return err(Value::Null, "bad_request", "message must be an object");
    };
    let id = match msg.get("id") {
        Some(v @ (Value::String(_) | Value::Number(_))) => v.clone(),
        _ => return err(Value::Null, "bad_request", "id must be a string or number"),
    };
    if let Value::String(s) = &id
        && s.len() > 128
    {
        return err(Value::Null, "bad_request", "id too long");
    }
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        return err(id, "bad_request", "method must be a string");
    };
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    match method {
        "getData" if caps.get_data => Outcome {
            reply: json!({ "id": id, "result": data }),
            event: None,
        },
        "emitEvent" if caps.emit_event => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return err(id, "bad_request", "params.name must be a string");
            };
            if name.is_empty() || name.len() > MAX_EVENT_NAME {
                return err(id, "bad_request", "params.name length");
            }
            let payload = params.get("payload").cloned().unwrap_or(Value::Null);
            Outcome {
                reply: json!({ "id": id, "result": true }),
                event: Some((name.to_string(), payload)),
            }
        }
        "getData" | "emitEvent" => err(id, "not_allowed", "capability not granted"),
        _ => err(id, "unknown_method", "unknown method"),
    }
}

/// The script that delivers a reply to the page. `reply` is serialized by serde_json, which
/// yields a valid JavaScript literal, so page data cannot break out of the expression.
pub fn reply_script(reply: &Value) -> String {
    format!(
        "window.dispatchEvent(new CustomEvent('artifactbridgereply',{{detail:{reply}}}));void 0"
    )
}

/// Injected at document start in the main frame: `window.artifact.getData()`,
/// `window.artifact.emitEvent(name, payload)` and `window.artifact.call(method, params)`,
/// all returning promises. Pages may also post raw messages and listen to
/// `artifactbridgereply` events.
pub const PAGE_SCRIPT: &str = r#"(function(){
  var h = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.bridge;
  if (!h) return;
  var n = 0, pending = new Map();
  window.addEventListener('artifactbridgereply', function (e) {
    var d = e.detail || {}, p = pending.get(d.id);
    if (!p) return;
    pending.delete(d.id);
    if (d.error) { var x = new Error(d.error.message); x.code = d.error.code; p.reject(x); }
    else p.resolve(d.result);
  });
  function call(method, params) {
    return new Promise(function (resolve, reject) {
      var id = '__ab' + (++n);
      pending.set(id, { resolve: resolve, reject: reject });
      try { h.postMessage({ id: id, method: method, params: params === undefined ? null : params }); }
      catch (e) { pending.delete(id); reject(e); }
    });
  }
  Object.defineProperty(window, 'artifact', { value: Object.freeze({
    call: call,
    getData: function () { return call('getData'); },
    emitEvent: function (name, payload) { return call('emitEvent', { name: name, payload: payload }); }
  }) });
})();"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn methods_and_capabilities() {
        let data = json!({"a": 1});
        let o = handle(br#"{"id":1,"method":"getData"}"#, Capabilities::ALL, &data);
        assert_eq!(o.reply, json!({"id":1,"result":{"a":1}}));
        let o = handle(
            br#"{"id":"x","method":"getData"}"#,
            Capabilities::NONE,
            &data,
        );
        assert_eq!(o.reply["error"]["code"], "not_allowed");
        let o = handle(
            br#"{"id":"x","method":"emitEvent","params":{"name":"n","payload":[1]}}"#,
            Capabilities::ALL,
            &data,
        );
        assert_eq!(o.event, Some(("n".into(), json!([1]))));
        assert_eq!(o.reply["result"], true);
        let o = handle(
            br#"{"id":"x","method":"emitEvent","params":{"name":"n"}}"#,
            Capabilities {
                get_data: true,
                emit_event: false,
            },
            &data,
        );
        assert_eq!(
            (o.reply["error"]["code"].as_str(), o.event),
            (Some("not_allowed"), None)
        );
        let o = handle(
            br#"{"id":"x","method":"readFile"}"#,
            Capabilities::ALL,
            &data,
        );
        assert_eq!(o.reply["error"]["code"], "unknown_method");
        let o = handle(br#"{"id":{},"method":"getData"}"#, Capabilities::ALL, &data);
        assert_eq!(o.reply["error"]["code"], "bad_request");
        let o = handle(b"[1]", Capabilities::ALL, &data);
        assert_eq!(o.reply["error"]["code"], "bad_request");
    }

    #[test]
    fn reply_script_escapes() {
        let s = reply_script(&json!({"id":"</script>'\u{2028}","result":"\"); alert(1); (\""}));
        assert!(
            s.starts_with("window.dispatchEvent(new CustomEvent('artifactbridgereply',{detail:{")
        );
        assert!(s.contains(r#"\"); alert(1); (\""#));
    }
}
