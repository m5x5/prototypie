//! Headless acceptance probe for the Annotate picker inside a `LiveView` (spec 02 section R, spec 04
//! AN-IF-551…559). An offscreen, borderless window (never shown, no Dock icon) hosts a live view; the
//! probe drives the page with synthetic DOM events only and sends nothing anywhere.
//!
//! Part 1 runs scenarios P-1…P-9 on the probe page and writes `annotate-popover-light.png`,
//! `annotate-thread-dark.png` and `annotate-popover-dark.png` into the directory given as the first
//! argument (default: current directory). Part 2 checks the element description against the
//! acceptance examples E-01…E-15 and the probe button (AN-WV-066/068) through the test entry.
//!
//! `cargo run -p proto-web --example picker_probe -- [out_dir]`

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSRunLoop, NSSize};
use proto_web::mac::annotate::{OVERLAY_TAG, SCRIPT};
use proto_web::{AnnotateEvent, LiveKey, LiveOptions, LivePage, LiveView, WebPick, WebPin};
use serde_json::Value;

const PROBE_PAGE: &str = r##"<!doctype html><html><head><meta charset="utf-8"><style>
  body { margin: 0; background: #f6f6f6; font: 16px -apple-system, system-ui, sans-serif; }
  .hero { padding: 48px; display: flex; flex-direction: column; gap: 16px; align-items: flex-start; }
  h1 { font-size: 32px; margin: 0; }
  .cta { padding: 10px 18px; border-radius: 10px; background: #1447e6; color: #fff; border: 0; font-size: 15px; }
</style></head><body>
<main class="hero">
  <h1>Climb higher</h1>
  <p class="lede">Bouldering for everyone.</p>
  <button id="go" class="cta" data-testid="start">Start trial</button>
  <a href="/prices">Prices</a>
</main>
<script>
  window.__clicks = 0; window.__docClicks = 0; window.__docKeys = 0;
  document.getElementById("go").addEventListener("click", () => window.__clicks++);
  document.addEventListener("click", () => window.__docClicks++);
  document.addEventListener("keydown", () => window.__docKeys++);
</script>
</body></html>"##;

// ---------------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------------

fn spin_until(mut done: impl FnMut() -> bool, limit: Duration) -> bool {
    let run_loop = NSRunLoop::currentRunLoop();
    let t = Instant::now();
    while !done() {
        if t.elapsed() > limit {
            return false;
        }
        unsafe { run_loop.runMode_beforeDate(NSDefaultRunLoopMode, &NSDate::dateWithTimeIntervalSinceNow(0.02)) };
    }
    true
}

fn pause(ms: u64) {
    spin_until(|| false, Duration::from_millis(ms));
}

struct Probe {
    live: LiveView,
    out: PathBuf,
    failures: RefCell<Vec<String>>,
    passed: RefCell<usize>,
    /// Events taken but not yet looked at.
    events: RefCell<Vec<AnnotateEvent>>,
}

/// Page helpers available to every probe script.
fn helpers() -> String {
    format!(
        r##"const ov = () => Array.from(document.documentElement.children).find(c => c.localName === "{OVERLAY_TAG}");
const sr = () => {{ const o = ov(); return o ? o.shadowRoot : null; }};
const pop = () => {{ const r = sr(); return r ? r.querySelector(".pop") : null; }};
const fire = (target, x, y, shift) => {{
  const base = {{ bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, screenX: x + 1, screenY: y + 1, shiftKey: !!shift, view: window }};
  target.dispatchEvent(new PointerEvent("pointermove", Object.assign({{ pointerId: 1, isPrimary: true }}, base)));
  target.dispatchEvent(new PointerEvent("pointerdown", Object.assign({{ pointerId: 1, isPrimary: true, button: 0, buttons: 1 }}, base)));
  target.dispatchEvent(new MouseEvent("mousedown", Object.assign({{ button: 0, buttons: 1, detail: 1 }}, base)));
  target.dispatchEvent(new PointerEvent("pointerup", Object.assign({{ pointerId: 1, isPrimary: true, button: 0 }}, base)));
  target.dispatchEvent(new MouseEvent("mouseup", Object.assign({{ button: 0, detail: 1 }}, base)));
  target.dispatchEvent(new MouseEvent("click", Object.assign({{ button: 0, detail: 1 }}, base)));
}};
const clickSel = (sel, shift) => {{
  const el = typeof sel === "string" ? document.querySelector(sel) : sel;
  const r = el.getBoundingClientRect();
  const x = r.left + r.width / 2, y = r.top + r.height / 2;
  fire(document.elementFromPoint(x, y) || el, x, y, shift);
  return [x, y];
}};
const typeEnter = (text) => {{
  const ta = sr().querySelector(".pop textarea");
  ta.value = text;
  ta.dispatchEvent(new KeyboardEvent("keydown", {{ key: "Enter", code: "Enter", bubbles: true, composed: true, cancelable: true }}));
  return true;
}};
const esc = () => window.dispatchEvent(new KeyboardEvent("keydown", {{ key: "Escape", code: "Escape", bubbles: true, cancelable: true }}));
"##
    )
}

impl Probe {
    fn eval(&self, js: &str) -> Value {
        let out = Rc::new(RefCell::new(None));
        let o = out.clone();
        let body = format!("{}\n{js}", helpers());
        self.live.eval(&body, move |r| *o.borrow_mut() = Some(r));
        if !spin_until(|| out.borrow().is_some(), Duration::from_secs(5)) {
            return Value::String("<eval timeout>".into());
        }
        match out.borrow_mut().take().unwrap() {
            Ok(v) => v,
            Err(e) => Value::String(format!("<eval error: {e}>")),
        }
    }

    fn check(&self, id: &str, ok: bool, detail: impl std::fmt::Display) {
        if ok {
            *self.passed.borrow_mut() += 1;
            println!("  ok   {id}");
        } else {
            println!("  FAIL {id}: {detail}");
            self.failures.borrow_mut().push(format!("{id}: {detail}"));
        }
    }

    /// Waits until at least `n` events arrived (≤ 3 s), returns all pending ones.
    fn events(&self, n: usize) -> Vec<AnnotateEvent> {
        spin_until(
            || {
                self.events.borrow_mut().extend(self.live.take_annotate_events());
                self.events.borrow().len() >= n
            },
            Duration::from_secs(3),
        );
        pause(40);
        self.events.borrow_mut().extend(self.live.take_annotate_events());
        std::mem::take(&mut self.events.borrow_mut())
    }

    fn load(&self, key: &str, rev: u64, html: &str, rect: [f32; 4], viewport_w: u32) -> bool {
        let page = LivePage { key, rev, html, viewport_w, viewport_h: None };
        self.live.show(rect, &page, &LiveOptions::default());
        let ok = spin_until(|| self.live.is_loaded(), Duration::from_secs(10));
        pause(100);
        ok
    }

    fn snapshot(&self, name: &str) {
        let out = Rc::new(RefCell::new(None));
        let o = out.clone();
        self.live.snapshot_rgba(move |px| *o.borrow_mut() = Some(px));
        spin_until(|| out.borrow().is_some(), Duration::from_secs(5));
        let Some(Some((rgba, w, h))) = out.borrow_mut().take() else {
            println!("  note snapshot {name} not available");
            return;
        };
        let path = self.out.join(name);
        let write = || -> anyhow::Result<()> {
            let file = std::io::BufWriter::new(std::fs::File::create(&path)?);
            let mut enc = png::Encoder::new(file, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()?.write_image_data(&rgba)?;
            Ok(())
        };
        match write() {
            Ok(()) => println!("  wrote {} ({w}x{h})", path.display()),
            Err(e) => println!("  note snapshot {name}: {e}"),
        }
    }
}

fn saves(events: &[AnnotateEvent]) -> Vec<(&String, &Vec<WebPick>)> {
    events
        .iter()
        .filter_map(|e| match e {
            AnnotateEvent::Save { comment, targets } => Some((comment, targets)),
            _ => None,
        })
        .collect()
}

fn pins() -> Vec<WebPin> {
    let thread = vec![("agent".to_string(), "On it.".to_string())];
    let pin = |id: &str, number, path: &str, status: &str| WebPin {
        id: id.into(),
        number,
        element_path: path.into(),
        comment: "zz-bench: bigger".into(),
        status: status.into(),
        thread: thread.clone(),
    };
    vec![pin("p1", 1, "#go", "acknowledged"), pin("p2", 2, "h1", "pending"), pin("p3", 3, "#nope", "pending")]
}

// ---------------------------------------------------------------------------------------------
// Part 1: scenarios P-1 … P-9
// ---------------------------------------------------------------------------------------------

fn scenarios(p: &Probe) -> bool {
    let rect = [20.0, 30.0, 400.0, 300.0];
    println!("P-1 annotate on (light) before loading");
    p.live.set_annotate(true, false);
    if !p.load("probe", 1, PROBE_PAGE, rect, 800) {
        p.check("P-1 load", false, "page did not load within 10 s");
        return false;
    }
    let v = p.eval(
        r##"const hosts = Array.from(document.documentElement.children).filter(c => c.localName === ov()?.localName);
return { hosts: hosts.length, last: document.documentElement.lastElementChild === ov(), open: !!(ov() && ov().shadowRoot),
  cursor: document.documentElement.style.cursor, z: getComputedStyle(ov()).zIndex, pe: getComputedStyle(ov()).pointerEvents };"##,
    );
    p.check("P-1 overlay mounted as last child of <html> with open shadow root", v["hosts"] == 1 && v["last"] == true && v["open"] == true, &v);
    p.check("P-1 root cursor is crosshair", v["cursor"] == "crosshair", &v);
    p.check("AN-WV-021 host z-index max, no pointer events", v["z"] == "2147483647" && v["pe"] == "none", &v);

    // AN-WV-003: running the script again changes nothing.
    let again = p.eval(&format!("{SCRIPT}\nreturn Array.from(document.documentElement.children).filter(c => c.localName === {OVERLAY_TAG:?}).length;"));
    p.check("AN-WV-003 second run keeps one overlay", again == 1, &again);

    // AN-WV-036: a script click on #go while on is swallowed and picks nothing.
    let v = p.eval(r##"document.getElementById("go").click(); await new Promise(r => setTimeout(r, 30)); return { clicks: __clicks, pop: !!pop() };"##);
    p.check("AN-WV-036 script click swallowed", v["clicks"] == 0 && v["pop"] == false, &v);

    // Spy on the page → app channel to check message order (AN-WV-093/118).
    let spy = p.eval(
        r##"const h = window.webkit && window.webkit.messageHandlers.protoAnnotate;
if (!h) return false;
window.__log = [];
window.__spied = h; // keep the wrapper alive: WebKit may hand out a fresh one after GC
const orig = h.postMessage.bind(h);
try { h.postMessage = (m) => { window.__log.push(JSON.parse(m).t + (JSON.parse(m).open === undefined ? "" : ":" + JSON.parse(m).open)); orig(m); }; } catch (e) { return false; }
return window.webkit.messageHandlers.protoAnnotate.postMessage !== orig;"##,
    );
    let spying = spy == true;
    if !spying {
        println!("  note message spy unavailable ({spy}); order checks skipped");
    }
    let log = |p: &Probe| p.eval("const l = window.__log || []; window.__log = []; return l;");

    println!("P-2 click #go");
    let v = p.eval(r##"clickSel("#go"); await new Promise(r => setTimeout(r, 30));
const r = sr(); const chip = r.querySelector(".chip");
return { clicks: __clicks, docClicks: __docClicks, pops: r.querySelectorAll(".pop").length, focused: r.activeElement && r.activeElement.localName,
  head: pop() && pop().querySelector(".head").textContent, placeholder: pop() && pop().querySelector("textarea").placeholder,
  buttons: pop() && Array.from(pop().querySelectorAll("button")).map(b => b.textContent),
  chip: chip.textContent, chipShown: chip.style.display, path: __protoAnnotate.describe("#go").path, label: __protoAnnotate.describe("#go").label,
  sels: r.querySelectorAll(".sel").length, popW: pop().getBoundingClientRect().width, popLeft: pop().style.left, popTop: pop().style.top,
  hover: r.querySelector(".hover").style.cssText, go: (() => { const b = document.getElementById("go").getBoundingClientRect(); return [b.left, b.top, b.width, b.height]; })() };"##);
    p.check("P-2 page click counter stays 0", v["clicks"] == 0 && v["docClicks"] == 0, &v);
    p.check("P-2 note popover open", v["pops"] == 1, &v);
    p.check("P-2 textarea focused", v["focused"] == "textarea", &v);
    let path = v["path"].as_str().unwrap_or_default().to_string();
    p.check("P-2 chip shows tag + short path", v["chip"].as_str() == Some(&format!("button{path}")) && v["chipShown"] == "block", &v["chip"]);
    p.check("AN-WV-070 header = label · path", v["head"].as_str() == Some(&format!("{} · {path}", v["label"].as_str().unwrap_or_default())), &v["head"]);
    p.check("AN-WV-070 placeholder and buttons", v["placeholder"] == "What should change?" && v["buttons"] == serde_json::json!(["Cancel", "Save"]), &v);
    p.check("AN-WV-052 one selection frame", v["sels"] == 1, &v["sels"]);
    // Overlay geometry at z = 0.5: overlay px = page px × 0.5; host zoom 2.
    let go: Vec<f64> = v["go"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    let hover_ok = go.len() == 4 && {
        let css = v["hover"].as_str().unwrap_or_default();
        let num = |k: &str| css.split(&format!("{k}: ")).nth(1).and_then(|r| r.split("px").next()).and_then(|n| n.parse::<f64>().ok()).unwrap_or(f64::NAN);
        ["left", "top", "width", "height"].iter().zip(&go).all(|(k, g)| (num(k) - g * 0.5).abs() < 0.001)
    };
    p.check("AN-WV-024/041 hover frame = element rect × z", hover_ok, &v["hover"]);
    // Popover: anchored 8 px below the button in overlay px, left-aligned (min 8).
    let want_top = (go.get(1).unwrap_or(&0.0) + go.get(3).unwrap_or(&0.0)) * 0.5 + 8.0;
    let top: f64 = v["popTop"].as_str().unwrap_or("0px").trim_end_matches("px").parse().unwrap_or(-1.0);
    p.check("AN-WV-072 popover 8 px below the anchor", (top - want_top).abs() < 0.01, format!("top {top}, want {want_top}"));
    println!("  info popover getBoundingClientRect().width = {} (page px; 300 overlay px at z 0.5 = 600 page px)", v["popW"]);
    if spying {
        let l = log(p);
        p.check("AN-WV-093 click opens: popover:true", l == serde_json::json!(["popover:true"]), &l);
    }
    p.snapshot("annotate-popover-light.png");

    // AN-WV-011: identical applies keep the popover and its text.
    let v = p.eval(r##"pop().querySelector("textarea").value = "draft";
__protoAnnotate.apply(true, false, 0.5, []); const r = __protoAnnotate.apply(true, false, 0.5, []);
return { pops: sr().querySelectorAll(".pop").length, text: pop() && pop().querySelector("textarea").value, r };"##);
    p.check("AN-WV-011 repeated apply keeps the popover and its text", v["pops"] == 1 && v["text"] == "draft", &v);
    p.check("AN-WV-010 apply returns on + pin count", v["r"] == serde_json::json!({"on": true, "pins": 0}), &v["r"]);
    let early = p.events(0);
    p.check("AN-WV-011 no cancel from repeated apply", early.is_empty(), format!("{early:?}"));
    if spying {
        log(p);
    }

    println!("P-3 type + Enter");
    let v = p.eval(r##"typeEnter("zz-bench: make the CTA larger"); return { pops: sr().querySelectorAll(".pop").length, sels: sr().querySelectorAll(".sel").length, docKeys: __docKeys };"##);
    let ev = p.events(1);
    let s = saves(&ev);
    p.check("P-3 exactly one Save", s.len() == 1 && ev.len() == 1, format!("{ev:?}"));
    if let Some((comment, targets)) = s.first() {
        p.check("P-3 comment", comment.as_str() == "zz-bench: make the CTA larger", comment);
        let t = &targets[0];
        p.check("P-3 label is button \"Start trial\"", t.element == "button \"Start trial\"", &t.element);
        p.check("P-3 short path contains #go", t.element_path.contains("#go"), &t.element_path);
        p.check("P-3 window_rect.x = 20 + left·0.5", (t.window_rect[0] - (20.0 + t.rect[0] * 0.5)).abs() <= 0.01, format!("{:?} / {:?}", t.window_rect, t.rect));
        p.check("AN-WV-115 window_rect y/w/h", (t.window_rect[1] - (30.0 + t.rect[1] * 0.5)).abs() <= 0.01 && (t.window_rect[2] - t.rect[2] * 0.5).abs() <= 0.01, format!("{:?}", t.window_rect));
        p.check("AN-WV-066 selected text null on click", t.selected_text.is_none(), format!("{:?}", t.selected_text));
    }
    p.check("AN-WV-075 popover and frames gone after save", v["pops"] == 0 && v["sels"] == 0, &v);
    p.check("AN-WV-094 typing in the popover never reaches document keydown", v["docKeys"] == 0, &v["docKeys"]);
    if spying {
        let l = log(p);
        p.check("AN-WV-118 save → save, popover:false", l == serde_json::json!(["save", "popover:false"]), &l);
    }

    println!("P-4 click a, shift-click h1, Enter");
    let v = p.eval(r##"clickSel("a"); await new Promise(r => setTimeout(r, 30)); clickSel("h1", true); await new Promise(r => setTimeout(r, 30));
clickSel("a", true); await new Promise(r => setTimeout(r, 30));
return { sels: sr().querySelectorAll(".sel").length, head: pop().querySelector(".head").textContent, h1: __protoAnnotate.describe("h1").label };"##);
    p.check("P-4 two selection frames (re-shift-click keeps order)", v["sels"] == 2, &v);
    p.check(
        "AN-WV-070 header \"2 elements · <last>\"",
        v["head"].as_str() == Some(&format!("2 elements · {}", v["h1"].as_str().unwrap_or_default())),
        &v["head"],
    );
    if spying {
        let l = log(p);
        p.check("AN-WV-093 popover replaced: one popover:true only", l == serde_json::json!(["popover:true"]), &l);
    }
    p.eval(r##"typeEnter("zz-bench: tighten"); return true;"##);
    let ev = p.events(1);
    let s = saves(&ev);
    let ok = s.len() == 1 && s[0].0 == "zz-bench: tighten" && s[0].1.len() == 2 && s[0].1[0].element.starts_with("link") && s[0].1[1].element.starts_with("h1");
    p.check("P-4 one Save with [link, h1]", ok, format!("{:?}", s.iter().map(|(c, t)| (c, t.iter().map(|x| &x.element).collect::<Vec<_>>())).collect::<Vec<_>>()));
    if spying {
        log(p);
    }

    println!("P-5 Esc with popover, Esc with nothing open");
    p.live.take_keys();
    p.eval(r##"clickSel("#go"); await new Promise(r => setTimeout(r, 30)); esc(); return true;"##);
    let ev = p.events(1);
    p.check("P-5 Esc with popover → Cancel", ev == vec![AnnotateEvent::Cancel], format!("{ev:?}"));
    if spying {
        let l = log(p);
        p.check("AN-WV-118 cancel → popover:false, cancel", l == serde_json::json!(["popover:true", "popover:false", "cancel"]), &l);
    }
    p.eval("esc(); return true;");
    pause(100);
    let keys = p.live.take_keys();
    p.check("P-5 Esc with nothing open → one LiveKey::Escape", keys == vec![LiveKey::Escape], format!("{keys:?}"));
    p.check("AN-WV-092 no Cancel without anything open", p.events(0).is_empty(), "");

    println!("P-6 annotate off (dark) + pins");
    p.live.set_annotate(false, true);
    p.live.set_pins(pins());
    spin_until(|| p.live.missing_pins() == ["p3"], Duration::from_secs(3));
    p.check("P-6 missing pins = [p3]", p.live.missing_pins() == ["p3"], format!("{:?}", p.live.missing_pins()));
    let v = p.eval(r##"const ps = Array.from(sr().querySelectorAll(".pin"));
const go = document.getElementById("go").getBoundingClientRect();
return { n: ps.length, nums: ps.map(x => x.textContent), colors: ps.map(x => getComputedStyle(x).backgroundColor), cursor: document.documentElement.style.cursor,
  left: ps[0] && ps[0].style.left, top: ps[0] && ps[0].style.top, want: [go.left * 0.5 + Math.min(12, go.width * 0.25) - 11, go.top * 0.5 + Math.min(12, go.height * 0.25) - 11] };"##);
    p.check("AN-WV-080/081 two pins drawn, numbers 1 and 2", v["n"] == 2 && v["nums"] == serde_json::json!(["1", "2"]), &v);
    p.check("AN-WV-081 colours amber / blue", v["colors"] == serde_json::json!(["rgb(245, 158, 11)", "rgb(59, 130, 246)"]), &v["colors"]);
    let want = v["want"].as_array().cloned().unwrap_or_default();
    let px = |s: &Value| s.as_str().unwrap_or("x").trim_end_matches("px").parse::<f64>().unwrap_or(f64::NAN);
    let pin_ok = want.len() == 2 && (px(&v["left"]) - want[0].as_f64().unwrap_or(0.0)).abs() < 0.01 && (px(&v["top"]) - want[1].as_f64().unwrap_or(0.0)).abs() < 0.01;
    p.check("AN-WV-082 pin centre 12 px inside the top-left corner", pin_ok, &v);
    p.check("AN-WV-010h cursor cleared when off", v["cursor"] == "", &v["cursor"]);

    println!("P-7 click #go while off");
    let v = p.eval(r##"clickSel("#go"); await new Promise(r => setTimeout(r, 30)); return { clicks: __clicks, pop: !!pop() };"##);
    p.check("P-7 click reaches the page", v["clicks"] == 1 && v["pop"] == false, &v);

    println!("P-8 pin → thread, reply, resolve");
    let v = p.eval(r##"const before = __docClicks; sr().querySelector(".pin").click(); await new Promise(r => setTimeout(r, 30));
const t = pop();
return { thread: !!t && t.classList.contains("thread"), head: t && t.querySelector(".head").textContent, status: t && t.querySelector(".status").textContent,
  msgs: t && Array.from(t.querySelectorAll(".msg")).map(m => m.textContent), placeholder: t && t.querySelector("textarea").placeholder,
  buttons: t && Array.from(t.querySelectorAll("button")).map(b => b.textContent), clicks: __clicks, docDelta: __docClicks - before,
  focused: sr().activeElement && sr().activeElement.localName, bg: t && getComputedStyle(t).backgroundColor };"##);
    p.check("P-8 thread popover open", v["thread"] == true, &v);
    p.check("AN-WV-085 head #1 · #go, status, comment, history", v["head"] == "#1 · #go" && v["status"] == "acknowledged" && v["msgs"] == serde_json::json!(["zz-bench: bigger", "agentOn it."]), &v);
    p.check("AN-WV-085 placeholder Reply… and Delete/Resolve", v["placeholder"] == "Reply…" && v["buttons"] == serde_json::json!(["Delete", "Resolve"]), &v);
    p.check("AN-WV-084/034 pin click reaches neither button nor document", v["clicks"] == 1 && v["docDelta"] == 0, &v);
    p.check("AN-WV-071 dark popover background", v["bg"] == "rgba(23, 23, 23, 0.94)", &v["bg"]);
    p.check("AN-WV-073 thread textarea focused", v["focused"] == "textarea", &v["focused"]);
    p.snapshot("annotate-thread-dark.png");
    p.eval(r##"typeEnter("zz-bench: thanks"); return true;"##);
    let ev = p.events(1);
    p.check("P-8 Reply event", ev == vec![AnnotateEvent::Reply { id: "p1".into(), text: "zz-bench: thanks".into() }], format!("{ev:?}"));
    let v = p.eval(r##"return { open: !!pop(), text: pop() && pop().querySelector("textarea").value };"##);
    p.check("AN-WV-086 reply keeps the thread open, field cleared", v["open"] == true && v["text"] == "", &v);
    if spying {
        log(p);
    }
    p.eval(r##"Array.from(pop().querySelectorAll("button")).find(b => b.textContent === "Resolve").click(); return true;"##);
    let ev = p.events(1);
    p.check("P-8 Resolve event", ev == vec![AnnotateEvent::Resolve { id: "p1".into() }], format!("{ev:?}"));
    if spying {
        let l = log(p);
        p.check("AN-WV-118 resolve → resolve, popover:false", l == serde_json::json!(["resolve", "popover:false"]), &l);
    }
    p.eval(r##"sr().querySelectorAll(".pin")[1].click(); await new Promise(r => setTimeout(r, 30));
Array.from(pop().querySelectorAll("button")).find(b => b.textContent === "Delete").click(); return true;"##);
    let ev = p.events(1);
    p.check("AN-IF-558 Delete → Dismiss", ev == vec![AnnotateEvent::Dismiss { id: "p2".into() }], format!("{ev:?}"));
    // AN-WV-010 i: a pin list without the open thread's pin closes it (popover:false, then missing).
    p.eval(r##"sr().querySelectorAll(".pin")[1].click(); await new Promise(r => setTimeout(r, 30)); return true;"##);
    if spying {
        log(p);
    }
    p.live.set_pins(pins().into_iter().filter(|x| x.id != "p2").collect());
    pause(60);
    let v = p.eval("return !!pop();");
    p.check("AN-WV-010i thread closes when its pin disappears", v == false, &v);
    if spying {
        let l = log(p);
        p.check("AN-WV-118 apply closing a thread → popover:false, missing", l == serde_json::json!(["popover:false", "missing"]), &l);
    }
    p.live.set_pins(pins());
    p.check("AN-WV-086 no Cancel from closing via apply", p.events(0).is_empty(), "");

    println!("P-9 annotate on (dark), reload with a new revision");
    p.live.set_annotate(true, true);
    if !p.load("probe", 2, PROBE_PAGE, rect, 800) {
        p.check("P-9 reload", false, "page did not reload within 10 s");
        return false;
    }
    pause(50);
    spin_until(|| p.live.missing_pins() == ["p3"], Duration::from_secs(3));
    let v = p.eval(r##"return { pins: sr().querySelectorAll(".pin").length, cursor: document.documentElement.style.cursor, fresh: typeof window.__log === "undefined" };"##);
    p.check("P-9 pins drawn again after reload (2 visible)", v["pins"] == 2 && v["fresh"] == true, &v);
    p.check("P-9 missing pins again [p3]", p.live.missing_pins() == ["p3"], format!("{:?}", p.live.missing_pins()));
    p.check("P-9 mode on after reload", v["cursor"] == "crosshair", &v);
    let v = p.eval(r##"clickSel("h1"); await new Promise(r => setTimeout(r, 30)); return { pop: !!pop(), bg: pop() && getComputedStyle(pop()).backgroundColor };"##);
    p.check("P-9 click h1 opens the popover (dark)", v["pop"] == true && v["bg"] == "rgba(23, 23, 23, 0.94)", &v);
    p.snapshot("annotate-popover-dark.png");
    p.eval("esc(); return true;");
    let ev = p.events(1);
    p.check("P-9 Esc closes (Cancel)", ev == vec![AnnotateEvent::Cancel], format!("{ev:?}"));

    extras(p);

    // AN-WV-010 f: switching off with a note popover open cancels; with pins as garbage nothing draws.
    p.eval(r##"clickSel("h1"); await new Promise(r => setTimeout(r, 30)); return true;"##);
    p.live.set_annotate(false, true);
    let ev = p.events(1);
    p.check("AN-WV-010f off with note popover → Cancel", ev == vec![AnnotateEvent::Cancel], format!("{ev:?}"));
    let v = p.eval(r##"const r = __protoAnnotate.apply(false, true, 0.5, "x"); return { r, pins: sr().querySelectorAll(".pin").length };"##);
    p.check("AN-WV-010 pins \"x\" → none drawn", v["r"] == serde_json::json!({"on": false, "pins": 0}) && v["pins"] == 0, &v);
    pause(60);
    p.check("AN-WV-010 pins \"x\" → empty missing list", p.live.missing_pins().is_empty(), format!("{:?}", p.live.missing_pins()));
    p.live.set_pins(Vec::new());
    true
}

/// Further checks on the reloaded probe page (mode on, dark, pins p1…p3).
fn extras(p: &Probe) {
    // AN-WV-033: a selection made before the click is captured with the target.
    p.eval(
        r##"const r = document.createRange(); r.selectNodeContents(document.querySelector(".lede"));
getSelection().removeAllRanges(); getSelection().addRange(r);
clickSel("#go"); await new Promise(r => setTimeout(r, 30)); typeEnter("zz-bench: selection"); getSelection().removeAllRanges(); return true;"##,
    );
    let ev = p.events(1);
    let sel = saves(&ev).first().and_then(|(_, t)| t.first()).and_then(|t| t.selected_text.clone());
    p.check("AN-WV-033 selected text captured", sel.as_deref() == Some("Bouldering for everyone."), format!("{sel:?}"));

    // AN-WV-021: hostile page CSS leaves the overlay alone.
    let v = p.eval(
        r##"const s = document.createElement("style"); s.id = "hostile";
s.textContent = "* { color: red !important; font-family: serif !important; box-sizing: content-box !important } div { padding: 50px !important }";
document.head.appendChild(s);
clickSel("h1"); await new Promise(r => setTimeout(r, 30));
const cs = getComputedStyle(pop().querySelector(".head"));
const out = { w: pop().offsetWidth, font: cs.fontFamily, color: getComputedStyle(pop()).color, pad: getComputedStyle(pop()).paddingTop, sizing: getComputedStyle(pop()).boxSizing };
s.remove(); esc(); return out;"##,
    );
    p.check(
        "AN-WV-021 page CSS does not reach the overlay",
        v["w"] == 300 && v["font"].as_str().is_some_and(|f| f.starts_with("-apple-system")) && v["color"] == "rgb(250, 250, 250)" && v["pad"] == "10px" && v["sizing"] == "border-box",
        &v,
    );
    p.events(1); // the Cancel from esc()

    // AN-WV-020: replacing <body> keeps the overlay; a removed host comes back on the next apply.
    let v = p.eval(
        r##"const kept = document.createElement("body"); kept.innerHTML = document.body.innerHTML; document.body.replaceWith(kept);
const after = !!ov();
ov().remove();
const gone = !ov();
__protoAnnotate.apply(true, true, 0.5, []);
return { after, gone, back: !!ov() && document.documentElement.lastElementChild === ov() };"##,
    );
    p.check("AN-WV-020 overlay survives body replacement and re-mounts", v["after"] == true && v["gone"] == true && v["back"] == true, &v);
}

// ---------------------------------------------------------------------------------------------
// Part 2: element description (AN-WV-066, AN-WV-068)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum C {
    /// Field contains the text.
    Has(&'static str, &'static str),
    /// Field contains one of the texts.
    Any(&'static str, &'static [&'static str]),
    Not(&'static str, &'static str),
    Eq(&'static str, &'static str),
    /// Field contains the needle exactly n times.
    Times(&'static str, &'static str, usize),
    /// Field contains the texts in this order.
    Order(&'static str, &'static [&'static str]),
    MaxLen(&'static str, usize),
    Empty(&'static str),
    /// Label is not empty.
    Named,
    Attr(&'static str, &'static str),
    NoAttr(&'static str),
    /// Short path finds exactly this element first.
    Sel,
    Before(&'static str),
    After(&'static str),
    NullSelection,
    /// Label is shortened recognisably.
    Clipped,
    /// Rect = bounding box (always checked); this asks for given top/height or width/height.
    TopHeight(f64, f64),
    Size(f64, f64),
}
use C::*;

struct Ex {
    id: &'static str,
    body: &'static str,
    /// Script run after load (e.g. attach a shadow root, scroll).
    setup: &'static str,
    targets: &'static [(&'static str, &'static [C])],
}

const LONG_INTRO: &str = "Our gyms are open seven days a week from early morning until late at night, with routes for every level, friendly staff, rental shoes, chalk and a cafe; members get free classes, guest passes and discounts in our shop, and kids climb free on Sundays with a paying adult and so much more to enjoy here.";
const LONG_H2: &str = "Everything you need to start climbing today, from shoes and chalk to classes and coaching for absolute beginners and pros";

fn examples() -> Vec<Ex> {
    vec![
        Ex {
            id: "E-01",
            body: r##"<div class="toolbar"><button id="save-btn" class="btn btn-primary" aria-label="Save draft"><svg width="16" height="16" viewBox="0 0 16 16"><path d="M2 2h12v12H2z"/></svg></button></div>"##,
            setup: "",
            targets: &[
                ("#save-btn", &[Has("label", "button"), Has("label", "Save draft"), Sel, Has("path", "save-btn"), Has("classes", "btn"), Has("classes", "btn-primary"), Has("a11y", "Save draft"), Has("a11y", "focusable"), Attr("id", "save-btn"), Attr("aria-label", "Save draft"), Has("styles", "background-color"), Has("styles", "padding"), Has("styles", "font-size")]),
                ("#save-btn svg", &[Named, Any("label", &["button", "Save draft"]), Has("styles", "width: 16px"), Has("styles", "height: 16px"), Sel]),
                ("#save-btn path", &[Named, Any("label", &["button", "Save draft"]), Sel]),
            ],
        },
        Ex {
            id: "E-02",
            body: r##"<div class="toolbar"><button class="tool"><svg width="12" height="12"><circle cx="6" cy="6" r="5"/></svg> Save</button><span class="hint">Ctrl+S</span></div>"##,
            setup: "",
            targets: &[
                (".tool svg", &[Has("label", "Save"), Any("label", &["icon", "graphic", "svg"]), Sel]),
                (".tool", &[Has("label", "button"), Has("label", "\"Save\""), After("Ctrl+S"), Has("nearbyElements", "span"), Has("nearbyElements", "hint"), Sel]),
            ],
        },
        Ex {
            id: "E-03",
            body: r##"<nav class="site-nav"><a href="/docs/getting-started">Getting started</a><a href="/pricing" class="active">Pricing</a><a href="https://example.com/help"><img src="help.svg" alt=""></a></nav>"##,
            setup: "",
            targets: &[
                ("nav a:nth-child(2)", &[Has("label", "link"), Has("label", "\"Pricing\""), Sel, Attr("href", "/pricing"), Before("Getting started"), Has("nearbyElements", "Getting started"), Has("a11y", "focusable")]),
                ("nav a:nth-child(3)", &[Has("label", "link"), Has("label", "example.com/help"), Sel]),
            ],
        },
        Ex {
            id: "E-04",
            body: r##"<form class="signup"><label for="mail">E-mail</label><input id="mail" type="email" name="email" placeholder="you@example.com"><input type="number" class="qty"><textarea name="msg"></textarea></form>"##,
            setup: "",
            targets: &[
                ("#mail", &[Has("label", "input"), Has("label", "you@example.com"), Has("path", "mail"), Attr("id", "mail"), Attr("type", "email"), Attr("name", "email"), Attr("placeholder", "you@example.com"), Before("E-mail"), Has("a11y", "focusable"), Has("styles", "font-size"), Sel]),
                (".qty", &[Has("label", "input"), Has("label", "number"), Sel, Attr("type", "number")]),
                ("textarea", &[Named, Any("label", &["textarea", "msg"]), Has("a11y", "focusable"), Sel]),
                ("label", &[Has("label", "E-mail"), Sel]),
            ],
        },
        Ex {
            id: "E-05",
            body: r##"<ul class="menu"><li>Home</li><li>Products<ul class="submenu"><li>Shoes</li><li>Ropes</li><li>Chalk</li></ul></li><li>About</li><li>Blog</li><li>Contact</li><li>Jobs</li><li>Press</li><li>Legal</li><li>Privacy</li><li>Imprint</li></ul>"##,
            setup: "",
            targets: &[
                (".submenu li:nth-child(2)", &[Has("label", "Ropes"), Any("label", &["list item", "li"]), Sel, Before("Shoes"), After("Chalk"), Order("fullPath", &["body", "menu", "li", "submenu", "li"])]),
                (".menu > li:nth-child(5)", &[Sel, Has("nearbyElements", "10"), Has("nearbyElements", "menu")]),
                (".menu > li:nth-child(2)", &[MaxLen("label", 80), Sel]),
            ],
        },
        Ex {
            id: "E-06",
            body: r##"<div class="chart-card"><svg class="chart" width="200" height="100" viewBox="0 0 200 100"><g class="bars"><rect x="10" y="20" width="30" height="80" fill="#3b82f6"/><circle cx="150" cy="50" r="20"/></g></svg></div>"##,
            setup: "",
            targets: &[
                (".chart rect", &[Named, Has("label", "chart"), Sel]),
                (".chart", &[Named, Has("styles", "width: 200px"), Has("styles", "height: 100px"), Sel]),
                (".chart circle", &[Sel]),
            ],
        },
        Ex {
            id: "E-07",
            body: r##"<x-card id="card"></x-card><iframe id="preview" title="Preview" src="about:blank" width="300" height="150"></iframe>"##,
            setup: r##"document.getElementById("card").attachShadow({ mode: "open" }).innerHTML = '<button class="inner">Buy</button>';"##,
            targets: &[
                ("x-card", &[Has("label", "x-card"), Has("path", "card"), Sel]),
                ("iframe", &[Has("label", "iframe"), Has("path", "preview"), Attr("id", "preview"), Size(304.0, 154.0), Sel]),
            ],
        },
        Ex {
            id: "E-08",
            body: r##"<header class="topbar" style="position:fixed;top:0;left:0;right:0;height:48px;background:#fff"><span class="brand">Acme</span></header><div style="height:3000px"></div>"##,
            setup: "window.scrollTo(0, 500); await new Promise(r => requestAnimationFrame(() => r()));",
            targets: &[
                (".topbar", &[TopHeight(0.0, 48.0), Any("label", &["header", "topbar"]), Sel, Has("styles", "background-color: rgb(255, 255, 255)")]),
                (".brand", &[Has("label", "\"Acme\""), Sel]),
            ],
        },
        Ex {
            id: "E-09",
            body: r##"<div class="tabs" role="tablist"><div role="tab" tabindex="0" aria-selected="true" aria-controls="p1" class="tab tab--active">Overview</div><div role="tab" tabindex="-1" class="tab">Details</div></div>"##,
            setup: "",
            targets: &[
                ("[aria-controls=\"p1\"]", &[Has("label", "tab"), Has("a11y", "role=\"tab\""), Has("a11y", "tabindex=0"), Has("a11y", "focusable"), Attr("role", "tab"), Attr("aria-selected", "true"), Attr("aria-controls", "p1"), Has("classes", "tab"), Has("classes", "tab--active"), Sel]),
                (".tabs", &[Has("label", "tablist"), Has("a11y", "tablist"), Not("a11y", "focusable"), Sel]),
                (".tab:not(.tab--active)", &[Sel, Has("label", "Details")]),
            ],
        },
        Ex {
            id: "E-10",
            body: "",
            setup: "",
            targets: &[
                (".intro", &[MaxLen("label", 80), Has("label", "Our gyms are open"), Clipped, MaxLen("nearbyText", 300), Sel]),
                ("h2", &[MaxLen("label", 80), Has("label", "h2"), Has("label", "Everything you need"), Sel]),
                ("p:not(.intro)", &[MaxLen("label", 80), Sel]),
            ],
        },
        Ex {
            id: "E-11",
            body: r##"<div class="spacer" style="height:24px"></div><img src="/img/hero.jpg" width="320" height="180"><div></div>"##,
            setup: "",
            targets: &[
                (".spacer", &[Named, Has("label", "spacer"), Has("styles", "display: block"), Eq("classes", "spacer"), Empty("nearbyText"), Sel]),
                ("img", &[Any("label", &["image", "img"]), Attr("src", "/img/hero.jpg"), Has("styles", "width: 320px"), Has("styles", "height: 180px"), Sel]),
                ("div:last-of-type", &[Named, Has("styles", "display: block"), Not("styles", "0px"), Not("styles", "none"), Not("styles", "normal"), Not("styles", "auto"), Not("styles", "rgba(0, 0, 0, 0)"), Sel]),
            ],
        },
        Ex {
            id: "E-12",
            body: r##"<button class="Button_primary__x7aB2 cta big cta">Sign up</button><div class="Card_root_x9Y2k Card_root_ZZ123">Plan</div>"##,
            setup: "",
            targets: &[
                ("button", &[Has("label", "\"Sign up\""), Sel, Times("classes", "cta", 1), Times("classes", "big", 1)]),
                ("div", &[Times("classes", "Card_root", 1), Not("classes", "x9Y2k"), Not("classes", "ZZ123"), Sel]),
            ],
        },
        Ex {
            id: "E-13",
            body: r##"<section data-element="Pricing table (annual)" data-component="PricingTable" data-testid="pricing"><h3>Annual</h3><p>Save 20 % with yearly billing.</p></section>"##,
            setup: "",
            targets: &[("section", &[Eq("label", "Pricing table (annual)"), Has("path", "pricing"), Attr("data-component", "PricingTable"), Attr("data-testid", "pricing"), Attr("data-element", "Pricing table (annual)"), Sel])],
        },
        Ex {
            id: "E-14",
            body: r##"<div id="a:b" data-testid='say "hi" \ now'>x</div><div id="1st">y</div>"##,
            setup: "",
            targets: &[("[id=\"a:b\"]", &[Sel, Attr("id", "a:b"), Attr("data-testid", "say \"hi\" \\ now")]), ("[id=\"1st\"]", &[Sel, Attr("id", "1st")])],
        },
        Ex {
            id: "E-15",
            body: r##"<div class="toolbar"><svg width="10" height="10"><path d="M0 0h10v10z"/></svg></div>"##,
            setup: "",
            targets: &[(".toolbar path", &[Has("label", "toolbar"), Sel])],
        },
        Ex {
            id: "AN-WV-066",
            body: "",
            setup: "",
            targets: &[
                (
                    "#go",
                    &[
                        Has("label", "button"),
                        Has("label", "\"Start trial\""),
                        Sel,
                        Has("path", "go"),
                        Has("path", "start"),
                        Order("fullPath", &["body", "main", "hero", "button", "go"]),
                        Eq("classes", "cta"),
                        Before("Bouldering for everyone."),
                        Order("nearbyText", &["Bouldering for everyone.", "Start trial", "Prices"]),
                        After("Prices"),
                        Has("nearbyElements", "h1"),
                        Has("nearbyElements", "p.lede"),
                        Has("nearbyElements", "a \"Prices\""),
                        Not("nearbyElements", "children"),
                        NullSelection,
                        Has("styles", "background-color: rgb(20, 71, 230)"),
                        Has("styles", "color: rgb(255, 255, 255)"),
                        Has("styles", "padding: 10px 18px"),
                        Has("styles", "border-radius: 10px"),
                        Has("styles", "font-size: 15px"),
                        Eq("a11y", "focusable"),
                        Attr("data-testid", "start"),
                        Attr("id", "go"),
                        NoAttr("class"),
                    ],
                ),
                ("h1", &[Has("label", "h1"), Has("label", "\"Climb higher\""), Sel]),
                ("a", &[Has("label", "link"), Has("label", "\"Prices\""), Attr("href", "/prices"), Sel]),
                ("p.lede", &[Has("label", "Bouldering for everyone."), Sel]),
                ("main", &[Any("label", &["hero", "main"]), Sel]),
                ("body", &[Named, Eq("path", "body"), Eq("fullPath", "body"), Sel]),
                ("html", &[Named, Sel]),
            ],
        },
    ]
}

fn page_for(ex: &Ex) -> String {
    match ex.id {
        "AN-WV-066" => PROBE_PAGE.to_string(),
        "E-10" => format!(
            r##"<!doctype html><html><head><meta charset="utf-8"><style>body {{ margin: 0; font: 16px -apple-system, system-ui, sans-serif; }}</style></head><body><p class="intro">{LONG_INTRO}</p><h2>{LONG_H2}</h2><p>{}</p></body></html>"##,
            "Climbing is fun for the whole family, yes!"
        ),
        _ => format!(r##"<!doctype html><html><head><meta charset="utf-8"><style>body {{ margin: 0; font: 16px -apple-system, system-ui, sans-serif; }}</style></head><body>{}</body></html>"##, ex.body),
    }
}

/// Facts of the target plus the in-page checks that need the DOM.
fn describe_js(sel: &str) -> String {
    format!(
        r##"const sel = {sel:?};
const el = document.querySelector(sel);
if (!el) return {{ missing: true }};
const t0 = performance.now();
const f = __protoAnnotate.describe(sel);
const ms = performance.now() - t0;
let first = false; try {{ first = document.querySelector(f.path) === el; }} catch (e) {{ first = "throws: " + e.message; }}
const b = el.getBoundingClientRect();
return {{ f, first, ms, box: [b.left, b.top, b.width, b.height] }};"##
    )
}

fn field<'a>(f: &'a Value, name: &str) -> &'a str {
    f[name].as_str().unwrap_or("")
}

fn run_check(c: C, r: &Value) -> Result<(), String> {
    let f = &r["f"];
    let got = |name: &str| format!("{name} = {:?}", field(f, name));
    let ok = |b: bool, why: String| if b { Ok(()) } else { Err(why) };
    match c {
        Has(k, n) => ok(field(f, k).contains(n), format!("{} should contain {n:?}", got(k))),
        Any(k, ns) => ok(ns.iter().any(|n| field(f, k).contains(n)), format!("{} should contain one of {ns:?}", got(k))),
        Not(k, n) => ok(!field(f, k).contains(n), format!("{} should not contain {n:?}", got(k))),
        Eq(k, v) => ok(field(f, k) == v, format!("{} should be {v:?}", got(k))),
        Times(k, n, times) => ok(field(f, k).matches(n).count() == times, format!("{} should contain {n:?} {times}×", got(k))),
        Order(k, ns) => {
            let s = field(f, k);
            let mut at = 0;
            for n in ns {
                match s[at..].find(n) {
                    Some(i) => at += i + n.len(),
                    None => return Err(format!("{} should contain {ns:?} in order", got(k))),
                }
            }
            Ok(())
        }
        MaxLen(k, n) => ok(field(f, k).chars().count() <= n, format!("{} longer than {n}", got(k))),
        Empty(k) => ok(field(f, k).is_empty(), format!("{} should be empty", got(k))),
        Named => ok(!field(f, "label").is_empty(), "label empty".into()),
        Attr(k, v) => ok(f["attrs"][k].as_str() == Some(v), format!("attrs[{k}] = {:?}, want {v:?}", f["attrs"][k])),
        NoAttr(k) => ok(f["attrs"].get(k).is_none(), format!("attrs has {k}")),
        Sel => ok(r["first"] == true, format!("path {:?} → first match is the target: {}", field(f, "path"), r["first"])),
        Before(n) => ok(field(f, "nearbyText").contains(&format!("[before: \"{n}")), format!("{} should have before {n:?}", got("nearbyText"))),
        After(n) => ok(field(f, "nearbyText").contains(&format!("[after: \"{n}")), format!("{} should have after {n:?}", got("nearbyText"))),
        NullSelection => ok(f["selectedText"].is_null(), format!("selectedText = {}", f["selectedText"])),
        Clipped => ok(field(f, "label").contains('…') || field(f, "label").contains("..."), format!("{} not marked as shortened", got("label"))),
        TopHeight(t, h) => ok(f["rect"][1].as_f64() == Some(t) && f["rect"][3].as_f64() == Some(h), format!("rect = {}", f["rect"])),
        Size(w, h) => ok(f["rect"][2].as_f64() == Some(w) && f["rect"][3].as_f64() == Some(h), format!("rect = {}", f["rect"])),
    }
}

/// AN-WV-067 and the general limits from AN-WV-054/055/063, for every target.
fn general_checks(r: &Value) -> Result<(), String> {
    let f = &r["f"];
    for k in ["label", "path", "fullPath", "classes", "nearbyText", "nearbyElements", "styles", "a11y"] {
        let Some(s) = f[k].as_str() else { return Err(format!("{k} is not a string: {}", f[k])) };
        if s.contains('\n') || s != s.trim() || s.contains("  ") {
            return Err(format!("{k} not normalised: {s:?}"));
        }
    }
    let label = field(f, "label");
    let own = f["attrs"]["data-element"].as_str();
    if label.is_empty() || (own != Some(label) && label.chars().count() > 80) {
        return Err(format!("label empty or over 80: {label:?}"));
    }
    for k in ["path", "fullPath"] {
        if field(f, k).contains(" | ") {
            return Err(format!("{k} contains \" | \""));
        }
    }
    for k in ["nearbyText", "nearbyElements"] {
        if field(f, k).chars().count() > 300 {
            return Err(format!("{k} over 300"));
        }
    }
    if field(f, "styles").split("; ").filter(|s| !s.is_empty()).count() > 8 {
        return Err("more than 8 styles".into());
    }
    if !(f["selectedText"].is_null() || f["selectedText"].is_string()) {
        return Err("selectedText neither null nor text".into());
    }
    let attrs = f["attrs"].as_object().ok_or("attrs not an object")?;
    if attrs.len() > 16 || attrs.contains_key("class") || attrs.contains_key("style") || attrs.values().any(|v| v.as_str().is_none_or(|s| s.chars().count() > 500)) {
        return Err(format!("attrs break the limits: {attrs:?}"));
    }
    let rect: Vec<f64> = f["rect"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    let bx: Vec<f64> = r["box"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    if rect.len() != 4 || rect.iter().zip(&bx).any(|(a, b)| (a - b).abs() > 0.01) {
        return Err(format!("rect {:?} != bounding box {:?}", f["rect"], r["box"]));
    }
    Ok(())
}

fn descriptions(p: &Probe) {
    p.live.set_annotate(false, false);
    p.live.set_pins(Vec::new());
    let rect = [0.0, 0.0, 900.0, 600.0];
    let mut slowest: f64 = 0.0;
    for (n, ex) in examples().iter().enumerate() {
        println!("{} description", ex.id);
        let html = page_for(ex);
        if !p.load(&format!("ex-{}", ex.id), n as u64 + 10, &html, rect, 900) {
            p.check(&format!("{} load", ex.id), false, "page did not load");
            continue;
        }
        if !ex.setup.is_empty() {
            p.eval(&format!("{}\nreturn true;", ex.setup));
        }
        for (sel, checks) in ex.targets {
            let r = p.eval(&describe_js(sel));
            if r["missing"] == true || !r["f"].is_object() {
                p.check(&format!("{} {sel}", ex.id), false, format!("no facts: {r}"));
                continue;
            }
            slowest = slowest.max(r["ms"].as_f64().unwrap_or(0.0));
            let mut errors: Vec<String> = checks.iter().filter_map(|c| run_check(*c, &r).err()).collect();
            if let Err(e) = general_checks(&r) {
                errors.push(e);
            }
            p.check(&format!("{} {sel}", ex.id), errors.is_empty(), errors.join("; "));
            println!("       label {:?} · path {:?}", field(&r["f"], "label"), field(&r["f"], "path"));
        }
        match ex.id {
            "E-07" => shadow_and_iframe(p),
            "E-08" => {
                let r = p.eval(&describe_js(".topbar"));
                p.check("E-08 scrolled by 500 px", p.eval("return scrollY;") == 500, "");
                println!("       topbar rect {}", r["f"]["rect"]);
            }
            _ => {}
        }
    }
    p.check("AN-WV-067 description under 16 ms per element", slowest < 16.0, format!("slowest {slowest:.2} ms"));
    combined(p);
}

/// AN-WV-055: every element of one page holding all snippets is found again by its short path
/// (look-alikes across examples: two `.toolbar`s, several `svg`, `path`, `div`, `li`).
fn combined(p: &Probe) {
    println!("all snippets on one page");
    let body: String = examples().iter().map(|e| e.body).collect();
    let html = format!(r##"<!doctype html><html><head><meta charset="utf-8"></head><body>{body}<p class="intro">{LONG_INTRO}</p><h2>{LONG_H2}</h2><p>short</p></body></html>"##);
    if !p.load("combined", 1, &html, [0.0, 0.0, 900.0, 600.0], 900) {
        p.check("combined load", false, "");
        return;
    }
    let v = p.eval(
        r##"const bad = []; let n = 0;
for (const el of document.body.querySelectorAll("*")) {
  if (el.localName === "script") continue;
  n++;
  const f = __protoAnnotate.describeElement(el);
  let first = false; try { first = document.querySelector(f.path) === el; } catch (e) {}
  if (!first || f.path.includes(" | ") || !f.label) bad.push(f.path + " (" + f.label + ")");
  if (!window.__longest || f.path.length > window.__longest.length) window.__longest = f.path;
}
return { n, bad, longest: window.__longest };"##,
    );
    println!("       longest path {}", v["longest"]);
    p.check("AN-WV-055 every element on the combined page found by its path", v["bad"].as_array().is_some_and(|a| a.is_empty()) && v["n"].as_u64().unwrap_or(0) > 40, &v);
}

/// E-07 by click: the shadow host and the iframe are what gets picked; inner shadow facts complete.
fn shadow_and_iframe(p: &Probe) {
    let v = p.eval(
        r##"const f = __protoAnnotate.describeElement(document.getElementById("card").shadowRoot.querySelector(".inner"));
return { f, keys: f && Object.keys(f).length, iframeEntry: typeof document.getElementById("preview").contentWindow.__protoAnnotate };"##,
    );
    p.check("E-07 facts inside a shadow root complete, no error", v["keys"] == 11 && v["f"]["label"].as_str().is_some_and(|s| s.contains("Buy")), &v["f"]);
    p.check("AN-WV-001 no picker inside an iframe", v["iframeEntry"] == "undefined", &v["iframeEntry"]);
    p.live.set_annotate(true, false);
    pause(60);
    for (sel, comment) in [("#card", "zz-bench: card"), ("#preview", "zz-bench: frame")] {
        p.eval(&format!(r##"clickSel({sel:?}); await new Promise(r => setTimeout(r, 30)); typeEnter({comment:?}); return true;"##));
        let ev = p.events(1);
        let s = saves(&ev);
        let Some(t) = s.first().and_then(|(_, t)| t.first()) else {
            p.check(&format!("E-07 click {sel}"), false, format!("{ev:?}"));
            continue;
        };
        let first = p.eval(&format!("return document.querySelector({:?}) === document.querySelector({sel:?});", t.element_path));
        if sel == "#card" {
            p.check("E-07 click on \"Buy\" picks the host x-card", t.element.contains("x-card") && t.element_path.contains("card") && first == true, format!("{t:?}"));
        } else {
            // 300 × 150 content plus the default 2 px iframe border: the border box.
            let rect_ok = t.rect[2] == 304.0 && t.rect[3] == 154.0 && t.window_rect[2] == 304.0;
            p.check("E-07 click on the iframe picks the iframe", t.element.contains("iframe") && t.element_path.contains("preview") && t.attributes.get("id").map(String::as_str) == Some("preview") && rect_ok && first == true, format!("{t:?}"));
        }
    }
    p.live.set_annotate(false, false);
    pause(30);
}

/// AN-WV-004: the script alone (no apply) leaves a page untouched. Runs in a same-origin iframe of
/// a page that has the script, so the frame gets it only by our own evaluation.
fn idle(p: &Probe) {
    println!("AN-WV-004 idle page");
    let html = r##"<!doctype html><html><body><iframe id="f" srcdoc="<button id=b>B</button><script>window.n={click:0,dbl:0,ctx:0,down:0};for(const t of ['click','dblclick','contextmenu','mousedown'])document.addEventListener(t,()=>n[{click:'click',dblclick:'dbl',contextmenu:'ctx',mousedown:'down'}[t]]++)</script>" width="300" height="100"></iframe></body></html>"##;
    if !p.load("idle", 1, html, [0.0, 0.0, 900.0, 600.0], 900) {
        p.check("AN-WV-004 load", false, "");
        return;
    }
    pause(100);
    // AN-WV-005: the apply call in a frame without the script yields nothing and throws nothing.
    let v = p.eval(
        r##"const w = document.getElementById("f").contentWindow;
const a = w["__protoAnnotate"]; return a && typeof a.apply === "function" ? a.apply(true, false, 1, []) : null;"##,
    );
    p.check("AN-WV-005 apply without the script → null, no error", v.is_null(), &v);
    let v = p.eval(&format!(
        r##"const w = document.getElementById("f").contentWindow;
w.eval({SCRIPT:?});
const b = w.document.getElementById("b");
for (const t of ["mousedown", "click", "dblclick", "contextmenu"]) b.dispatchEvent(new w.MouseEvent(t, {{ bubbles: true, cancelable: true, clientX: 5, clientY: 5 }}));
const kids = Array.from(w.document.documentElement.children).map(c => c.localName);
return {{ n: w.n, kids, entry: typeof w.__protoAnnotate }};"##
    ));
    p.check("AN-WV-004 idle: clicks, double clicks, context menu reach the page", v["n"] == serde_json::json!({"click": 1, "dbl": 1, "ctx": 1, "down": 1}), &v);
    p.check("AN-WV-004 idle: no overlay element in the DOM", v["kids"] == serde_json::json!(["head", "body"]) && v["entry"] == "object", &v);
    // AN-WV-101: 100 scroll events in one frame → one redraw. A never-shown window runs no animation
    // frames, so the iframe's frame source is stubbed before the script captures it.
    let v = p.eval(&format!(
        r##"const f = document.getElementById("f");
f.srcdoc = "<p id=a>A</p><div style='height:2000px'></div>";
await new Promise(r => f.addEventListener("load", r, {{ once: true }}));
const w = f.contentWindow;
const frames = [];
w.requestAnimationFrame = (cb) => frames.push(cb);
w.eval({SCRIPT:?});
w.__protoAnnotate.apply(false, false, 1, [{{ id: "a", number: 1, path: "#a", comment: "", status: "pending", thread: [] }}]);
const host = Array.from(w.document.documentElement.children).find(c => c.shadowRoot);
const pinsBox = host.shadowRoot.querySelectorAll(".layer > div")[3];
const first = pinsBox.firstChild;
for (let i = 0; i < 100; i++) w.dispatchEvent(new w.Event("scroll"));
const queued = frames.length;
frames.splice(0).forEach(cb => cb(0));
return {{ queued, redrawn: pinsBox.firstChild !== first && pinsBox.children.length === 1 }};"##
    ));
    p.check("AN-WV-101 100 scroll events → one frame, one redraw", v["queued"] == 1 && v["redrawn"] == true, &v);
    let v = p.eval(r##"return typeof window.__protoAnnotate;"##);
    p.check("AN-WV-001 main frame has the entry", v == "object", &v);
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let _ = std::fs::create_dir_all(&out);
    let mtm = MainThreadMarker::new().expect("main thread");
    NSApplication::sharedApplication(mtm).setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let frame = NSRect::new(NSPoint::new(-30000.0, -30000.0), NSSize::new(900.0, 600.0));
    let window = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(NSWindow::alloc(mtm), frame, NSWindowStyleMask::Borderless, NSBackingStoreType::Buffered, false) };
    unsafe { window.setReleasedWhenClosed(false) };
    let host = window.contentView().expect("content view");
    // The wake callback does nothing here: the probe polls by itself.
    let live = LiveView::attach_view(host, Arc::new(|| {}));
    let probe = Probe { live, out, failures: RefCell::new(Vec::new()), passed: RefCell::new(0), events: RefCell::new(Vec::new()) };

    let t = Instant::now();
    let loaded = scenarios(&probe);
    if loaded {
        descriptions(&probe);
        idle(&probe);
    }
    probe.live.hide();
    let failures = probe.failures.borrow();
    println!("\n{} checks passed, {} failed in {:.1} s", probe.passed.borrow(), failures.len(), t.elapsed().as_secs_f64());
    if failures.is_empty() {
        println!("ok");
    } else {
        for f in failures.iter() {
            println!("FAIL {f}");
        }
        std::process::exit(1);
    }
}
