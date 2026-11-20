//! Prompt sections built from request context: guidelines (`guidelines.ts`), @-mentioned pod data
//! (`pod-catalog.ts` describePodContext), #-mentioned components (`components.ts`), plus `htmlToText`.

use std::sync::LazyLock;

use regex::Regex;

use crate::types::{ComponentRef, Guidelines, PodAccess, PodContextEntry};

/// Most #-mentioned components passed on per request, and the longest html kept for each.
pub const MAX_COMPONENTS: usize = 12;
const MAX_COMPONENT_HTML: usize = 120_000;
/// Longest guidelines text (chars) the server should pass in (`MAX_TEXT` in guidelines.ts).
pub const MAX_GUIDELINES_TEXT: usize = 12_000;

/// First `n` chars of `s` (JS `slice(0, n)`; counted in chars, not UTF-16 units).
pub fn truncate_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Keep only well-formed refs with html, at most MAX_COMPONENTS, each title/html capped. None when empty.
pub fn clean_components(list: Option<&[ComponentRef]>) -> Option<Vec<ComponentRef>> {
    let out: Vec<ComponentRef> = list
        .unwrap_or_default()
        .iter()
        .filter(|c| !c.html.trim().is_empty())
        .take(MAX_COMPONENTS)
        .map(|c| ComponentRef {
            id: c.id.clone(),
            title: truncate_chars(&c.title, 200).to_string(),
            html: truncate_chars(&c.html, MAX_COMPONENT_HTML).to_string(),
        })
        .collect();
    (!out.is_empty()).then_some(out)
}

/// Prompt section for #-mentioned components: reuse them exactly instead of redesigning them.
pub fn describe_components(list: Option<&[ComponentRef]>) -> Option<String> {
    let list = list.filter(|l| !l.is_empty())?;
    let blocks: Vec<String> = list
        .iter()
        .enumerate()
        .map(|(i, c)| format!("Component {} (\"{}\", card {}):\n<component_html>\n{}\n</component_html>", i + 1, c.title, c.id, c.html))
        .collect();
    Some(format!(
        "Reference components the designer #-mentioned. Wherever the design needs one of these, reuse it EXACTLY as given \
(same markup, styles, tokens, icons, copy and sizes); only adapt what placement requires (width, outer spacing). \
Don't redesign, restyle or re-theme them, and don't show them on their own as a reference sheet:\n\n{}",
        blocks.join("\n\n")
    ))
}

fn access_text(a: PodAccess) -> &'static str {
    match a {
        PodAccess::Live => "publicly readable (live)",
        PodAccess::Locked => "locked (401), values from documented captures",
        PodAccess::Ref => "known by reference only",
        PodAccess::Cache => "app cache",
    }
}

/// Turn the @-mentioned pod data into a prompt section for the design agent.
pub fn describe_pod_context(ctx: Option<&[PodContextEntry]>) -> Option<String> {
    let ctx = ctx.filter(|c| !c.is_empty())?;
    let blocks: Vec<String> = ctx
        .iter()
        .map(|e| {
            let mut lines = vec![format!(
                "- {} [{}] · {} · {}{}",
                e.label,
                e.kind,
                e.source,
                access_text(e.access),
                e.stand.as_deref().filter(|s| !s.is_empty()).map(|s| format!(" · {s}")).unwrap_or_default()
            )];
            for f in &e.fields {
                let tail = match (f.v.as_deref().filter(|v| !v.is_empty()), f.kind.as_deref().filter(|t| !t.is_empty())) {
                    (Some(v), _) => format!(": {v}"),
                    (None, Some(t)) => format!(" ({t})"),
                    (None, None) => String::new(),
                };
                lines.push(format!("    {}{}", f.p, tail));
            }
            for r in e.relations.as_deref().unwrap_or_default() {
                lines.push(format!("    {} → {}", r.p, r.target));
            }
            if let Some(note) = e.note.as_deref().filter(|n| !n.is_empty()) {
                lines.push(format!("    note: {note}"));
            }
            lines.join("\n")
        })
        .collect();
    Some(format!(
        "Pod data mentioned by the designer (Solid pod, read-only). Use these real values and shapes; don't invent \
values for fields that aren't listed, and show gaps honestly (locked, unread):\n{}",
        blocks.join("\n")
    ))
}

/// The guidelines as a hard constraint for the LLM's system prompt ("" when there are none).
pub fn describe_guidelines(g: Option<&Guidelines>) -> String {
    let Some(g) = g else { return String::new() };
    format!(
        "Design guidelines for this space (\"{}\", set in {}). They frame every design made here: follow them strictly; they override the base design and your own taste wherever they conflict (only an explicit instruction from the designer may override them):\n<design_guidelines>\n{}\n</design_guidelines>",
        g.title, g.from_space, g.text
    )
}

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).expect("valid regex"));
    };
}

re!(RE_HEAD, r"(?i)<head\b[\s\S]*?</head>");
re!(RE_STYLE, r"(?i)<style\b[\s\S]*?</style>");
re!(RE_SCRIPT, r"(?i)<script\b[\s\S]*?</script>");
re!(RE_NOSCRIPT, r"(?i)<noscript\b[\s\S]*?</noscript>");
re!(RE_SVG, r"(?i)<svg\b[\s\S]*?</svg>");
re!(RE_COMMENT, r"<!--[\s\S]*?-->");
re!(RE_BR, r"(?i)<(br|hr)\b[^>]*>");
re!(
    RE_BLOCK,
    r"(?i)</?(p|div|section|article|header|footer|main|aside|nav|h[1-6]|li|ul|ol|dl|dt|dd|tr|table|blockquote|pre|figure|figcaption)\b[^>]*>"
);
re!(RE_CELL, r"(?i)<(td|th)\b[^>]*>");
re!(RE_TAG, r"<[^>]+>");
re!(RE_DEC, r"&#(\d+);");
re!(RE_HEX, r"(?i)&#x([0-9a-f]+);");
re!(RE_SPACES, r"[ \t\x0C\x0B\r]+");
re!(RE_NL_PAD, r" *\n *");
re!(RE_NL_RUN, r"\n{2,}");

/// Readable text of an HTML document: no head/style/script, block elements on their own lines, entities decoded.
pub fn html_to_text(html: &str) -> String {
    let mut s = html.to_string();
    for re in [&*RE_HEAD, &*RE_STYLE, &*RE_SCRIPT, &*RE_NOSCRIPT, &*RE_SVG, &*RE_COMMENT] {
        s = re.replace_all(&s, " ").into_owned();
    }
    s = RE_BR.replace_all(&s, "\n").into_owned();
    s = RE_BLOCK.replace_all(&s, "\n").into_owned();
    s = RE_CELL.replace_all(&s, " | ").into_owned();
    s = RE_TAG.replace_all(&s, " ").into_owned();
    s = s.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"");
    s = s.replace("&#39;", "'").replace("&apos;", "'");
    let code_point = |n: Option<u32>, orig: &str| n.and_then(char::from_u32).map(String::from).unwrap_or_else(|| orig.to_string());
    s = RE_DEC.replace_all(&s, |c: &regex::Captures| code_point(c[1].parse().ok(), &c[0])).into_owned();
    s = RE_HEX.replace_all(&s, |c: &regex::Captures| code_point(u32::from_str_radix(&c[1], 16).ok(), &c[0])).into_owned();
    s = s.replace("&amp;", "&");
    s = RE_SPACES.replace_all(&s, " ").into_owned();
    s = RE_NL_PAD.replace_all(&s, "\n").into_owned();
    s = RE_NL_RUN.replace_all(&s, "\n").into_owned();
    s.trim().to_string()
}

re!(RE_C_SCRIPT, r"(?i)<script[\s\S]*?</script>");
re!(RE_C_SVG, r"(?i)<svg[\s\S]*?</svg>");
re!(RE_C_DATA, r#"data:[^"')\s]{80,}"#);
re!(RE_C_WS, r"\s+");

/// Strip what a model doesn't need (scripts, svgs, data URIs, whitespace) and cap the length (decisions route).
pub fn condense(html: &str, max: usize) -> String {
    let mut s = RE_C_SCRIPT.replace_all(html, "<script>…</script>").into_owned();
    s = RE_C_SVG.replace_all(&s, "<svg>…</svg>").into_owned();
    s = RE_C_DATA.replace_all(&s, "data:…").into_owned();
    s = RE_COMMENT.replace_all(&s, "").into_owned();
    s = RE_C_WS.replace_all(&s, " ").into_owned();
    let s = s.trim();
    if s.chars().count() > max { format!("{} …[truncated]", truncate_chars(s, max)) } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{PodField, PodRelation};

    #[test]
    fn html_to_text_matches_reference_behaviour() {
        let html = "<html><head><title>x</title><style>a{}</style></head><body><nav>Home</nav>\
<div>Hello&nbsp;<b>world</b> &amp; &lt;you&gt; &#39;q&#39; &#x41;&#66;</div><!-- c --><table><tr><td>a</td><td>b</td></tr></table>\
<script>alert(1)</script><svg><path/></svg><p>  end  </p></body></html>";
        assert_eq!(html_to_text(html), "Home\nHello world & <you> 'q' AB\n| a | b\nend");
    }

    #[test]
    fn components_are_cleaned_and_described() {
        let list: Vec<ComponentRef> = (0..15)
            .map(|i| ComponentRef { id: format!("c{i}"), title: "T".repeat(300), html: if i == 0 { " ".into() } else { "<b/>".into() } })
            .collect();
        let cleaned = clean_components(Some(&list)).unwrap();
        assert_eq!(cleaned.len(), MAX_COMPONENTS);
        assert_eq!(cleaned[0].id, "c1");
        assert_eq!(cleaned[0].title.chars().count(), 200);
        assert!(clean_components(Some(&[])).is_none());
        let text = describe_components(Some(&cleaned[..1])).unwrap();
        assert!(text.starts_with("Reference components the designer #-mentioned."));
        assert!(text.ends_with(&format!("Component 1 (\"{}\", card c1):\n<component_html>\n<b/>\n</component_html>", "T".repeat(200))));
        assert!(describe_components(None).is_none());
    }

    #[test]
    fn pod_context_lists_fields_and_relations() {
        let e = PodContextEntry {
            id: "t".into(),
            kind: "resource".into(),
            label: "Tasks".into(),
            source: "home · /tasks".into(),
            access: PodAccess::Locked,
            stand: Some("2026-11-01".into()),
            fields: vec![
                PodField { p: "ical:summary".into(), v: Some("Buy milk".into()), kind: None },
                PodField { p: "ical:due".into(), v: None, kind: Some("date".into()) },
                PodField { p: "x".into(), v: None, kind: None },
            ],
            relations: Some(vec![PodRelation { p: "schema:about".into(), target: "course".into() }]),
            note: Some("listing is 401".into()),
        };
        let text = describe_pod_context(Some(&[e])).unwrap();
        assert!(text.ends_with(
            ":\n- Tasks [resource] · home · /tasks · locked (401), values from documented captures · 2026-11-01\n    ical:summary: Buy milk\n    ical:due (date)\n    x\n    schema:about → course\n    note: listing is 401"
        ));
    }

    #[test]
    fn guidelines_and_condense() {
        assert_eq!(describe_guidelines(None), "");
        let g = Guidelines { card_id: "c".into(), title: "Rules".into(), from_space: "Main canvas".into(), text: "Be blue".into() };
        assert!(describe_guidelines(Some(&g)).starts_with("Design guidelines for this space (\"Rules\", set in Main canvas)."));
        let html = format!("<div>\n  <script>x()</script><svg>p</svg><img src=\"data:{}\"><!-- n --></div>", "a".repeat(100));
        assert_eq!(condense(&html, 1000), "<div> <script>…</script><svg>…</svg><img src=\"data:…\"></div>");
        assert_eq!(condense("abcdef", 3), "abc …[truncated]");
    }
}
