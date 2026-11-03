//! Value snapshots (REFS §A2.2): what the person saw, at most 2048 bytes as JSON. A bigger
//! value is shrunk (long lists halved, long texts cut) and marked `"truncated": true`.

use serde_json::{Map, Value};

pub const MAX_BYTES: usize = chat_core::proto::MAX_BLOCK_REF_VALUE_BYTES;

fn bytes(v: &Value) -> usize {
    serde_json::to_vec(v).map_or(usize::MAX, |b| b.len())
}

/// The biggest list or text inside `v`, by its JSON size, as a mutable reference.
fn biggest(v: &mut Value) -> Option<&mut Value> {
    fn size_of(v: &Value) -> usize {
        match v {
            Value::Array(a) if a.len() > 1 => bytes(v),
            Value::String(s) if s.chars().count() > 16 => s.len(),
            _ => 0,
        }
    }
    // Find the path of the biggest shrinkable node, then walk to it.
    fn find(v: &Value, path: &mut Vec<PathStep>, best: &mut (usize, Vec<PathStep>)) {
        let size = size_of(v);
        if size > best.0 {
            *best = (size, path.clone());
        }
        match v {
            Value::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    path.push(PathStep::Index(i));
                    find(x, path, best);
                    path.pop();
                }
            }
            Value::Object(o) => {
                for (k, x) in o {
                    path.push(PathStep::Key(k.clone()));
                    find(x, path, best);
                    path.pop();
                }
            }
            _ => {}
        }
    }
    let mut best = (0, Vec::new());
    find(v, &mut Vec::new(), &mut best);
    if best.0 == 0 {
        return None;
    }
    let mut at = v;
    for step in best.1 {
        at = match (step, at) {
            (PathStep::Index(i), Value::Array(a)) => a.get_mut(i)?,
            (PathStep::Key(k), Value::Object(o)) => o.get_mut(&k)?,
            _ => return None,
        };
    }
    Some(at)
}

#[derive(Clone)]
enum PathStep {
    Index(usize),
    Key(String),
}

/// `v` within [`MAX_BYTES`].
pub fn cap(v: Value) -> Value {
    if bytes(&v) <= MAX_BYTES {
        return v;
    }
    // Leave room for the `truncated` member.
    let budget = MAX_BYTES - 24;
    let mut v = v;
    for _ in 0..200 {
        if bytes(&v) <= budget {
            break;
        }
        let Some(node) = biggest(&mut v) else { break };
        match node {
            Value::Array(a) => {
                let keep = a.len() / 2;
                a.truncate(keep.max(1));
            }
            Value::String(s) => {
                let keep = s.chars().count() / 2;
                *s = super::label::cut(s, keep.max(8));
            }
            _ => break,
        }
    }
    let mut out = match v {
        Value::Object(o) => Value::Object(o),
        other => {
            let mut o = Map::new();
            o.insert("value".into(), other);
            Value::Object(o)
        }
    };
    if let Value::Object(o) = &mut out {
        o.insert("truncated".into(), Value::Bool(true));
    }
    if bytes(&out) <= MAX_BYTES {
        return out;
    }
    // Nothing left to shrink (many small members): keep a text of it.
    let text = serde_json::to_string(&out).unwrap_or_default();
    serde_json::json!({ "text": super::label::cut(&text, 500), "truncated": true })
}

/// A number as people read it: integers with thousands separators (2,340), others with up
/// to 2 decimals.
pub fn display_number(n: f64) -> String {
    if !n.is_finite() {
        return String::new();
    }
    if n.fract() == 0.0 && n.abs() < 1e15 {
        let i = n as i64;
        let digits = i.unsigned_abs().to_string();
        let mut out = String::new();
        for (k, c) in digits.chars().enumerate() {
            if k > 0 && (digits.len() - k).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        return if i < 0 { format!("-{out}") } else { out };
    }
    let s = format!("{n:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The number a cell shows, when it is one (`9`, `2,340`, `-3`, `1.2`): the `raw` member.
pub fn parse_number(display: &str) -> Option<Value> {
    let t = display.trim().replace(',', "");
    if t.is_empty() || t.len() > 24 {
        return None;
    }
    if let Ok(i) = t.parse::<i64>() {
        return Some(Value::from(i));
    }
    let f = t.parse::<f64>().ok().filter(|f| f.is_finite())?;
    serde_json::Number::from_f64(f).map(Value::Number)
}
