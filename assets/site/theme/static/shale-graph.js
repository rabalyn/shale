/* The graph page of the site (SITE-040): the notes as dots and their links as lines, laid out by a
   small force simulation in the browser. It reads the data that the page itself carries
   (`data-graph`: the published notes and their links, from data/graph.json) and loads nothing. */
(function () {
  "use strict";
  var host = document.getElementById("graph");
  if (!host) { return; }
  var data;
  try { data = JSON.parse(host.getAttribute("data-graph") || "{}"); } catch (e) { return; }
  var base = host.getAttribute("data-base") || "/";
  var NS = "http://www.w3.org/2000/svg";
  var W = 900, H = 640;

  var nodes = (data.nodes || []).map(function (n, i, all) {
    var a = (2 * Math.PI * i) / Math.max(all.length, 1);
    return { id: n.id, title: n.title, url: n.url, x: W / 2 + Math.cos(a) * 200, y: H / 2 + Math.sin(a) * 200, vx: 0, vy: 0, degree: 0, fixed: false };
  });
  var byId = {};
  nodes.forEach(function (n) { byId[n.id] = n; });
  var links = [];
  (data.links || []).forEach(function (l) {
    var s = byId[l.source], t = byId[l.target];
    if (s && t) { links.push({ s: s, t: t }); s.degree++; t.degree++; }
  });
  if (!nodes.length) { return; }

  function el(name, attrs, parent) {
    var e = document.createElementNS(NS, name);
    for (var k in attrs) { e.setAttribute(k, attrs[k]); }
    if (parent) { parent.appendChild(e); }
    return e;
  }
  var svg = el("svg", { viewBox: "0 0 " + W + " " + H, role: "img", "aria-label": "Graph of the published notes", "class": "graph-svg" }, host);
  var view = el("g", {}, svg);
  var lineEls = links.map(function () { return el("line", { "class": "graph-link" }, view); });
  var nodeEls = nodes.map(function (n) {
    var a = el("a", { href: base.replace(/\/$/, "") + n.url, "class": "graph-node" }, view);
    var c = el("circle", { r: 4 + Math.min(Math.sqrt(n.degree) * 2, 10) }, a);
    el("title", {}, c).textContent = n.title;
    var t = el("text", { x: 8, y: 4 }, a);
    t.textContent = n.title;
    a.addEventListener("mouseenter", function () { highlight(n, true); });
    a.addEventListener("mouseleave", function () { highlight(n, false); });
    return a;
  });
  if (nodes.length > 150) { host.className += " graph-quiet"; }

  function highlight(n, on) {
    links.forEach(function (l, i) {
      var near = l.s === n || l.t === n;
      lineEls[i].setAttribute("class", "graph-link" + (on && near ? " near" : ""));
    });
  }

  function draw() {
    links.forEach(function (l, i) {
      var e = lineEls[i];
      e.setAttribute("x1", l.s.x); e.setAttribute("y1", l.s.y);
      e.setAttribute("x2", l.t.x); e.setAttribute("y2", l.t.y);
    });
    nodes.forEach(function (n, i) { nodeEls[i].setAttribute("transform", "translate(" + n.x.toFixed(1) + "," + n.y.toFixed(1) + ")"); });
  }

  // forces: every pair repels, a link is a spring, everything is pulled gently to the middle
  var ticks = 0, limit = nodes.length > 800 ? 120 : 300;
  function step() {
    var alpha = 1 - ticks / limit, i, j, n, m, dx, dy, d2, d, f;
    for (i = 0; i < nodes.length; i++) {
      n = nodes[i];
      for (j = i + 1; j < nodes.length; j++) {
        m = nodes[j];
        dx = n.x - m.x; dy = n.y - m.y; d2 = dx * dx + dy * dy + 0.01;
        if (d2 > 90000) { continue; }
        f = (2400 * alpha) / d2; d = Math.sqrt(d2);
        n.vx += (dx / d) * f; n.vy += (dy / d) * f; m.vx -= (dx / d) * f; m.vy -= (dy / d) * f;
      }
    }
    links.forEach(function (l) {
      dx = l.t.x - l.s.x; dy = l.t.y - l.s.y; d = Math.sqrt(dx * dx + dy * dy) + 0.01;
      f = (d - 70) * 0.02 * alpha;
      l.s.vx += (dx / d) * f; l.s.vy += (dy / d) * f; l.t.vx -= (dx / d) * f; l.t.vy -= (dy / d) * f;
    });
    nodes.forEach(function (n) {
      n.vx += (W / 2 - n.x) * 0.004 * alpha; n.vy += (H / 2 - n.y) * 0.004 * alpha;
      if (!n.fixed) { n.x += n.vx; n.y += n.vy; }
      n.vx *= 0.8; n.vy *= 0.8;
    });
    ticks++;
  }
  function run() {
    for (var k = 0; k < 4 && ticks < limit; k++) { step(); }
    draw();
    if (ticks < limit) { window.requestAnimationFrame(run); }
  }
  run();

  // zoom with the wheel, move with a drag of the background, move a dot with a drag of the dot
  var scale = 1, tx = 0, ty = 0, drag = null;
  function apply() { view.setAttribute("transform", "translate(" + tx + "," + ty + ") scale(" + scale + ")"); }
  function point(ev) {
    var r = svg.getBoundingClientRect();
    return { x: ((ev.clientX - r.left) / r.width) * W, y: ((ev.clientY - r.top) / r.height) * H };
  }
  svg.addEventListener("wheel", function (ev) {
    ev.preventDefault();
    var p = point(ev), k = ev.deltaY < 0 ? 1.15 : 1 / 1.15, next = Math.min(Math.max(scale * k, 0.3), 6);
    tx = p.x - ((p.x - tx) / scale) * next; ty = p.y - ((p.y - ty) / scale) * next; scale = next; apply();
  }, { passive: false });
  svg.addEventListener("pointerdown", function (ev) {
    var a = ev.target.closest ? ev.target.closest("a") : null;
    var node = a ? nodes[nodeEls.indexOf(a)] : null;
    drag = { node: node, from: point(ev), moved: false, tx: tx, ty: ty };
    svg.setPointerCapture(ev.pointerId);
  });
  svg.addEventListener("pointermove", function (ev) {
    if (!drag) { return; }
    var p = point(ev);
    if (Math.abs(p.x - drag.from.x) + Math.abs(p.y - drag.from.y) > 3) { drag.moved = true; }
    if (!drag.moved) { return; }
    if (drag.node) { drag.node.x = (p.x - tx) / scale; drag.node.y = (p.y - ty) / scale; drag.node.fixed = true; draw(); }
    else { tx = drag.tx + p.x - drag.from.x; ty = drag.ty + p.y - drag.from.y; apply(); }
  });
  svg.addEventListener("pointerup", function () { drag = null; });
  svg.addEventListener("click", function (ev) { if (drag && drag.moved) { ev.preventDefault(); } }, true);
})();
