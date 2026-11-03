//! Diagrams (REFS §B2, §B6): the bundled renderer page, its request and result protocol,
//! the fixed library config, the SVG sanitizer and, on macOS, the long-lived hidden
//! renderer window ([`DiagramRenderer`]).
//!
//! The page runs on the reserved origin `artifact://teamchat-diagram-v<RENDERER_VERSION>/`
//! from memory, with the usual CSP, rule list and navigation policy, and only `emitEvent`.

use serde_json::{Value, json};

use crate::resolve::Content;

/// Bump when the page, the config or the sanitizer change: every cache key changes.
pub const RENDERER_VERSION: u32 = 1;
/// The artifact id of the reserved origin (`teamchat-diagram-v<RENDERER_VERSION>`).
pub const ARTIFACT_ID: &str = "teamchat-diagram";
/// The bundled library: `mermaid` 12.2.0 from npm, `dist/mermaid.min.js`, MIT
/// (`assets/diagram/mermaid.LICENSE`).
pub const MERMAID_JS: &[u8] = include_bytes!("../assets/diagram/mermaid.min.js");
pub const MERMAID_VERSION: &str = "12.2.0";
/// SHA-256 of [`MERMAID_JS`]; part of every cache key.
pub const MERMAID_SHA256: &str = "a8a6a6f805b074a8261a2825c86f99c8054acb3b35f50bb7e24a4f9f2c3c3b9f";
pub const DIAGRAM_HTML: &str = include_str!("../assets/diagram/diagram.html");
pub const DIAGRAM_JS: &str = include_str!("../assets/diagram/diagram.js");

/// Largest bridge message of the renderer window (an SVG with its hit map).
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
/// Source limit (`maxTextSize`).
pub const MAX_SOURCE_BYTES: usize = 16 * 1024;
pub const MAX_NODES: usize = 150;
pub const MAX_EDGES: usize = 300;
/// Largest output side at scale 1, CSS px.
pub const MAX_SIDE: f64 = 4096.0;
/// Largest PNG edge in pixels; a larger diagram renders at the largest scale that fits.
pub const MAX_PNG_EDGE: f64 = 8192.0;

/// The renderer page's files, served from memory.
pub fn content() -> Content {
    Content::memory([
        ("index.html".to_string(), DIAGRAM_HTML.as_bytes().to_vec()),
        ("mermaid.min.js".to_string(), MERMAID_JS.to_vec()),
        ("diagram.js".to_string(), DIAGRAM_JS.as_bytes().to_vec()),
    ])
}

/// Resolved theme colors (`#rrggbb` or `#rrggbbaa`) for one theme (REFS §B8).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ThemeVars {
    pub raised: String,
    pub line: String,
    pub line_2: String,
    pub ink: String,
    pub ink_2: String,
    pub ink_3: String,
    pub sunken: String,
    pub rail: String,
    pub bg: String,
    pub sel: String,
    /// `--live` mixed 20 % into `--raised`.
    pub live_mix: String,
    /// `--ok` mixed 20 % into `--raised`.
    pub ok_mix: String,
    /// `--bad` mixed 20 % into `--raised`.
    pub bad_mix: String,
    pub font_family: String,
    pub font_size_px: u32,
}

/// The library config. Fixed by the host, never from the source (REFS §B6).
pub fn mermaid_config(t: &ThemeVars) -> Value {
    let font_size = format!("{}px", t.font_size_px);
    let font_size_v = font_size.as_str();
    let pairs: &[(&str, &str)] = &[
        ("background", "transparent"),
        ("fontFamily", &t.font_family),
        ("fontSize", font_size_v),
        ("primaryColor", &t.raised),
        ("mainBkg", &t.raised),
        ("nodeBkg", &t.raised),
        ("actorBkg", &t.raised),
        ("classText", &t.ink),
        ("primaryBorderColor", &t.line_2),
        ("nodeBorder", &t.line_2),
        ("actorBorder", &t.line_2),
        ("primaryTextColor", &t.ink),
        ("textColor", &t.ink),
        ("actorTextColor", &t.ink),
        ("signalTextColor", &t.ink),
        ("titleColor", &t.ink),
        ("nodeTextColor", &t.ink),
        ("lineColor", &t.ink_3),
        ("signalColor", &t.ink_3),
        ("arrowheadColor", &t.ink_3),
        ("actorLineColor", &t.line_2),
        ("secondaryColor", &t.sunken),
        ("clusterBkg", &t.sunken),
        ("labelBoxBkgColor", &t.sunken),
        ("labelBoxBorderColor", &t.line),
        ("labelTextColor", &t.ink),
        ("loopTextColor", &t.ink),
        ("clusterBorder", &t.line),
        ("noteBorderColor", &t.line),
        ("noteBkgColor", &t.rail),
        ("noteTextColor", &t.ink),
        ("tertiaryColor", &t.bg),
        ("edgeLabelBackground", &t.bg),
        ("activationBkgColor", &t.sunken),
        ("activationBorderColor", &t.line_2),
        ("sequenceNumberColor", &t.bg),
        // State and class diagrams.
        ("altBackground", &t.sunken),
        ("compositeBackground", &t.sunken),
        ("compositeTitleBackground", &t.sunken),
        ("compositeBorder", &t.line),
        ("innerEndBackground", &t.ink_3),
        ("specialStateColor", &t.ink_3),
        ("stateLabelColor", &t.ink),
        ("stateBkg", &t.raised),
        ("labelColor", &t.ink),
        ("transitionColor", &t.ink_3),
        ("transitionLabelColor", &t.ink_2),
        // ER.
        ("attributeBackgroundColorOdd", &t.raised),
        ("attributeBackgroundColorEven", &t.sunken),
        // Gantt: blue, green and red only for task states (the status band meanings).
        ("taskBkgColor", &t.sel),
        ("taskBorderColor", &t.line_2),
        ("activeTaskBkgColor", &t.live_mix),
        ("activeTaskBorderColor", &t.line_2),
        ("doneTaskBkgColor", &t.ok_mix),
        ("doneTaskBorderColor", &t.line_2),
        ("critBkgColor", &t.bad_mix),
        ("critBorderColor", &t.line_2),
        ("taskTextColor", &t.ink),
        ("taskTextOutsideColor", &t.ink),
        ("taskTextLightColor", &t.ink),
        ("taskTextDarkColor", &t.ink),
        ("taskTextClickableColor", &t.ink),
        ("sectionBkgColor", &t.bg),
        ("sectionBkgColor2", &t.sunken),
        ("altSectionBkgColor", &t.bg),
        ("excludeBkgColor", &t.sunken),
        ("gridColor", &t.line),
        ("todayLineColor", &t.ink_3),
        // Timeline sections stay grey.
        ("cScale0", &t.sunken),
        ("cScale1", &t.sel),
        ("cScale2", &t.sunken),
        ("cScale3", &t.sel),
        ("cScale4", &t.sunken),
        ("cScale5", &t.sel),
        ("cScale6", &t.sunken),
        ("cScale7", &t.sel),
        ("cScale8", &t.sunken),
        ("cScale9", &t.sel),
        ("cScale10", &t.sunken),
        ("cScale11", &t.sel),
        ("cScaleLabel0", &t.ink),
        ("cScaleLabel1", &t.ink),
        ("cScaleLabel2", &t.ink),
        ("cScaleLabel3", &t.ink),
        ("cScaleLabel4", &t.ink),
        ("cScaleLabel5", &t.ink),
        ("cScaleLabel6", &t.ink),
        ("cScaleLabel7", &t.ink),
        ("cScaleLabel8", &t.ink),
        ("cScaleLabel9", &t.ink),
        ("cScaleLabel10", &t.ink),
        ("cScaleLabel11", &t.ink),
    ];
    let vars: serde_json::Map<String, Value> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
        .collect();

    let mut config = json!({
        "startOnLoad": false,
        "securityLevel": "strict",
        "htmlLabels": false,
        "flowchart": {
            "htmlLabels": false, "useMaxWidth": false, "nodeSpacing": 28, "rankSpacing": 36,
            "padding": 10, "diagramPadding": 8
        },
        "sequence": {
            "useMaxWidth": false, "mirrorActors": false, "height": 40,
            "width": 132, "boxMargin": 8, "noteMargin": 8, "messageMargin": 32,
            "actorFontFamily": t.font_family, "noteFontFamily": t.font_family,
            "messageFontFamily": t.font_family
        },
        "class": { "useMaxWidth": false, "htmlLabels": false },
        "state": { "useMaxWidth": false },
        "er": { "useMaxWidth": false },
        "gantt": { "useMaxWidth": false, "axisFormat": "%m-%d", "barHeight": 22, "barGap": 6 },
        "timeline": { "useMaxWidth": false, "disableMulticolor": true },
        "maxTextSize": MAX_SOURCE_BYTES,
        "maxEdges": MAX_EDGES,
        "theme": "base",
        "darkMode": false,
        "fontFamily": t.font_family,
        "themeVariables": vars,
        "themeCSS": theme_css(t),
    });
    // `secure`: every key the host sets, so no directive can change them.
    let keys: Vec<Value> = config
        .as_object()
        .map(|o| o.keys().map(|k| Value::String(k.clone())).collect())
        .unwrap_or_default();
    let mut secure = keys;
    secure.push(Value::String("secure".into()));
    config["secure"] = Value::Array(secure);
    config
}

/// Shape details the theme variables do not cover: no drop shadows, rounded boxes (the
/// artboard's 6 px), hairline grid, no "today" line, labels in the UI font.
fn theme_css(t: &ThemeVars) -> String {
    format!(
        "* {{ filter: none !important; }}\
         .node rect, .node polygon, rect.actor, rect.task, rect.note, .cluster rect {{ rx: 6px; ry: 6px; }}\
         .node rect, .node polygon, .node circle, rect.actor, rect.task {{ stroke-width: 1px; }}\
         .grid .tick line {{ stroke: {line}; opacity: 1; }} .grid path {{ stroke-width: 0; }}\
         .grid .tick text {{ fill: {ink3}; }} .today {{ display: none; }}\
         text, tspan {{ font-family: {font}; }}",
        line = t.line,
        ink3 = t.ink_3,
        font = t.font_family
    )
}

/// One render request.
#[derive(Clone, Debug)]
pub struct RenderRequest {
    pub req_id: u64,
    pub source: String,
    pub theme: ThemeVars,
}

/// The script that starts a render. serde_json yields a valid JavaScript literal, so the
/// source cannot break out of the expression.
pub fn render_script(req: &RenderRequest) -> String {
    let arg = json!({
        "reqId": req.req_id,
        "source": req.source,
        "config": mermaid_config(&req.theme),
        "limits": { "maxNodes": MAX_NODES, "maxEdges": MAX_EDGES, "maxSide": MAX_SIDE },
    });
    format!("window.tcRender({arg});void 0")
}

/// Why a diagram did not render. Open on the page side: an unknown value is `Internal`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FailReason {
    Parse,
    TooBig,
    Unsupported,
    Internal,
    /// The host gave up after the render timeout.
    Timeout,
}

impl FailReason {
    pub fn as_str(self) -> &'static str {
        match self {
            FailReason::Parse => "parse",
            FailReason::TooBig => "too_big",
            FailReason::Unsupported => "unsupported",
            FailReason::Internal => "internal",
            FailReason::Timeout => "timeout",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "parse" => FailReason::Parse,
            "too_big" => FailReason::TooBig,
            "unsupported" => FailReason::Unsupported,
            "timeout" => FailReason::Timeout,
            _ => FailReason::Internal,
        }
    }
}

/// One referenceable node of a rendered diagram, in CSS px of the diagram at scale 1.
#[derive(Clone, Debug, PartialEq)]
pub struct DiagramNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A rendered diagram before its raster.
#[derive(Clone, Debug, PartialEq)]
pub struct Rendered {
    pub req_id: u64,
    /// Sanitized SVG (for copy only; people see the PNG).
    pub svg: String,
    pub width: f64,
    pub height: f64,
    pub nodes: Vec<DiagramNode>,
    /// `flowchart`, `sequence`, `state`, `class`, `er`, `gantt`, `timeline`.
    pub kind: String,
}

/// What the page reported for one request.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderOutcome {
    Rendered(Rendered),
    Failed {
        req_id: u64,
        reason: FailReason,
        /// For the log only (redacted by the host); never UI text.
        detail: String,
    },
}

impl RenderOutcome {
    pub fn req_id(&self) -> u64 {
        match self {
            RenderOutcome::Rendered(r) => r.req_id,
            RenderOutcome::Failed { req_id, .. } => *req_id,
        }
    }
}

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
}

fn text(v: &Value, key: &str, max: usize) -> String {
    let s = v.get(key).and_then(Value::as_str).unwrap_or("");
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

/// Reads a `rendered` or `failed` event from the page. Anything malformed is `None`.
/// The SVG is sanitized again here.
pub fn parse_event(name: &str, payload: &Value) -> Option<RenderOutcome> {
    let req_id = payload.get("reqId")?.as_u64()?;
    match name {
        "rendered" => {
            let width = num(payload, "width")?;
            let height = num(payload, "height")?;
            if width <= 0.0 || height <= 0.0 {
                return None;
            }
            let svg = sanitize_svg(payload.get("svg")?.as_str()?);
            let nodes = payload
                .get("nodes")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .take(MAX_NODES * 2)
                        .filter_map(|n| {
                            Some(DiagramNode {
                                id: text(n, "id", 256),
                                label: text(n, "label", 256),
                                kind: text(n, "kind", 16),
                                x: num(n, "x")?,
                                y: num(n, "y")?,
                                w: num(n, "w")?.max(0.0),
                                h: num(n, "h")?.max(0.0),
                            })
                        })
                        .filter(|n| !n.id.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            Some(RenderOutcome::Rendered(Rendered {
                req_id,
                svg,
                width,
                height,
                nodes,
                kind: text(payload, "type", 16),
            }))
        }
        "failed" => Some(RenderOutcome::Failed {
            req_id,
            reason: FailReason::parse(payload.get("reason").and_then(Value::as_str).unwrap_or("")),
            detail: text(payload, "detail", 2000),
        }),
        _ => None,
    }
}

/// The scale to raster at: `wanted`, lowered so that no PNG edge passes [`MAX_PNG_EDGE`].
pub fn raster_scale(width: f64, height: f64, wanted: f64) -> f64 {
    let side = width.max(height).max(1.0);
    wanted.min(MAX_PNG_EDGE / side).max(0.01)
}

// --- SVG sanitizer ---------------------------------------------------------------------

/// Removes active content from an SVG (REFS §B6): `<script>` and `<foreignObject>` with
/// their content, `<a>` (children kept), `<image>` whose href is not `data:`, every `on*`
/// attribute, `href`/`xlink:href` not starting with `#`, `style` values with `url(` to a
/// non-fragment, and in `<style>` text every `url(` to a non-fragment and `@import`.
/// Comments, processing instructions and doctypes are dropped. The input is mermaid's
/// output, which is well-formed XML; anything else is cut at the first broken tag.
pub fn sanitize_svg(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    // Depth inside a dropped element (script, foreignObject, unsafe image).
    let mut skip: Vec<String> = Vec::new();
    // Open `<a>` elements whose end tags must also vanish (by nesting depth).
    let mut open: Vec<bool> = Vec::new();
    let mut in_style = false;
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            let end = input[i..].find('<').map_or(bytes.len(), |n| i + n);
            if skip.is_empty() {
                let chunk = &input[i..end];
                if in_style {
                    out.push_str(&clean_css(chunk));
                } else {
                    out.push_str(chunk);
                }
            }
            i = end;
            continue;
        }
        let rest = &input[i..];
        if rest.starts_with("<!--") {
            i += rest.find("-->").map_or(rest.len(), |n| n + 3);
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            let n = rest.find("]]>").map_or(rest.len(), |n| n + 3);
            if skip.is_empty() {
                let inner = &rest[9..n.saturating_sub(3).max(9)];
                let inner = if in_style { clean_css(inner) } else { inner.to_string() };
                out.push_str("<![CDATA[");
                out.push_str(&inner.replace("]]>", ""));
                out.push_str("]]>");
            }
            i += n;
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            i += rest.find('>').map_or(rest.len(), |n| n + 1);
            continue;
        }
        let Some(len) = tag_len(rest) else {
            break;
        };
        let tag = &rest[..len];
        i += len;
        let closing = tag.starts_with("</");
        let self_closing = !closing && tag.ends_with("/>");
        let name_start = if closing { 2 } else { 1 };
        let name: String = tag[name_start..]
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '/' && *c != '>')
            .collect();
        let local = name.rsplit(':').next().unwrap_or("").to_ascii_lowercase();
        if !skip.is_empty() {
            if closing && skip.last().is_some_and(|n| *n == local) {
                skip.pop();
            } else if !closing && !self_closing && skip.last().is_some_and(|n| *n == local) {
                skip.push(local);
            }
            continue;
        }
        if closing {
            if local == "style" {
                in_style = false;
            }
            if local == "a" {
                if open.pop().unwrap_or(false) {
                    continue;
                }
            } else {
                open.pop();
            }
            out.push_str(&format!("</{name}>"));
            continue;
        }
        let attrs = parse_attrs(&tag[name_start + name.len()..]);
        let dropped = match local.as_str() {
            "script" | "foreignobject" => true,
            "image" => !attrs.iter().any(|(k, v)| {
                let k = k.to_ascii_lowercase();
                (k == "href" || k == "xlink:href") && v.trim_start().to_ascii_lowercase().starts_with("data:")
            }),
            _ => false,
        };
        if dropped {
            if !self_closing {
                skip.push(local);
            }
            continue;
        }
        if local == "a" {
            if !self_closing {
                open.push(true);
            }
            continue;
        }
        out.push('<');
        out.push_str(&name);
        for (k, v) in &attrs {
            let lk = k.to_ascii_lowercase();
            let lv = v.trim_start().to_ascii_lowercase();
            if lk.starts_with("on") {
                continue;
            }
            if (lk == "href" || lk.ends_with(":href"))
                && !lv.starts_with('#')
                && !(local == "image" && lv.starts_with("data:"))
            {
                continue;
            }
            if lk == "style" && has_external_url(&lv) {
                continue;
            }
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            out.push_str(&v.replace('"', "&quot;"));
            out.push('"');
        }
        if self_closing {
            out.push_str("/>");
        } else {
            out.push('>');
            open.push(false);
            if local == "style" {
                in_style = true;
            }
        }
    }
    out
}

/// Length of the tag at the start of `s` (which starts with `<`), honoring quotes.
fn tag_len(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices().skip(1) {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '>' => return Some(i + 1),
            None if c == '<' => return None,
            None => {}
        }
    }
    None
}

/// Attributes of a tag body (`  a="1" b='2'/>`), values unescaped only for quotes kept as
/// written (entities stay entities).
fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == '/' || chars[i] == '>') {
            i += 1;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '=' && chars[i] != '>' && chars[i] != '/' {
            i += 1;
        }
        if start == i {
            break;
        }
        let key: String = chars[start..i].iter().collect();
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < chars.len() && chars[i] == '=' {
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
                let q = chars[i];
                i += 1;
                let vs = i;
                while i < chars.len() && chars[i] != q {
                    i += 1;
                }
                value = chars[vs..i].iter().collect();
                i += 1;
            } else {
                let vs = i;
                while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '>' {
                    i += 1;
                }
                value = chars[vs..i].iter().collect();
            }
        }
        // Keep only plain XML names.
        if key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':' || c == '.')
        {
            out.push((key, value));
        }
    }
    out
}

/// True when a style value has `url(` that does not point to a fragment.
fn has_external_url(lower: &str) -> bool {
    let mut rest = lower;
    while let Some(n) = rest.find("url(") {
        let after = rest[n + 4..].trim_start_matches(|c: char| c.is_whitespace() || c == '"' || c == '\'');
        if !after.starts_with('#') {
            return true;
        }
        rest = &rest[n + 4..];
    }
    lower.contains("@import") || lower.contains("expression(")
}

/// CSS text with external `url(...)` and `@import` neutralized.
fn clean_css(css: &str) -> String {
    let lower = css.to_ascii_lowercase();
    if !has_external_url(&lower) {
        return css.to_string();
    }
    let mut out = String::with_capacity(css.len());
    let mut i = 0;
    while i < css.len() {
        let l = &lower[i..];
        if l.starts_with("@import") {
            let end = l.find(';').map_or(css.len(), |n| i + n + 1);
            i = end;
            continue;
        }
        if l.starts_with("url(") {
            let close = l.find(')').map_or(css.len(), |n| i + n + 1);
            let inner = lower[i + 4..close.saturating_sub(1).max(i + 4)]
                .trim_matches(|c: char| c.is_whitespace() || c == '"' || c == '\'');
            if inner.starts_with('#') {
                out.push_str(&css[i..close]);
            } else {
                out.push_str("none");
            }
            i = close;
            continue;
        }
        let ch = css[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

// --- macOS renderer --------------------------------------------------------------------

#[cfg(target_os = "macos")]
pub use mac::{DiagramRenderer, RendererEvent};

#[cfg(target_os = "macos")]
mod mac {
    use objc2_foundation::MainThreadMarker;

    use super::*;
    use crate::{
        ArtifactEvent, ArtifactWindow, Capabilities, Error, OpenOptions, Presentation, Rect,
        VersionSpec,
    };

    /// The renderer page may only emit its outcomes; it never reads 产出 data.
    const RENDERER_CAPS: Capabilities = Capabilities {
        get_data: false,
        emit_event: true,
    };

    /// What the renderer tells its host, on the main thread.
    #[derive(Clone, Debug, PartialEq)]
    pub enum RendererEvent {
        /// The page loaded and its library is ready for requests.
        Ready,
        /// The page answered a request.
        Outcome(RenderOutcome),
        /// The page could not load or its web content process ended. The host recreates
        /// the renderer and does not retry the request that was running.
        Crashed(String),
    }

    /// One long-lived hidden window that renders diagrams (REFS §B2 step 3). Main thread
    /// only. A web view in a window that is never ordered on screen gives snapshot pixels
    /// (the §B2 spike, `examples/snapshot_spike.rs`).
    pub struct DiagramRenderer {
        window: ArtifactWindow,
    }

    impl DiagramRenderer {
        pub fn open(
            mtm: MainThreadMarker,
            on_event: impl Fn(RendererEvent) + 'static,
        ) -> Result<Self, Error> {
            let mut options = OpenOptions::new(
                "diagram",
                ARTIFACT_ID,
                VersionSpec::new(RENDERER_VERSION, content()),
            );
            options.presentation = Presentation::Hidden;
            options.capabilities = RENDERER_CAPS;
            options.max_message_bytes = MAX_MESSAGE_BYTES;
            options.transparent = true;
            options.width = 800.0;
            options.height = 600.0;
            let window = ArtifactWindow::open(mtm, options, move |event| match event {
                ArtifactEvent::Emit { name, payload, .. } => {
                    if name == "ready" {
                        on_event(RendererEvent::Ready);
                    } else if let Some(outcome) = parse_event(&name, &payload) {
                        on_event(RendererEvent::Outcome(outcome));
                    }
                }
                ArtifactEvent::LoadFailed { reason, .. } => on_event(RendererEvent::Crashed(reason)),
                _ => {}
            })?;
            Ok(Self { window })
        }

        /// Starts one render. The answer comes as [`RendererEvent::Outcome`].
        pub fn request(&self, req: &RenderRequest) {
            self.window.evaluate_script(&render_script(req));
        }

        /// Takes the PNG of the last rendered diagram (`width` × `height` CSS px at the top
        /// left) at `scale` pixels per CSS px, lowered by [`raster_scale`].
        pub fn capture(
            &self,
            width: f64,
            height: f64,
            scale: f64,
            done: impl FnOnce(Result<Vec<u8>, String>) + 'static,
        ) {
            let scale = raster_scale(width, height, scale);
            self.window.set_content_size(width.ceil().max(64.0), height.ceil().max(64.0));
            self.window
                .snapshot(Rect::new(0.0, 0.0, width.ceil(), height.ceil()), scale, done);
        }

        /// Loads the page again (after a timeout).
        pub fn reload(&self) {
            let _ = self
                .window
                .show_version(VersionSpec::new(RENDERER_VERSION, content()), RENDERER_CAPS);
        }

        pub fn is_closed(&self) -> bool {
            self.window.is_closed()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn theme() -> ThemeVars {
        ThemeVars {
            raised: "#ffffff".into(),
            line: "#eeeeee".into(),
            line_2: "#dddddd".into(),
            ink: "#111111".into(),
            ink_2: "#555555".into(),
            ink_3: "#888888".into(),
            sunken: "#f4f4f4".into(),
            rail: "#fafafa".into(),
            bg: "#ffffff".into(),
            sel: "#ececec".into(),
            live_mix: "#dde8ff".into(),
            ok_mix: "#dff3e4".into(),
            bad_mix: "#fbe0e0".into(),
            font_family: "-apple-system".into(),
            font_size_px: 13,
        }
    }

    #[test]
    fn bundle_matches_pinned_sha256() {
        let hex: String = Sha256::digest(MERMAID_JS)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(hex, MERMAID_SHA256);
    }

    #[test]
    fn origin_is_reserved_and_versioned() {
        assert_eq!(
            crate::origin_host(ARTIFACT_ID, RENDERER_VERSION).unwrap(),
            format!("teamchat-diagram-v{RENDERER_VERSION}")
        );
    }

    #[test]
    fn config_is_strict_and_secures_every_key() {
        let c = mermaid_config(&theme());
        assert_eq!(c["securityLevel"], "strict");
        assert_eq!(c["htmlLabels"], false);
        assert_eq!(c["flowchart"]["htmlLabels"], false);
        assert_eq!(c["startOnLoad"], false);
        assert_eq!(c["theme"], "base");
        assert_eq!(c["maxTextSize"], 16384);
        assert_eq!(c["maxEdges"], 300);
        let secure: Vec<&str> = c["secure"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        for key in c.as_object().unwrap().keys() {
            assert!(secure.contains(&key.as_str()), "{key} not secure");
        }
        assert_eq!(c["themeVariables"]["primaryColor"], "#ffffff");
        assert_eq!(c["themeVariables"]["lineColor"], "#888888");
    }

    #[test]
    fn render_script_is_one_safe_call() {
        let s = render_script(&RenderRequest {
            req_id: 7,
            source: "graph TD\nA[\"</script>\"]-->B');alert(1);//".into(),
            theme: theme(),
        });
        assert!(s.starts_with("window.tcRender({"));
        assert!(s.ends_with("});void 0"));
        assert!(s.contains(r#"\"</script>\""#));
    }

    #[test]
    fn parses_events() {
        let p = json!({"reqId": 3, "svg": "<svg><script>x</script><g/></svg>", "width": 120.5,
            "height": 80, "type": "flowchart",
            "nodes": [{"id":"A","label":"开始","kind":"node","x":1,"y":2,"w":30,"h":20},
                      {"id":"","label":"x","kind":"node","x":1,"y":2,"w":3,"h":4}]});
        let Some(RenderOutcome::Rendered(r)) = parse_event("rendered", &p) else {
            panic!("rendered");
        };
        assert_eq!(r.svg, "<svg><g/></svg>");
        assert_eq!(r.nodes.len(), 1);
        assert_eq!(r.nodes[0].label, "开始");
        let f = parse_event("failed", &json!({"reqId": 4, "reason": "nope", "detail": "d"}));
        assert_eq!(
            f,
            Some(RenderOutcome::Failed { req_id: 4, reason: FailReason::Internal, detail: "d".into() })
        );
        assert_eq!(parse_event("rendered", &json!({"reqId": 1})), None);
        assert_eq!(parse_event("other", &json!({"reqId": 1})), None);
    }

    #[test]
    fn raster_scale_keeps_png_under_limit() {
        assert_eq!(raster_scale(400.0, 300.0, 2.0), 2.0);
        assert_eq!(raster_scale(4096.0, 100.0, 4.0), 2.0);
    }

    #[test]
    fn sanitizer_drops_active_content() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" onload="x()"><!-- c --><script>alert(1)</script><svg:script>a</svg:script><style>.a{fill:url(#g)} .b{background:url(https://x/y.png)} @import "https://x/a.css";</style><a href="javascript:alert(1)"><g id="n" onclick="x" class="node"><text>A &amp; B</text></g></a><foreignObject><div>hi</div></foreignObject><image href="https://x/p.png"/><image href="data:image/png;base64,AA"/><image xlink:href="file:///etc"><title>t</title></image><use href="#m"/><use xlink:href="https://x#m"/><rect style="fill:url(#g)"/><rect style="fill:url( 'http://x' )"/></svg>"##;
        let out = sanitize_svg(svg);
        assert!(!out.contains("script"), "{out}");
        assert!(!out.contains("onload") && !out.contains("onclick"), "{out}");
        assert!(!out.contains("foreignObject") && !out.contains("<div"), "{out}");
        assert!(!out.contains("<a") && !out.contains("</a>"), "{out}");
        assert!(out.contains(r#"<g id="n" class="node"><text>A &amp; B</text></g>"#), "{out}");
        assert!(!out.contains("https://x/p.png") && !out.contains("file:///"), "{out}");
        assert!(out.contains(r#"<image href="data:image/png;base64,AA"/>"#), "{out}");
        assert!(out.contains(r##"<use href="#m"/>"##), "{out}");
        assert!(out.contains("<use/>"), "{out}");
        assert!(out.contains(r##"<rect style="fill:url(#g)"/>"##), "{out}");
        assert!(out.contains("<rect/>"), "{out}");
        assert!(out.contains(".a{fill:url(#g)}"), "{out}");
        assert!(!out.contains("https://x/y.png") && !out.contains("@import"), "{out}");
        assert!(!out.contains("<!--"), "{out}");
        assert!(out.ends_with("</svg>"), "{out}");
    }

    #[test]
    fn sanitizer_keeps_plain_svg_and_css_child_combinators() {
        let svg = "<svg viewBox=\"0 0 10 10\"><style>#a .b > rect{fill:#fff}</style><g transform=\"translate(1,2)\"><path d=\"M0 0L1 1\"/></g></svg>";
        assert_eq!(sanitize_svg(svg), svg);
    }
}
