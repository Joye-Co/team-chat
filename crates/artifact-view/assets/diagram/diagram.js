// The TeamChat diagram renderer page. The host calls `window.tcRender(request)` with
// `{reqId, source, config, limits}` (one request at a time) and gets back one bridge event:
//   rendered {reqId, svg, width, height, nodes: [{id, label, kind, x, y, w, h}], type}
//   failed   {reqId, reason: "parse" | "too_big" | "unsupported" | "internal", detail}
// The page has no network and no navigation; it only calls `window.artifact.emitEvent`.
(function () {
  'use strict';

  // mermaid's diagram type id -> TeamChat type.
  var TYPES = {
    'flowchart': 'flowchart', 'flowchart-v2': 'flowchart', 'flowchart-elk': 'flowchart',
    'graph': 'flowchart',
    'sequence': 'sequence',
    'state': 'state', 'stateDiagram': 'state',
    'class': 'class', 'classDiagram': 'class',
    'er': 'er',
    'gantt': 'gantt',
    'timeline': 'timeline'
  };

  function emit(name, payload) {
    try { window.artifact.emitEvent(name, payload); } catch (e) { /* bridge gone */ }
  }

  function message(e) {
    var m = (e && (e.message || e.str)) || String(e);
    return String(m).slice(0, 2000);
  }

  function fail(reqId, reason, detail) {
    var out = document.getElementById('out');
    if (out) out.textContent = '';
    emit('failed', { reqId: reqId, reason: reason, detail: String(detail || '').slice(0, 2000) });
  }

  // Same rules as the host's Rust sanitizer (REFS §B6).
  function sanitize(root) {
    var drop = root.querySelectorAll('script, foreignObject');
    for (var i = 0; i < drop.length; i++) drop[i].remove();
    var links = root.querySelectorAll('a');
    for (var j = 0; j < links.length; j++) {
      var a = links[j];
      while (a.firstChild) a.parentNode.insertBefore(a.firstChild, a);
      a.remove();
    }
    var all = root.querySelectorAll('*');
    var list = [root];
    for (var k = 0; k < all.length; k++) list.push(all[k]);
    list.forEach(function (el) {
      var tag = (el.localName || '').toLowerCase();
      if (tag === 'image') {
        var src = el.getAttribute('href') || el.getAttribute('xlink:href') || '';
        if (src.indexOf('data:') !== 0) { el.remove(); return; }
      }
      Array.prototype.slice.call(el.attributes).forEach(function (at) {
        var n = at.name.toLowerCase(), v = at.value || '';
        if (n.indexOf('on') === 0) el.removeAttribute(at.name);
        else if ((n === 'href' || n === 'xlink:href') && v.charAt(0) !== '#' &&
                 !(tag === 'image' && v.indexOf('data:') === 0)) el.removeAttribute(at.name);
        else if (n === 'style' && /url\(\s*['"]?\s*[^#'"\s)]/i.test(v)) el.removeAttribute(at.name);
      });
    });
  }

  function idFrom(el, patterns) {
    var d = el.getAttribute('data-id');
    if (d) return d;
    var id = el.getAttribute('id') || '';
    for (var i = 0; i < patterns.length; i++) {
      var m = id.match(patterns[i]);
      if (m) return m[1];
    }
    return id || null;
  }

  // Text of a node: its lines (leaf text and tspan elements) joined by spaces.
  function label(el) {
    var parts = [];
    var leaves = el.querySelectorAll('text, tspan');
    for (var i = 0; i < leaves.length; i++) {
      if (leaves[i].querySelector('tspan')) continue;
      var t = (leaves[i].textContent || '').replace(/\s+/g, ' ').trim();
      if (t) parts.push(t);
    }
    var s = parts.length ? parts.join(' ') : (el.textContent || '');
    return s.replace(/\s+/g, ' ').trim().slice(0, 256);
  }

  // Elements that are referenceable nodes, per type (REFS §B5).
  function nodeElements(svg, type, svgId) {
    var out = [];
    function add(sel, kind, patterns, labelOf) {
      var els = svg.querySelectorAll(sel);
      for (var i = 0; i < els.length; i++) {
        var id = idFrom(els[i], patterns);
        if (!id) continue;
        out.push({ el: els[i], id: id, kind: kind, label: labelOf ? labelOf(els[i]) : label(els[i]) });
      }
    }
    if (type === 'flowchart') {
      add('g.node', 'node', [/^(?:.*-)?flowchart-(.+)-\d+$/]);
      add('g.cluster', 'subgraph', [/^(.+)$/]);
    } else if (type === 'state') {
      add('g.node', 'state', [/^(?:.*-)?state-(.+)-\d+$/]);
    } else if (type === 'class') {
      add('g.node', 'class', [/^(?:.*-)?classId-(.+)-\d+$/]);
    } else if (type === 'er') {
      add('g.node', 'entity', [/^(?:.*-)?entity-(.+)-\d+$/]);
    } else if (type === 'sequence') {
      // The top box of each participant (the one nearest the top).
      var best = {}, order = [];
      var boxes = svg.querySelectorAll('[name]');
      for (var i = 0; i < boxes.length; i++) {
        var cls = boxes[i].getAttribute('class') || '';
        if (!/actor/.test(cls)) continue;
        var nm = boxes[i].getAttribute('name');
        var g = boxes[i].tagName.toLowerCase() === 'g' ? boxes[i] : boxes[i].parentNode;
        var top = g.getBoundingClientRect().top;
        if (!(nm in best)) order.push(nm);
        if (!(nm in best) || top < best[nm].top) best[nm] = { el: g, top: top };
      }
      order.forEach(function (nm) {
        out.push({ el: best[nm].el, id: nm, kind: 'participant', label: label(best[nm].el) || nm });
      });
    } else if (type === 'gantt') {
      add('rect.task', 'task', [/^tc\d+-(.+)$/, /^(.+)$/], function (el) {
        var t = svg.querySelector('[id="' + el.id + '-text"]');
        return t ? label(t) : el.id;
      });
    } else if (type === 'timeline') {
      var evs = svg.querySelectorAll('g.eventWrapper');
      for (var e = 0; e < evs.length; e++) {
        out.push({ el: evs[e], id: 'event-' + e, kind: 'event', label: label(evs[e]) });
      }
      if (!evs.length) {
        var ts = svg.querySelectorAll('g.taskWrapper');
        for (var t2 = 0; t2 < ts.length; t2++) {
          out.push({ el: ts[t2], id: 'event-' + t2, kind: 'event', label: label(ts[t2]) });
        }
      }
    }
    // Drop helper nodes without a label (start and end dots of state diagrams).
    return out.filter(function (n) { return n.label.length > 0; });
  }

  function countEdges(svg) {
    return svg.querySelectorAll(
      '.edgePaths path, path.flowchart-link, line.messageLine0, line.messageLine1, ' +
      'path.messageLine0, path.messageLine1, path.transition, path.relation, path.relationshipLine'
    ).length;
  }

  var seq = 0;

  window.tcRender = async function (req) {
    var reqId = req.reqId;
    var limits = req.limits || {};
    try {
      var m = window.mermaid;
      if (!m) return fail(reqId, 'internal', 'library missing');
      m.initialize(req.config);
      var parsed;
      try {
        parsed = await m.parse(req.source);
      } catch (e) {
        var pm = message(e);
        return fail(reqId, /maximum text size|edge limit|too many/i.test(pm) ? 'too_big' : 'parse', pm);
      }
      var type = TYPES[parsed && parsed.diagramType];
      if (!type) return fail(reqId, 'unsupported', parsed && parsed.diagramType);
      var svgId = 'tc' + (++seq);
      var r;
      try {
        r = await m.render(svgId, req.source);
      } catch (e) {
        var rm = message(e);
        return fail(reqId, /maximum text size|edge limit|too many/i.test(rm) ? 'too_big' : 'parse', rm);
      }
      var out = document.getElementById('out');
      out.innerHTML = r.svg;
      var svg = out.querySelector('svg');
      if (!svg) return fail(reqId, 'internal', 'no svg');
      sanitize(svg);
      var vb = svg.viewBox && svg.viewBox.baseVal;
      var w = vb && vb.width ? vb.width : svg.getBBox().width;
      var h = vb && vb.height ? vb.height : svg.getBBox().height;
      w = Math.ceil(w); h = Math.ceil(h);
      svg.setAttribute('width', String(w));
      svg.setAttribute('height', String(h));
      svg.style.maxWidth = 'none';
      svg.style.backgroundColor = 'transparent';
      var maxSide = limits.maxSide || 4096;
      if (w > maxSide || h > maxSide) return fail(reqId, 'too_big', 'size ' + w + 'x' + h);
      if (countEdges(svg) > (limits.maxEdges || 300)) return fail(reqId, 'too_big', 'edges');
      var found = nodeElements(svg, type, svgId);
      if (found.length > (limits.maxNodes || 150)) return fail(reqId, 'too_big', 'nodes ' + found.length);
      var sr = svg.getBoundingClientRect();
      var nodes = found.map(function (n) {
        var b = n.el.getBoundingClientRect();
        return {
          id: String(n.id).slice(0, 256), label: n.label, kind: n.kind,
          x: b.left - sr.left, y: b.top - sr.top, w: b.width, h: b.height
        };
      });
      var text = new XMLSerializer().serializeToString(svg);
      emit('rendered', { reqId: reqId, svg: text, width: w, height: h, nodes: nodes, type: type });
    } catch (e) {
      fail(reqId, 'internal', message(e));
    }
  };

  emit('ready', {});
})();
