// Annotate picker, page half (spec 02). live.rs injects this into every live page (document end, main
// frame only). It stays idle until the app calls `window.__protoAnnotate.apply(on, dark, zoom, pins)`:
// before that nothing is mounted and no pointer or click handling exists; only Esc, the batched redraw
// on scroll/resize and (once mounted) the keyboard shield of the overlay are wired up.
// Messages go to the `protoAnnotate` channel as JSON strings `{ t: kind, ...data }` (see annotate.rs).
(() => {
  "use strict";
  const ENTRY = "__protoAnnotate";
  if (window[ENTRY]) return; // already running in this page

  // Keep our own references; pages may patch globals later.
  const W = window;
  const D = document;
  const stringify = JSON.stringify.bind(JSON);
  const computed = W.getComputedStyle.bind(W);
  const nextFrame = W.requestAnimationFrame.bind(W);
  const later = W.setTimeout.bind(W);
  const cssEscape = W.CSS && typeof W.CSS.escape === "function" ? W.CSS.escape.bind(W.CSS) : (s) => String(s).replace(/[^A-Za-z0-9_-]/g, (c) => "\\" + c);

  const CHANNEL = "protoAnnotate";
  const HOST_TAG = "proto-annotate-overlay";
  const POP_W = 300;
  const EDGE = 8;
  const PIN = 22;
  const SVG_NS = "http://www.w3.org/2000/svg";

  const send = (t, data) => {
    try {
      W.webkit.messageHandlers[CHANNEL].postMessage(stringify(Object.assign({ t }, data || {})));
    } catch (e) {
      // No channel (plain browser) or a failing post: ignore.
    }
  };

  // ---------------------------------------------------------------------------------------------
  // Element description ("facts")
  // ---------------------------------------------------------------------------------------------

  const norm = (s) => String(s == null ? "" : s).replace(/\s+/g, " ").trim();
  const clip = (s, n) => (s.length <= n ? s : s.slice(0, Math.max(0, n - 1)).replace(/\s+$/, "") + "…");
  const tagOf = (el) => String(el.localName || el.nodeName || "").toLowerCase();
  const attr = (el, name) => (el && el.getAttribute ? el.getAttribute(name) : null);
  const isSvg = (el) => el.namespaceURI === SVG_NS;
  const safe = (fn, fallback) => {
    try {
      return fn();
    } catch (e) {
      return fallback;
    }
  };

  const NO_TEXT = new Set(["script", "style", "noscript", "template"]);
  const BLOCKISH = new Set([
    "address", "article", "aside", "blockquote", "br", "button", "dd", "details", "div", "dl", "dt", "fieldset",
    "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "label", "li",
    "main", "nav", "ol", "option", "p", "pre", "section", "summary", "table", "tbody", "td", "tfoot", "th", "thead",
    "tr", "ul",
  ]);

  // Text content without script/style, with a space where block-level boundaries are; stops early.
  function textOf(el, limit) {
    const max = limit || 400;
    let out = "";
    const walk = (node) => {
      for (let c = node.firstChild; c && out.length < max; c = c.nextSibling) {
        if (c.nodeType === 3) out += c.data;
        else if (c.nodeType === 1) {
          const t = tagOf(c);
          if (NO_TEXT.has(t) || c === host) continue;
          const gap = BLOCKISH.has(t);
          if (gap) out += " ";
          walk(c);
          if (gap) out += " ";
        }
      }
    };
    walk(el);
    return norm(out);
  }

  // Generated parts of class names (CSS modules, CSS-in-JS hashes).
  const hashy = (s) => /^[A-Za-z0-9]{5,10}$/.test(s) && /\d/.test(s) && /[A-Za-z]/.test(s);
  function classStem(c) {
    if (/^(css|sc|jsx|emotion|svelte|astro|tw)-[A-Za-z0-9_-]+$/.test(c) && /\d/.test(c)) return "";
    if (c.length >= 6 && hashy(c) && /[a-z]/.test(c) && /[A-Z]/.test(c)) return "";
    const m = /^(.*[A-Za-z0-9])(__|--|_|-)([A-Za-z0-9]+)$/.exec(c);
    if (m && hashy(m[3]) && /[A-Za-z]/.test(m[1])) return m[1];
    return c;
  }
  const UTILITY = /[:[\]/!@]|^-?(m|p)[trblxyse]?-|^(flex|grid|block|inline|hidden|contents|relative|absolute|fixed|sticky|static|items-|justify-|content-|self-|place-|gap-|space-|text-|font-|leading-|tracking-|bg-|from-|via-|to-|w-|h-|min-|max-|size-|rounded|shadow|border|ring|outline|opacity-|z-|top-|left-|right-|bottom-|inset-|overflow|truncate|transition|duration-|ease-|animate-|cursor-|select-|pointer-|col-|row-|order-|basis-|grow|shrink|sr-only|underline|uppercase|lowercase|capitalize|italic|antialiased|aspect-|object-|fill-|stroke-|divide-|backdrop-|blur|filter|transform|translate-|scale-|rotate-|origin-|whitespace-|break-|align-|float-|clear-|visible|invisible)/;

  const rawClasses = (el) => {
    const v = norm(attr(el, "class"));
    return v ? v.split(" ") : [];
  };
  // Author classes that are neither generated nor utilities, deduplicated, in source order.
  const goodClasses = (el) => {
    const out = [];
    for (const c of rawClasses(el)) if (classStem(c) === c && !UTILITY.test(c) && !out.includes(c)) out.push(c);
    return out;
  };
  const stemClasses = (el) => {
    const out = [];
    for (const c of rawClasses(el)) {
      const s = classStem(c);
      if (s && s !== c && !out.includes(s)) out.push(s);
    }
    return out;
  };
  const classesText = (el) => {
    const out = [];
    for (const c of rawClasses(el)) {
      const s = classStem(c);
      if (s && !out.includes(s)) out.push(s);
    }
    return out.join(", ");
  };

  const generatedId = (id) => /^(:r|«r|radix-|headlessui-|mui-|react-aria|ember\d|ext-gen|yui_|__)/.test(id) || /^[0-9a-f-]{16,}$/i.test(id) || /\d{5,}/.test(id);

  // A CSS string literal; `|` is escaped so a selector never contains " | " (the target separator).
  const cssString = (s) =>
    '"' +
    String(s)
      .replace(/\\/g, "\\\\")
      .replace(/"/g, '\\"')
      .replace(/\|/g, "\\7c ")
      .replace(/[\n\r\f]/g, "\\a ")
      .replace(/[\u0000-\u0009\u000b\u000e-\u001f\u007f]/g, (c) => "\\" + c.charCodeAt(0).toString(16) + " ") +
    '"';

  const TEST_ATTRS = ["data-testid", "data-test", "data-qa", "data-cy"];
  const testAttr = (el) => TEST_ATTRS.find((a) => attr(el, a) != null);

  // The element's own selector part: id and test id when present, else a component hint, author
  // classes or class stems, else the tag.
  function ownSeg(el) {
    const tag = cssEscape(el.localName || tagOf(el));
    const id = attr(el, "id");
    const ta = testAttr(el);
    let s = id ? "#" + cssEscape(id) : "";
    if (ta) s = (s || tag) + "[" + ta + "=" + cssString(attr(el, ta)) + "]";
    if (s) return s;
    const comp = attr(el, "data-component");
    if (comp) return tag + "[data-component=" + cssString(comp) + "]";
    const good = goodClasses(el).slice(0, 2);
    if (good.length) return tag + good.map((c) => "." + cssEscape(c)).join("");
    const stems = stemClasses(el);
    if (stems.length) return tag + "[class*=" + cssString(stems[0]) + "]";
    return tag;
  }
  // Other stable ways to name an element directly.
  function altSegs(el) {
    const t = tagOf(el);
    const out = [];
    const href = attr(el, "href");
    if (t === "a" && href && href !== "#" && href.length <= 100) out.push("a[href=" + cssString(href) + "]");
    const name = attr(el, "name");
    if (/^(input|select|textarea|button)$/.test(t) && name && name.length <= 60) out.push(t + "[name=" + cssString(name) + "]");
    const src = attr(el, "src");
    if (/^(img|iframe|video)$/.test(t) && src && src.length <= 100) out.push(t + "[src=" + cssString(src) + "]");
    return out;
  }
  const isAnchor = (el) => {
    const id = attr(el, "id");
    return (id && !generatedId(id)) || testAttr(el) || attr(el, "data-component") || goodClasses(el).length > 0;
  };
  const parentOf = (el) => el.parentElement;
  const childIndex = (el) => {
    const p = el.parentNode;
    if (!p || !p.children) return 1;
    return Array.prototype.indexOf.call(p.children, el) + 1;
  };
  const scopeOf = (el) => {
    const r = el.getRootNode ? el.getRootNode() : D;
    return r && typeof r.querySelector === "function" ? r : D;
  };

  // Short re-find selector: the first match in the element's document (or shadow root) is the element.
  function shortPath(el) {
    const html = D.documentElement;
    if (el === html) return "html";
    if (el === D.body) return "body";
    const scope = scopeOf(el);
    const hits = (s) => safe(() => scope.querySelector(s) === el, false);
    const own = ownSeg(el);
    if (hits(own)) return own;
    for (const s of altSegs(el)) if (hits(s)) return s;
    const anchors = [];
    let depth = 1;
    for (let n = parentOf(el); n && n !== html && n !== D.body && depth <= 6; n = parentOf(n), depth++) if (isAnchor(n)) anchors.push([n, depth]);
    const join = (a, d, s) => ownSeg(a) + (d === 1 ? " > " : " ") + s;
    for (const [a, d] of anchors) {
      const s = join(a, d, own);
      if (hits(s)) return s;
    }
    const nth = own + ":nth-child(" + childIndex(el) + ")";
    const parent = parentOf(el);
    if (parent && parent !== html) {
      const s = (parent === D.body ? "body" : ownSeg(parent)) + " > " + nth;
      if (hits(s)) return s;
    }
    for (const [a, d] of anchors) {
      const s = join(a, d, nth);
      if (hits(s)) return s;
    }
    if (hits(nth)) return nth;
    // Positional chain upwards; ends at body at the latest.
    let chain = nth;
    for (let n = parent; n && n !== html; n = parentOf(n)) {
      if (n === D.body) return "body > " + chain;
      const id = attr(n, "id");
      chain = (id && !generatedId(id) ? ownSeg(n) : ownSeg(n) + ":nth-child(" + childIndex(n) + ")") + " > " + chain;
      if (hits(chain)) return chain;
    }
    return chain;
  }

  // One level of the structure path: tag plus id or a telling class.
  function levelName(el) {
    const t = tagOf(el);
    const id = attr(el, "id");
    if (id) return t + "#" + cssEscape(id);
    const c = goodClasses(el)[0] || stemClasses(el)[0];
    return c ? t + "." + cssEscape(c) : t;
  }
  function fullPath(el) {
    const html = D.documentElement;
    if (el === html) return "html";
    const parts = [];
    for (let n = el; n && n !== html; ) {
      parts.unshift(levelName(n));
      if (n === D.body) break;
      if (n.parentElement) n = n.parentElement;
      else {
        const r = n.getRootNode ? n.getRootNode() : null;
        if (r && r.host) {
          parts.unshift("#shadow-root");
          n = r.host;
        } else break;
      }
    }
    return parts.join(" > ");
  }

  const KINDS = {
    a: "link", button: "button", textarea: "textarea", select: "select", option: "option", label: "label",
    img: "image", picture: "image", video: "video", audio: "audio", canvas: "canvas", iframe: "iframe",
    p: "paragraph", li: "list item", ul: "list", ol: "list", dl: "list", table: "table", tr: "table row",
    td: "table cell", th: "table header", nav: "navigation", header: "header", footer: "footer", main: "main",
    aside: "sidebar", section: "section", article: "article", form: "form", dialog: "dialog", fieldset: "fieldset",
    span: "text", strong: "text", em: "text", b: "text", small: "text", code: "code", pre: "code block",
    blockquote: "quote", hr: "divider", body: "body", html: "document", div: "container", figure: "figure",
    figcaption: "caption", summary: "summary", details: "details", math: "formula",
  };
  const INPUT_KINDS = { checkbox: "checkbox", radio: "radio button", range: "slider", button: "button", submit: "button", reset: "button", image: "button", color: "color picker" };

  function kindOf(el) {
    const t = tagOf(el);
    const role = norm(attr(el, "role")).split(" ")[0];
    if (role) return role.toLowerCase();
    if (/^h[1-6]$/.test(t)) return t;
    if (t === "input") {
      const type = norm(attr(el, "type")).toLowerCase() || "text";
      return INPUT_KINDS[type] || type + " input";
    }
    if (t === "i") return textOf(el, 40) ? "text" : "icon";
    return KINDS[t] || t;
  }

  const quoted = (s) => '"' + s + '"';
  const shortHref = (h) => h.replace(/^[a-z]+:\/\//i, "").replace(/^www\./i, "");
  const fileName = (u) => {
    const s = String(u).split(/[?#]/)[0];
    return s.slice(s.lastIndexOf("/") + 1) || s;
  };
  const labelledBy = (el) =>
    norm(attr(el, "aria-labelledby"))
      .split(" ")
      .filter(Boolean)
      .map((id) => {
        const n = D.getElementById(id);
        return n ? textOf(n, 120) : "";
      })
      .filter(Boolean)
      .join(" ");
  const labelFor = (el) => {
    const id = attr(el, "id");
    if (!id) return "";
    const l = safe(() => D.querySelector("label[for=" + cssString(id) + "]"), null);
    return l ? textOf(l, 120) : "";
  };

  // Best identifier of a (non-SVG) element: page text in quotes, or a class/id hint.
  function identOf(el) {
    const t = tagOf(el);
    const field = /^(input|textarea|select)$/.test(t);
    if (!field && t !== "img" && t !== "iframe" && t !== "html") {
      const tx = textOf(el, 200);
      if (tx) return quoted(tx);
    }
    if (t === "input" && /^(button|submit|reset)$/i.test(norm(attr(el, "type")))) {
      const v = norm(el.value);
      if (v) return quoted(v);
    }
    if (t === "select") {
      const o = el.selectedOptions && el.selectedOptions[0];
      const v = o ? norm(o.textContent) : "";
      if (v) return quoted(v);
    }
    const named = [norm(attr(el, "aria-label")), labelledBy(el), norm(attr(el, "alt")), norm(attr(el, "placeholder")), field ? labelFor(el) : "", norm(attr(el, "name")), norm(attr(el, "title"))].find(Boolean);
    if (named) return quoted(named);
    const href = norm(attr(el, "href"));
    if (t === "a" && href) return "to " + quoted(shortHref(href));
    const src = norm(attr(el, "src"));
    if (src && /^(img|iframe|video|audio)$/.test(t)) return quoted(fileName(src));
    const c = goodClasses(el)[0] || stemClasses(el)[0];
    if (c) return "." + c;
    const id = attr(el, "id");
    if (id) return "#" + id;
    return "";
  }
  function compose(kind, ident) {
    if (!ident) return clip(kind, 80);
    if (ident[0] === '"' && ident.length > 1) {
      const room = 80 - kind.length - 3;
      if (room >= 8) return kind + " " + quoted(clip(ident.slice(1, -1), room));
    } else if (ident.startsWith('to "')) {
      const room = 80 - kind.length - 6;
      if (room >= 8) return kind + " to " + quoted(clip(ident.slice(4, -1), room));
    }
    return clip(kind + " " + ident, 80);
  }
  const INTERACTIVE = /^(a|button|input|select|textarea|label|summary)$/;
  function baseLabel(el) {
    const own = attr(el, "data-element");
    if (own) return own;
    return compose(kindOf(el), identOf(el));
  }
  // Nearest named or operable ancestor, for graphics without a name of their own.
  function contextLabel(el) {
    for (let n = el.parentElement; n && n !== D.documentElement && n !== D.body; n = n.parentElement) {
      if (isSvg(n)) continue;
      const id = attr(n, "id");
      if (INTERACTIVE.test(tagOf(n)) || attr(n, "role") || attr(n, "aria-label") || attr(n, "data-element") || goodClasses(n).length || (id && !generatedId(id))) return baseLabel(n);
    }
    return "";
  }
  function svgIdent(el) {
    const a = norm(attr(el, "aria-label"));
    if (a) return quoted(a);
    const title = Array.prototype.find.call(el.children || [], (c) => tagOf(c) === "title");
    if (title && norm(title.textContent)) return quoted(norm(title.textContent));
    const c = goodClasses(el)[0] || stemClasses(el)[0];
    if (c) return "." + c;
    const id = attr(el, "id");
    if (id && !generatedId(id)) return "#" + id;
    return "";
  }
  function svgLabel(el) {
    const t = tagOf(el);
    const svg = t === "svg" ? el : el.ownerSVGElement || (el.closest && el.closest("svg"));
    if (!svg) return compose(t, svgIdent(el));
    const r = svg.getBoundingClientRect();
    const kind = r.width <= 48 && r.height <= 48 ? "icon" : "graphic";
    const name = svgIdent(svg);
    const where = name ? "" : contextLabel(svg);
    const tail = name ? " " + name : where ? " in " + where : "";
    if (el === svg) return clip(kind + tail, 80);
    const own = svgIdent(el);
    return clip(t + (own ? " " + own : "") + " of " + kind + tail, 80);
  }
  function label(el) {
    const own = attr(el, "data-element");
    if (own) return own;
    if (isSvg(el)) return svgLabel(el);
    return baseLabel(el);
  }

  // Text of the nearest sibling in a direction (text node or element), "" when it has none.
  function siblingText(el, dir) {
    for (let s = el[dir]; s; s = s[dir]) {
      if (s.nodeType === 3) {
        const v = norm(s.data);
        if (v) return v;
        continue;
      }
      if (s.nodeType !== 1 || NO_TEXT.has(tagOf(s)) || s === host) continue;
      return textOf(s, 200);
    }
    return "";
  }
  function nearbyText(el) {
    const parts = [];
    const before = siblingText(el, "previousSibling");
    if (before) parts.push("[before: " + quoted(clip(before, 80)) + "]");
    const own = /^(input|textarea|select|html|body)$/.test(tagOf(el)) ? "" : textOf(el, 200);
    if (own) parts.push(clip(own, 100));
    const after = siblingText(el, "nextSibling");
    if (after) parts.push("[after: " + quoted(clip(after, 80)) + "]");
    return clip(parts.join(" "), 300);
  }

  function siblingName(el) {
    let s = levelName(el);
    const t = tagOf(el);
    const role = norm(attr(el, "role"));
    if (t === "a" || t === "button" || role === "button" || role === "link" || role === "tab") {
      const tx = textOf(el, 60) || norm(attr(el, "aria-label"));
      if (tx) s += " " + quoted(clip(tx, 24));
    }
    return s;
  }
  function nearbyElements(el) {
    const p = el.parentNode;
    if (!p || !p.children) return "";
    const kids = Array.prototype.filter.call(p.children, (k) => k !== host && !NO_TEXT.has(tagOf(k)));
    const sibs = kids.filter((k) => k !== el);
    if (!sibs.length) return "";
    let s = sibs.slice(0, 4).map(siblingName).join(", ");
    if (sibs.length > 4) {
      const owner = p.nodeType === 11 ? "a shadow root" : levelName(p);
      s += ", … (" + kids.length + " children in " + owner + ", this is no. " + (kids.indexOf(el) + 1) + ")";
    }
    return clip(s, 300);
  }

  const TEXTY = /^(h[1-6]|p|span|label|li|a|strong|em|b|i|small|code|pre|blockquote|dt|dd|figcaption|caption|th|td|legend|summary|q|cite|mark|sup|sub|abbr|time|option)$/;
  const FIELDS = /^(button|input|select|textarea)$/;
  const MEDIA = /^(img|picture|video|audio|canvas|iframe|svg|object|embed)$/;
  const PROPS = {
    shape: ["fill", "stroke", "stroke-width", "opacity"],
    field: ["background-color", "color", "padding", "border-radius", "font-size", "font-weight", "position"],
    media: ["width", "height", "object-fit", "border-radius", "display", "position"],
    link: ["color", "font-size", "font-weight", "text-decoration-line", "line-height", "font-family"],
    text: ["color", "font-size", "font-weight", "font-family", "line-height", "text-align", "background-color"],
    box: ["display", "position", "flex-direction", "padding", "margin", "gap", "background-color", "border-radius"],
  };
  const DEFAULTISH = new Set(["", "none", "normal", "auto", "0px", "0px 0px", "0px 0px 0px 0px", "rgba(0, 0, 0, 0)", "transparent", "static", "visible", "initial"]);
  function isDefault(prop, v, cs) {
    if (DEFAULTISH.has(v)) return true;
    if (prop === "font-weight" && v === "400") return true;
    if (prop === "opacity" && v === "1") return true;
    if (prop === "stroke-width" && v === "1px") return true;
    if (prop === "text-align" && (v === "start" || v === "left")) return true;
    if (prop === "flex-direction") return v === "row" || !/flex/.test(cs.getPropertyValue("display"));
    return false;
  }
  function stylesOf(el) {
    const t = tagOf(el);
    const role = norm(attr(el, "role"));
    const group = isSvg(el) && t !== "svg" ? "shape" : FIELDS.test(t) || role === "button" ? "field" : MEDIA.test(t) ? "media" : t === "a" ? "link" : TEXTY.test(t) ? "text" : "box";
    const cs = computed(el);
    const out = [];
    for (const prop of PROPS[group]) {
      if (out.length >= 8) break;
      const v = norm(cs.getPropertyValue(prop));
      if (!isDefault(prop, v, cs)) out.push(prop + ": " + v);
    }
    return out.join("; ");
  }

  function focusable(el) {
    const t = tagOf(el);
    if (el.disabled === true) return false;
    const ti = attr(el, "tabindex");
    if (ti != null && norm(ti) !== "") return true;
    if ((t === "a" || t === "area") && attr(el, "href") != null) return true;
    if (/^(button|select|textarea|iframe|summary)$/.test(t)) return true;
    if (t === "input") return norm(attr(el, "type")).toLowerCase() !== "hidden";
    if ((t === "video" || t === "audio") && attr(el, "controls") != null) return true;
    const ce = attr(el, "contenteditable");
    return ce === "" || ce === "true" || ce === "plaintext-only";
  }
  function a11yOf(el) {
    const out = [];
    const role = norm(attr(el, "role"));
    if (role) out.push("role=" + quoted(role));
    const lab = norm(attr(el, "aria-label"));
    if (lab) out.push("aria-label=" + quoted(clip(lab, 80)));
    const lby = norm(attr(el, "aria-labelledby"));
    if (lby) out.push("aria-labelledby=" + quoted(lby));
    const desc = norm(attr(el, "aria-describedby"));
    if (desc) out.push("aria-describedby=" + quoted(desc));
    const ti = norm(attr(el, "tabindex"));
    if (ti) out.push("tabindex=" + ti);
    if (norm(attr(el, "aria-hidden")).toLowerCase() === "true") out.push("aria-hidden");
    for (const a of ["aria-expanded", "aria-selected", "aria-checked", "aria-pressed", "aria-current", "aria-disabled"]) {
      const v = norm(attr(el, a));
      if (v) out.push(a + "=" + quoted(v));
    }
    if (el.disabled === true) out.push("disabled");
    if (focusable(el)) out.push("focusable");
    return out.join(", ");
  }

  const REQUIRED_ATTRS = [...TEST_ATTRS, "data-component", "alt", "aria-label", "href", "id", "name", "placeholder", "role", "src", "type"];
  function attrsOf(el) {
    const out = Object.create(null);
    let n = 0;
    for (const a of REQUIRED_ATTRS) {
      const v = attr(el, a);
      if (v != null) {
        out[a] = v.slice(0, 500);
        n++;
      }
    }
    for (const at of Array.from(el.attributes || [])) {
      if (n >= 16) break;
      const name = at.name;
      if (name in out || name === "class" || name === "style") continue;
      if (/^(data-|aria-)/.test(name) || name === "title" || name === "for" || name === "tabindex") {
        out[name] = String(at.value).slice(0, 500);
        n++;
      }
    }
    return out;
  }

  const finite = (v) => (typeof v === "number" && isFinite(v) ? v : 0);
  const pageRect = (el) => {
    const r = el.getBoundingClientRect();
    return [finite(r.left), finite(r.top), finite(r.width), finite(r.height)];
  };
  const selectedText = () => {
    const s = safe(() => W.getSelection(), null);
    const v = s ? norm(s.toString()) : "";
    return v || null;
  };

  // The eleven facts of one element (spec G); every part falls back to an empty value.
  function describe(el) {
    return {
      label: safe(() => label(el), "") || tagOf(el) || "element",
      path: safe(() => shortPath(el), "") || cssEscape(tagOf(el) || "*"),
      fullPath: safe(() => fullPath(el), ""),
      classes: safe(() => classesText(el), ""),
      nearbyText: safe(() => nearbyText(el), ""),
      nearbyElements: safe(() => nearbyElements(el), ""),
      selectedText: safe(selectedText, null),
      styles: safe(() => stylesOf(el), ""),
      a11y: safe(() => a11yOf(el), ""),
      attrs: safe(() => attrsOf(el), {}),
      rect: safe(() => pageRect(el), [0, 0, 0, 0]),
    };
  }

  // ---------------------------------------------------------------------------------------------
  // Overlay
  // ---------------------------------------------------------------------------------------------

  const CSS = `
* { box-sizing: border-box; font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", system-ui, sans-serif; }
.layer { position: static; }
.hover { position: fixed; display: none; border: 1.5px solid #3b82f6; background: rgba(59,130,246,0.08); border-radius: 3px; pointer-events: none; }
.chip { position: fixed; display: none; background: #3b82f6; color: #fff; font-size: 11px; line-height: 16px; padding: 2px 6px; border-radius: 5px; max-width: 420px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; pointer-events: none; }
.chip b { font-weight: 600; }
.chip i { font-style: normal; opacity: 0.8; margin-left: 6px; }
.sel { position: fixed; border: 2px solid #3b82f6; background: rgba(59,130,246,0.12); border-radius: 3px; pointer-events: none; }
.pin { position: fixed; width: 22px; height: 22px; border-radius: 50%; display: flex; align-items: center; justify-content: center; color: #fff; font-size: 11px; font-weight: 600; line-height: 1; border: 1.5px solid rgba(255,255,255,0.9); box-shadow: 0 2px 8px rgba(0,0,0,0.35); cursor: pointer; pointer-events: auto; user-select: none; -webkit-user-select: none; }
.pop { position: fixed; width: 300px; border-radius: 12px; padding: 10px; font-size: 13px; line-height: normal; text-align: left; pointer-events: auto; -webkit-backdrop-filter: blur(20px) saturate(1.3); backdrop-filter: blur(20px) saturate(1.3); box-shadow: 0 10px 30px -12px rgba(0,0,0,0.6); background: rgba(23,23,23,0.94); color: #fafafa; border: 1px solid rgba(255,255,255,0.1); }
.light .pop { background: rgba(255,255,255,0.94); color: #0a0a0a; border-color: rgba(0,0,0,0.1); }
.head { font-size: 11px; opacity: 0.65; margin: 0 2px 8px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.status { font-size: 11px; opacity: 0.65; margin: 0 2px 6px; }
.msg { margin: 0 2px 8px; line-height: 18px; white-space: pre-wrap; overflow-wrap: anywhere; }
.who { display: block; font-size: 11px; opacity: 0.6; font-style: normal; }
textarea { display: block; width: 100%; min-height: 64px; margin: 0; resize: none; border-radius: 8px; padding: 7px 8px; font-size: 13px; line-height: 18px; outline: none; color: inherit; background: rgba(255,255,255,0.06); border: 1px solid rgba(255,255,255,0.14); }
.light textarea { background: rgba(0,0,0,0.04); border-color: rgba(0,0,0,0.12); }
.thread textarea { min-height: 40px; }
textarea:focus, .light textarea:focus { border-color: #3b82f6; }
.row { display: flex; gap: 6px; justify-content: flex-end; margin-top: 8px; }
button { all: unset; box-sizing: border-box; font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", system-ui, sans-serif; cursor: pointer; padding: 4px 10px; border-radius: 999px; font-size: 12px; font-weight: 500; line-height: normal; background: rgba(255,255,255,0.1); color: #fafafa; }
.light button { background: rgba(0,0,0,0.06); color: #0a0a0a; }
button.primary, .light button.primary { background: #1447e6; color: #fff; }
button:focus-visible { outline: 2px solid rgba(59,130,246,0.6); outline-offset: 1px; }
`;
  const HOST_STYLE = "all: initial !important; display: block !important; position: fixed !important; inset: 0 !important; z-index: 2147483647 !important; pointer-events: none !important;";

  let host = null; // the overlay host, created by the first `apply`
  let ui = null; // { root, layer, hover, chip, chipTag, chipPath, sels, pins, pops }

  const st = {
    on: false,
    dark: true,
    zoom: 1,
    pins: [],
    sel: [], // [{ el, facts }]
    pop: null, // { kind: "note" | "thread", node, field, pinId? }
    missingSent: "[]",
    frame: 0,
    hoverEl: null,
  };

  const make = (tag, cls, text) => {
    const e = D.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  };
  const box = (e, x, y, w, h) => {
    e.style.left = x + "px";
    e.style.top = y + "px";
    if (w != null) e.style.width = Math.max(0, w) + "px";
    if (h != null) e.style.height = Math.max(0, h) + "px";
  };
  // Element rect in overlay px.
  const overlayRect = (el) => {
    const [x, y, w, h] = pageRect(el);
    const z = st.zoom;
    return [x * z, y * z, w * z, h * z];
  };
  const viewport = () => [W.innerWidth * st.zoom, W.innerHeight * st.zoom];

  function build() {
    host = D.createElement(HOST_TAG);
    host.setAttribute("style", HOST_STYLE);
    const root = host.attachShadow({ mode: "open" });
    const style = make("style");
    style.textContent = CSS;
    const layer = make("div", "layer dark");
    const hover = make("div", "hover");
    const chip = make("div", "chip");
    const chipTag = make("b");
    const chipPath = make("i");
    chip.append(chipTag, chipPath);
    const sels = make("div");
    const pins = make("div");
    const pops = make("div");
    layer.append(hover, chip, sels, pins, pops);
    root.append(style, layer);
    // Keyboard (and pointer) events from the overlay stop at the shadow root: page shortcuts and
    // "click outside" handlers never see them bubble. Capture listeners of the page still do.
    for (const type of ["keydown", "keyup", "keypress", "input", "beforeinput", "pointerdown", "pointerup", "mousedown", "mouseup", "click", "dblclick", "contextmenu", "auxclick"]) {
      root.addEventListener(type, (e) => e.stopPropagation());
    }
    ui = { root, layer, hover, chip, chipTag, chipPath, sels, pins, pops };
  }
  function mount() {
    if (!host) build();
    const html = D.documentElement;
    if (html && host.parentNode !== html) html.appendChild(host);
  }

  const isOwn = (e) => !!host && typeof e.composedPath === "function" && e.composedPath().indexOf(host) >= 0;

  // Topmost pickable element of the main document at a viewport point.
  function targetAt(x, y) {
    const el = safe(() => D.elementFromPoint(x, y), null);
    if (!el || el.nodeType !== 1 || el === D.documentElement || el === host) return null;
    return el;
  }

  function hideHover() {
    if (!ui) return;
    ui.hover.style.display = "none";
    ui.chip.style.display = "none";
    st.hoverEl = null;
  }
  function showHover(el) {
    const [x, y, w, h] = overlayRect(el);
    box(ui.hover, x, y, w, h);
    ui.hover.style.display = "block";
    if (st.hoverEl !== el) {
      st.hoverEl = el;
      ui.chipTag.textContent = tagOf(el);
      ui.chipPath.textContent = safe(() => shortPath(el), "");
    }
    const [vw] = viewport();
    box(ui.chip, Math.max(0, Math.min(x, vw - 200)), y >= 22 ? y - 20 : y + h + 4);
    ui.chip.style.display = "block";
  }

  function drawSelection() {
    if (!ui) return;
    ui.sels.textContent = "";
    for (const s of st.sel) {
      if (!s.el.isConnected) continue; // removed from the page: keep its facts, hide its frame
      const [x, y, w, h] = overlayRect(s.el);
      const f = make("div", "sel");
      box(f, x, y, w, h);
      ui.sels.appendChild(f);
    }
  }

  const PIN_COLORS = { acknowledged: "#f59e0b", resolved: "#22c55e" };
  function drawPins(always) {
    if (!ui) return;
    ui.pins.textContent = "";
    const missing = [];
    const z = st.zoom;
    for (const p of st.pins) {
      const el = p.path ? safe(() => D.querySelector(p.path), null) : null;
      if (!el) {
        missing.push(p.id);
        continue;
      }
      const r = el.getBoundingClientRect();
      const w = finite(r.width) * z;
      const h = finite(r.height) * z;
      const cx = finite(r.left) * z + Math.min(12, w / 2);
      const cy = finite(r.top) * z + Math.min(12, h / 2);
      const b = make("div", "pin", String(p.number));
      b.style.background = PIN_COLORS[p.status] || "#3b82f6";
      box(b, cx - PIN / 2, cy - PIN / 2);
      b.addEventListener("click", (e) => {
        e.stopPropagation();
        openThread(p, [cx - PIN / 2, cy - PIN / 2, PIN, PIN]);
      });
      ui.pins.appendChild(b);
    }
    const key = stringify(missing);
    if (always || key !== st.missingSent) {
      st.missingSent = key;
      send("missing", { ids: missing });
    }
  }

  // ---------------------------------------------------------------------------------------------
  // Popovers
  // ---------------------------------------------------------------------------------------------

  function placePopover(p, anchor) {
    const [vw, vh] = viewport();
    const h = p.node.offsetHeight || 150;
    let top = anchor[1] + anchor[3] + EDGE;
    if (top + h > vh - EDGE) top = Math.max(EDGE, anchor[1] - EDGE - h);
    const left = Math.min(Math.max(anchor[0], EDGE), vw - POP_W - EDGE);
    box(p.node, left, top);
  }
  function showPopover(p, anchor) {
    const wasOpen = !!st.pop;
    if (st.pop) st.pop.node.remove();
    st.pop = p;
    ui.pops.appendChild(p.node);
    placePopover(p, anchor);
    later(() => {
      if (st.pop === p) safe(() => p.field.focus({ preventScroll: true }));
    }, 0);
    if (!wasOpen) send("popover", { open: true });
  }
  function closePopover() {
    if (!st.pop) return false;
    st.pop.node.remove();
    st.pop = null;
    send("popover", { open: false });
    return true;
  }
  function cancelAll() {
    const had = !!st.pop || st.sel.length > 0;
    st.sel = [];
    drawSelection();
    closePopover();
    if (had) send("cancel");
  }

  function popoverShell(kind, head, placeholder, onEnter) {
    const node = make("div", kind === "thread" ? "pop thread" : "pop");
    node.appendChild(make("div", "head", head));
    const field = make("textarea");
    field.placeholder = placeholder;
    field.rows = kind === "thread" ? 2 : 3;
    field.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.keyCode !== 229) {
        e.preventDefault();
        onEnter();
      }
    });
    const row = make("div", "row");
    const button = (text, primary, act) => {
      const b = make("button", primary ? "primary" : "", text);
      b.type = "button";
      b.addEventListener("click", act);
      row.appendChild(b);
    };
    return { node, field, row, button };
  }

  function openNote() {
    const n = st.sel.length;
    const last = st.sel[n - 1].facts;
    const head = n === 1 ? last.label + " · " + last.path : n + " elements · " + last.label;
    const save = () => {
      const comment = p.field.value.trim();
      if (!comment || st.pop !== p) return;
      send("save", { comment, targets: st.sel.map((s) => s.facts) });
      st.sel = [];
      drawSelection();
      closePopover();
    };
    const shell = popoverShell("note", head, "What should change?", save);
    const p = { kind: "note", node: shell.node, field: shell.field };
    shell.node.appendChild(shell.field);
    shell.button("Cancel", false, cancelAll);
    shell.button("Save", true, save);
    shell.node.appendChild(shell.row);
    showPopover(p, overlayRect(st.sel[n - 1].el));
  }

  function openThread(pin, anchor) {
    const reply = () => {
      const text = p.field.value.trim();
      if (!text) return;
      send("reply", { id: pin.id, text });
      p.field.value = "";
    };
    const shell = popoverShell("thread", "#" + pin.number + " · " + pin.path, "Reply…", reply);
    const p = { kind: "thread", node: shell.node, field: shell.field, pinId: pin.id };
    shell.node.appendChild(make("div", "status", pin.status));
    shell.node.appendChild(make("div", "msg", pin.comment));
    for (const [who, text] of pin.thread) {
      const m = make("div", "msg");
      m.append(make("span", "who", who), D.createTextNode(text));
      shell.node.appendChild(m);
    }
    shell.node.appendChild(shell.field);
    const finish = (t) => () => {
      if (st.pop !== p) return;
      send(t, { id: pin.id });
      closePopover();
    };
    shell.button("Delete", false, finish("dismiss"));
    shell.button("Resolve", true, finish("resolve"));
    shell.node.appendChild(shell.row);
    showPopover(p, anchor);
  }

  // ---------------------------------------------------------------------------------------------
  // Pointer handling (only while on)
  // ---------------------------------------------------------------------------------------------

  const BLOCKED = ["pointerdown", "pointerup", "mousedown", "mouseup", "click", "dblclick", "contextmenu", "auxclick"];

  function onBlocked(e) {
    if (isOwn(e)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (e.type === "click") pick(e);
  }
  function pick(e) {
    // A click without a pointer position (script `el.click()`, keyboard activation) is only swallowed.
    if (e.clientX === 0 && e.clientY === 0 && e.screenX === 0 && e.screenY === 0 && !e.detail) return;
    const el = targetAt(e.clientX, e.clientY);
    if (!el) return;
    const facts = describe(el);
    if (!e.shiftKey) st.sel = [{ el, facts }];
    else if (!st.sel.some((s) => s.el === el)) st.sel.push({ el, facts });
    drawSelection();
    openNote();
  }
  function onMove(e) {
    if (isOwn(e)) return hideHover();
    const el = targetAt(e.clientX, e.clientY);
    if (el) showHover(el);
    else hideHover();
  }
  function listen(on) {
    const f = on ? W.addEventListener : W.removeEventListener;
    for (const type of BLOCKED) f.call(W, type, onBlocked, true);
    f.call(W, "pointermove", onMove, true);
  }

  // Esc: always wired (capture on window), but only acts while on or with a popover open.
  W.addEventListener(
    "keydown",
    (e) => {
      if (e.key !== "Escape" && e.key !== "Esc") return;
      if (!st.on && !st.pop) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      if (st.pop || st.sel.length) cancelAll();
      else send("escape");
    },
    true,
  );

  // Scroll (any element) and resize: one redraw per frame at most; idle otherwise.
  function redraw() {
    st.frame = 0;
    if (!ui) return;
    hideHover();
    drawSelection();
    drawPins(false);
  }
  const schedule = () => {
    if (!st.frame) st.frame = nextFrame(redraw);
  };
  W.addEventListener("scroll", schedule, { capture: true, passive: true });
  W.addEventListener("resize", schedule, { passive: true });

  // ---------------------------------------------------------------------------------------------
  // Entry
  // ---------------------------------------------------------------------------------------------

  const text = (v) => (typeof v === "string" ? v : v == null ? "" : String(v));
  function normPin(p) {
    if (!p || typeof p !== "object") return null;
    const thread = Array.isArray(p.thread) ? p.thread.filter(Array.isArray).map((m) => [text(m[0]), text(m[1])]) : [];
    return { id: text(p.id), number: Math.max(0, Math.floor(Number(p.number) || 0)), path: text(p.path), comment: text(p.comment), status: text(p.status), thread };
  }

  function apply(on, dark, zoom, pins) {
    on = !!on;
    dark = !!dark;
    zoom = typeof zoom === "number" && isFinite(zoom) && zoom > 0 ? zoom : 1;
    pins = Array.isArray(pins) ? pins.map(normPin).filter(Boolean) : [];
    mount(); // (a)
    st.dark = dark; // (b)
    ui.layer.className = dark ? "layer dark" : "layer light";
    st.zoom = zoom; // (c)
    host.style.setProperty("zoom", String(1 / zoom), "important");
    if (st.pop && st.pop.kind === "note" && st.sel.length) {
      const last = st.sel[st.sel.length - 1].el; // (d)
      if (last.isConnected) placePopover(st.pop, overlayRect(last));
    }
    drawSelection(); // (e)
    const wasOn = st.on;
    if (on !== wasOn) {
      // (f)
      st.on = on;
      listen(on);
      if (!on) {
        hideHover();
        if (st.pop && st.pop.kind === "note") cancelAll();
        else {
          st.sel = [];
          drawSelection();
        }
      }
    }
    st.pins = pins; // (g)
    const rootStyle = D.documentElement.style; // (h)
    if (on) rootStyle.cursor = "crosshair";
    else if (wasOn) rootStyle.cursor = "";
    if (st.pop && st.pop.kind === "thread" && !pins.some((p) => p.id === st.pop.pinId)) closePopover(); // (i)
    drawPins(true); // (j)
    return { on: st.on, pins: pins.length };
  }

  // Test entry: facts of the first element matching `selector`, or null. Changes nothing.
  function describeSelector(selector) {
    const el = safe(() => D.querySelector(selector), null);
    return el ? describe(el) : null;
  }

  Object.defineProperty(W, ENTRY, {
    value: Object.freeze({ apply, describe: describeSelector, describeElement: (el) => (el && el.nodeType === 1 ? describe(el) : null) }),
    enumerable: false,
    configurable: false,
    writable: false,
  });
})();
