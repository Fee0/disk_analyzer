(function () {
  var REPO = "Fee0/disk_analyzer";
  var FALLBACK_VERSION = "0.6.0";
  var SVG_NS = "http://www.w3.org/2000/svg";

  var KINDS = [
    { id: "video", label: "Video" },
    { id: "image", label: "Images" },
    { id: "audio", label: "Audio" },
    { id: "archive", label: "Archives" },
    { id: "disk-image", label: "Disk images" },
    { id: "document", label: "Documents" },
    { id: "code", label: "Code" },
    { id: "app", label: "Apps" },
    { id: "library", label: "Libraries" },
    { id: "cache", label: "Caches" },
    { id: "hidden", label: "Hidden space" }
  ];

  var PLATFORMS = {
    macos: { label: "macOS", suffix: "macos-universal.dmg", match: /macos.*\.dmg$/i, req: "macOS 11+ · Apple silicon & Intel", steps: ["Open the .dmg", "Drag Disk Analyzer to Applications", "Signed and notarized: opens without a warning"] },
    windows: { label: "Windows", suffix: "windows-x86_64.zip", match: /windows.*\.zip$/i, req: "Windows 10+ · x64", steps: ["Unzip and run disk-analyzer.exe", "If SmartScreen appears: More info → Run anyway"] },
    linux: { label: "Linux", suffix: "linux-x86_64.tar.gz", match: /linux-x86_64\.tar\.gz$/i, req: "x86_64", steps: ["Extract the archive", "Run ./install.sh to add it to your app menu"] },
    linuxArm: { label: "Linux ARM", suffix: "linux-aarch64.tar.gz", match: /linux-aarch64\.tar\.gz$/i, req: "aarch64", steps: [] }
  };

  function fileName(version, platform) {
    return "disk-analyzer-" + version + "-" + PLATFORMS[platform].suffix;
  }

  function fallbackRelease() {
    var assets = {};
    Object.keys(PLATFORMS).forEach(function (p) {
      var name = fileName(FALLBACK_VERSION, p);
      assets[p] = { name: name, url: "https://github.com/" + REPO + "/releases/download/v" + FALLBACK_VERSION + "/" + name, size: null };
    });
    return { version: FALLBACK_VERSION, assets: assets, live: false };
  }

  function resolveRelease(done) {
    var fallback = fallbackRelease();
    if (!window.fetch) { done(fallback); return; }
    fetch("https://api.github.com/repos/" + REPO + "/releases/latest", { headers: { Accept: "application/vnd.github+json" } })
      .then(function (r) { if (!r.ok) throw new Error("status " + r.status); return r.json(); })
      .then(function (rel) {
        var assets = {};
        Object.keys(PLATFORMS).forEach(function (p) {
          var hit = (rel.assets || []).filter(function (a) { return PLATFORMS[p].match.test(a.name); })[0];
          assets[p] = hit ? { name: hit.name, url: hit.browser_download_url, size: hit.size } : fallback.assets[p];
        });
        done({ version: String(rel.tag_name || "").replace(/^v/, "") || FALLBACK_VERSION, assets: assets, live: true });
      })
      .catch(function () { done(fallback); });
  }

  function detectOS() {
    var hint = ((navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || navigator.userAgent || "").toLowerCase();
    if (/mac|iphone|ipad/.test(hint)) return "macos";
    if (/win/.test(hint)) return "windows";
    if (/linux|x11|cros/.test(hint)) return "linux";
    return null;
  }

  function formatBytes(n) {
    if (n == null) return "";
    var units = ["B", "KB", "MB", "GB"], i = 0;
    while (n >= 1000 && i < units.length - 1) { n /= 1000; i++; }
    return (i ? n.toFixed(1) : n) + " " + units[i];
  }

  function wireDownloads(root) {
    root = root || document;
    var os = detectOS();
    root.querySelectorAll("[data-da-card]").forEach(function (card) {
      if (card.getAttribute("data-da-card") === os) card.setAttribute("data-current", "");
    });
    resolveRelease(function (rel) {
      root.querySelectorAll("[data-da-os]").forEach(function (el) {
        var asset = rel.assets[el.getAttribute("data-da-os")];
        if (asset) el.setAttribute("href", asset.url);
      });
      root.querySelectorAll("[data-da-file]").forEach(function (el) {
        var asset = rel.assets[el.getAttribute("data-da-file")];
        if (!asset) return;
        el.textContent = asset.size ? formatBytes(asset.size) : "";
      });
      root.querySelectorAll("[data-da-primary]").forEach(function (el) {
        var target = os || "macos";
        el.setAttribute("href", rel.assets[target].url);
        var label = el.querySelector("[data-da-primary-label]");
        if (label) label.textContent = os ? "Download for " + PLATFORMS[os].label : "Download";
        if (!os) el.setAttribute("href", "#download");
      });
    });
    return os;
  }

  function seeded(seed) {
    var s = seed >>> 0;
    return function () { s = (s * 1664525 + 1013904223) >>> 0; return s / 4294967296; };
  }

  function arcPath(cx, cy, r0, r1, a0, a1) {
    var large = a1 - a0 > Math.PI ? 1 : 0;
    function pt(r, a) { return (cx + r * Math.sin(a)).toFixed(2) + " " + (cy - r * Math.cos(a)).toFixed(2); }
    return "M" + pt(r1, a0) + " A" + r1 + " " + r1 + " 0 " + large + " 1 " + pt(r1, a1) +
      " L" + pt(r0, a1) + " A" + r0 + " " + r0 + " 0 " + large + " 0 " + pt(r0, a0) + " Z";
  }

  function sunburst(svg, opts) {
    opts = opts || {};
    var size = opts.size || 400, cx = size / 2, cy = size / 2;
    var rings = opts.rings || 4;
    var rand = seeded(opts.seed || 7);
    var core = size * 0.14, band = (size / 2 - core - 4) / rings;
    svg.setAttribute("viewBox", "0 0 " + size + " " + size);
    svg.classList.add("da-sunburst");
    while (svg.firstChild) svg.removeChild(svg.firstChild);
    var g = document.createElementNS(SVG_NS, "g");
    if (opts.spin) g.setAttribute("class", "da-spin");
    svg.appendChild(g);

    var palette = KINDS.filter(function (k) { return k.id !== "hidden"; });
    var top = [];
    var weights = opts.weights || [0.24, 0.13, 0.07, 0.09, 0.11, 0.08, 0.1, 0.09, 0.05, 0.04];
    weights.forEach(function (w, i) { top.push({ kind: palette[i % palette.length], share: w }); });
    var sum = top.reduce(function (a, b) { return a + b.share; }, 0);

    function draw(depth, a0, a1, kind) {
      if (depth >= rings || a1 - a0 < 0.05) return;
      var r0 = core + 4 + depth * band, r1 = r0 + band - 3;
      var p = document.createElementNS(SVG_NS, "path");
      p.setAttribute("d", arcPath(cx, cy, r0, r1, a0, a1));
      p.setAttribute("class", "seg");
      p.setAttribute("style", "fill:var(--kind-" + kind.id + ");fill-opacity:" + (1 - depth * 0.13).toFixed(2) + ";stroke:var(--well);stroke-width:2");
      var t = document.createElementNS(SVG_NS, "title");
      t.textContent = kind.label;
      p.appendChild(t);
      g.appendChild(p);
      var kids = 1 + Math.floor(rand() * (depth === 0 ? 3 : 2)), filled = 0.6 + rand() * 0.4, span = (a1 - a0) * filled, at = a0;
      for (var i = 0; i < kids; i++) {
        var part = i === kids - 1 ? a0 + span - at : span * (0.25 + rand() * 0.5);
        var childKind = rand() < 0.18 ? palette[Math.floor(rand() * palette.length)] : kind;
        draw(depth + 1, at, at + part, childKind);
        at += part;
      }
    }

    var a = 0;
    top.forEach(function (t) {
      var span = (t.share / sum) * Math.PI * 2;
      draw(0, a, a + span, t.kind);
      a += span;
    });
    var disc = document.createElementNS(SVG_NS, "circle");
    disc.setAttribute("cx", cx); disc.setAttribute("cy", cy); disc.setAttribute("r", core);
    disc.setAttribute("style", "fill:var(--accent)");
    svg.appendChild(disc);
    if (opts.centerLabel) {
      var label = document.createElementNS(SVG_NS, "text");
      label.setAttribute("x", cx); label.setAttribute("y", cy + 5);
      label.setAttribute("text-anchor", "middle");
      label.setAttribute("style", "fill:var(--on-accent);font:600 " + Math.round(core * 0.32) + "px var(--font-mono)");
      label.textContent = opts.centerLabel;
      svg.appendChild(label);
    }
    return svg;
  }

  function typeStrip(container, items) {
    var total = items.reduce(function (a, b) { return a + b.bytes; }, 0);
    var strip = document.createElement("div");
    strip.className = "da-strip";
    strip.setAttribute("role", "img");
    strip.setAttribute("aria-label", "Space per file type");
    var legend = document.createElement("ul");
    legend.className = "da-legend";
    items.forEach(function (it) {
      var kind = KINDS.filter(function (k) { return k.id === it.kind; })[0];
      var seg = document.createElement("span");
      seg.style.flex = String(it.bytes / total);
      seg.style.background = "var(--kind-" + it.kind + ")";
      seg.title = kind.label + " · " + formatBytes(it.bytes);
      strip.appendChild(seg);
      var li = document.createElement("li");
      li.innerHTML = '<i style="background:var(--kind-' + it.kind + ')"></i>' + kind.label + " <b>" + formatBytes(it.bytes) + "</b>";
      legend.appendChild(li);
    });
    container.appendChild(strip);
    container.appendChild(legend);
  }

  window.DiskAnalyzer = {
    REPO: REPO,
    KINDS: KINDS,
    PLATFORMS: PLATFORMS,
    detectOS: detectOS,
    resolveRelease: resolveRelease,
    wireDownloads: wireDownloads,
    formatBytes: formatBytes,
    sunburst: sunburst,
    typeStrip: typeStrip
  };
})();

(function () {
  var DA = window.DiskAnalyzer;
  DA.sunburst(document.getElementById('burst'), { size: 480, rings: 4, seed: 7, spin: true });
  var os = DA.wireDownloads(document) || 'macos';
  document.querySelectorAll('[data-da-card]').forEach(function (c) {
    var mine = c.getAttribute('data-da-card') === os;
    var b = c.querySelector('.da-btn');
    b.classList.toggle('da-btn-primary', mine);
    b.classList.toggle('da-btn-secondary', !mine);
  });
  if (os === 'macos') document.querySelectorAll('[data-mod]').forEach(function (k) { k.textContent = '⌘'; });
})();
