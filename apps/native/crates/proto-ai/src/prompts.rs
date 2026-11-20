//! Prompt text, ported verbatim from the reference: `api/variations/route.ts` (DESIGN_SYSTEM, describeTask, plan
//! and build prompts), `lib/server/ai-edit.ts` (EDIT_SYSTEM, describeEdit), `api/spokes/route.ts`,
//! `api/decisions/route.ts`.

use std::sync::LazyLock;

use crate::context::{condense, describe_components, describe_pod_context, truncate_chars};
use crate::types::{Annotation, DecisionCandidate, Direction, EditRequest, EditTarget, Scope, VariationRequest};

/// Card titles live inside a space (= the project), so they never repeat it.
pub const TITLE_RULE: &str = "Titles are short and space-scoped: 1-4 words describing the design (e.g. \"Werkzeugleiste\", \"Kompakte Sidebar\"). Never include the project, product or space name, and never words like \"Komponente\", \"Component\", \"Variant\" or \"Direction\".";

pub static DESIGN_SYSTEM: LazyLock<String> = LazyLock::new(|| {
    format!(
        "You are a senior product designer who prototypes in code.
You produce self-contained HTML documents: inline <style>, optional inline <script> for interactivity, no external JS.
Google Fonts <link> tags are allowed. Use real-feeling copy, not lorem ipsum. Images: use CSS gradients, SVG or emoji, never external image URLs.
Icons: inline SVG. If Lucide icons are asked for, inline the real Lucide SVG markup (24x24 viewBox, fill=\"none\", stroke=\"currentColor\", stroke-width=\"2\", round caps and joins).
The document is rendered in a sandboxed iframe. Make it responsive and polished; interactive states (hover, focus, toggles, tabs) are encouraged.
When a base design is given, keep its product, content and visual language unless the instruction says otherwise.
Hard rules: design only the component the instruction talks about and focus on it; don't add or redesign anything else.
No explanatory text in the document (no headings, captions, labels, spec lists, legends or annotations about the design); put reasoning in \"rationale\".
Each output is ONE variant: never show several options, states or versions side by side in one document.
{TITLE_RULE}"
    )
});

/// `system` for every call of a variations job: DESIGN_SYSTEM plus the space's guidelines, if any.
pub fn design_system(guidelines: &str) -> String {
    if guidelines.is_empty() { DESIGN_SYSTEM.clone() } else { format!("{}\n\n{guidelines}", *DESIGN_SYSTEM) }
}

fn numbered_notes(notes: &[Annotation]) -> String {
    notes
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let el = a.element.as_deref().filter(|e| !e.is_empty()).map(|e| format!("[{e}] ")).unwrap_or_default();
            format!("{}. {el}{}", i + 1, a.comment)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !s.is_empty())
}

/// The task part shared by the plan and build prompts (`describeTask`).
pub fn describe_task(req: &VariationRequest) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(bases) = req.bases.as_ref().filter(|b| !b.is_empty()) {
        let designs: Vec<String> = bases
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let title = if b.title.is_empty() { String::new() } else { format!(" (\"{}\")", b.title) };
                format!("Design {}{title}:\n<base_html>\n{}\n</base_html>", i + 1, b.html)
            })
            .collect();
        parts.push(format!(
            "Combine these {} designs into ONE new design. Take the strongest ideas from each (layout, hierarchy, components, interactions, visual language) and merge them into a coherent whole; don't place them side by side. Keep their shared product and content.\n\n{}",
            bases.len(),
            designs.join("\n\n")
        ));
    } else if let Some(html) = non_empty(req.base_html.as_deref()) {
        let title = non_empty(req.base_title.as_deref()).map(|t| format!(" (\"{t}\")")).unwrap_or_default();
        parts.push(format!("Base design{title}:\n<base_html>\n{html}\n</base_html>"));
    }
    if let Some(focus) = non_empty(req.focus.as_deref()) {
        parts.push(if req.scope == Some(Scope::Component) {
            format!("Focus ONLY on this component/region: {focus}. Each output document must contain just that component, centred on a neutral backdrop that matches the base design's styling, so it can be judged in isolation.")
        } else {
            format!("Change ONLY this component/region: {focus}. Return the full page, but everything outside it must stay identical to the base HTML (same markup, styles, copy and order; byte-for-byte where possible). Don't restyle, reflow or re-theme the rest of the page. Directions differ only in how they treat this region.")
        });
    }
    if let Some(notes) = req.annotations.as_ref().filter(|a| !a.is_empty()) {
        parts.push(format!(
            "Feedback pinned on the base design with Annotate (element -> note). Address every note by changing only the annotated elements (and their children); anything not annotated stays exactly as in the base design. Keep supporting CSS/JS changes minimal and scoped to those elements:\n{}",
            numbered_notes(notes)
        ));
    }
    if let Some(pod) = describe_pod_context(req.pod_context.as_deref()) {
        parts.push(pod);
    }
    if let Some(components) = describe_components(req.components.as_deref()) {
        parts.push(components);
    }
    let instruction = if req.instruction.is_empty() { "Explore meaningfully different directions." } else { &req.instruction };
    parts.push(format!("Instruction from the designer: {instruction}"));
    parts.join("\n\n")
}

/// Step 1 of a codex job: plan `count` directions.
pub fn plan_prompt(task: &str, count: usize) -> String {
    if count == 1 {
        format!("{task}\n\nPropose exactly 1 design direction: a short title (1-4 words, no project/space name, no \"Komponente\"/\"Component\") and a 1-3 sentence brief.")
    } else {
        format!("{task}\n\nPropose exactly {count} clearly distinct design directions (layout, hierarchy, tone, interaction - not just colour swaps). For each give a short title (1-4 words, no project/space name, no \"Komponente\"/\"Component\") and a 1-3 sentence brief.")
    }
}

/// Step 2 of a codex job: build one direction.
pub fn build_prompt(task: &str, dir: &Direction) -> String {
    format!(
        "{task}\n\nBuild this direction:\nTitle: {}\nBrief: {}\n\nSubmit the title, a one-or-two sentence rationale aimed at a stakeholder, and the complete HTML document.",
        dir.title, dir.brief
    )
}

pub const EDIT_SYSTEM: &str = "You are a senior product designer who edits HTML prototypes in place.
You get one self-contained HTML document and an instruction. Return the SAME document with only the requested change
applied: keep every other element, style, script, copy and the order exactly as it is (byte-for-byte where possible).
Don't restyle, re-theme, reflow or \"improve\" anything that wasn't asked for. Keep it self-contained: inline <style>,
optional inline <script>, Google Fonts <link> allowed, no external JS or image URLs.
No explanatory text in the document; put a one-or-two sentence summary of what changed in \"rationale\".";

pub fn edit_system(guidelines: &str) -> String {
    if guidelines.is_empty() { EDIT_SYSTEM.to_string() } else { format!("{EDIT_SYSTEM}\n\n{guidelines}") }
}

/// `describeEdit`
pub fn describe_edit(d: &EditTarget, req: &EditRequest) -> String {
    let mut parts = vec![format!("Design to edit (\"{}\"):\n<html_document>\n{}\n</html_document>", d.title, d.html)];
    if let Some(focus) = non_empty(req.focus.as_deref()) {
        parts.push(format!("Change ONLY this element/region: {focus}. Everything outside it stays identical."));
    }
    if let Some(notes) = req.annotations.as_ref().filter(|a| !a.is_empty()) {
        parts.push(format!(
            "Notes pinned on the design (element -> note). Address each by changing only the annotated elements:\n{}",
            numbered_notes(notes)
        ));
    }
    if let Some(pod) = describe_pod_context(req.pod_context.as_deref()) {
        parts.push(pod);
    }
    if let Some(components) = describe_components(req.components.as_deref()) {
        parts.push(components);
    }
    parts.push(format!("Edit instruction: {}", req.instruction));
    parts.join("\n\n")
}

pub const SPOKES_MAX_HTML: usize = 150_000;
pub const SPOKES_SYSTEM: &str = "You are a senior product designer. You look at a rough prototype (usually a full page, sometimes a single section or component) and break it into the individual components a designer would iterate on one at a time (hero, primary CTA, pricing card, nav, testimonial, form, footer...).";

pub fn spokes_prompt(html: &str, title: Option<&str>) -> String {
    let title = non_empty(title).map(|t| format!(" (\"{t}\")")).unwrap_or_default();
    format!(
        "Page{title}:\n<page_html>\n{}\n</page_html>\n\nIdentify between 3 and 8 distinct, self-contained components on this page that are worth exploring in isolation, most impactful first. If it is a single small component, list its parts instead (2 or more; never invent parts that are not there). For each give:\n- name: 1-4 words\n- description: one sentence describing what it is and what it contains, specific enough to find it on the page\n- selector: a CSS selector or short locator for it in the HTML (e.g. \"header nav\", \"section.pricing .card:first-child\")",
        truncate_chars(html, SPOKES_MAX_HTML)
    )
}

/// Characters of HTML across all decision candidates.
pub const DECISIONS_HTML_BUDGET: usize = 90_000;
pub const DECISIONS_SYSTEM: &str = "You are a design lead helping a product designer prepare a decision review for stakeholders.
The designer has explored many competing HTML prototypes. Stakeholders should not wade through the messy exploration;
they should answer a few crisp, consequential questions by comparing concrete options side by side.

Your job: look at the candidate designs and surface the 1-4 core decisions the designer is actually grappling with
(e.g. \"How prominent should pricing be?\", \"Guided wizard or single long form?\", \"Playful or corporate tone?\").
For each decision:
- question: a short, plain-language question a non-designer can answer (max ~12 words).
- why: one sentence on why this choice matters for users or the business.
- options: 2-4 candidate designs that best embody the competing answers, each with a 2-6 word label that names the
  answer it represents (e.g. \"Pricing up front\", \"Pricing after value\"), not the design's title.
Only use designId values from the provided candidates. A design may appear in several decisions if it is a clear
example of different answers, but prefer distinct designs. Order decisions from most to least consequential.
Skip trivial differences (colour tweaks, copy typos). If the candidates only really differ along one axis, return one decision.";

pub fn decisions_prompt(designs: &[DecisionCandidate], context: Option<&str>) -> String {
    let per_design = (DECISIONS_HTML_BUDGET / designs.len().max(1)).clamp(2_000, 16_000);
    let candidates: Vec<String> = designs
        .iter()
        .map(|d| {
            format!(
                "<candidate designId=\"{}\">\n<title>{}</title>\n<rationale>{}</rationale>\n<html>{}</html>\n</candidate>",
                d.id,
                d.title,
                d.rationale,
                condense(&d.html, per_design)
            )
        })
        .collect();
    let note = non_empty(context).map(|c| format!("Note from the designer: {c}\n\n")).unwrap_or_default();
    format!("{}\n\n{note}Surface the core decisions for the stakeholder poll.", candidates.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BaseDesign;

    #[test]
    fn task_with_base_focus_notes() {
        let req = VariationRequest {
            base_html: Some("<p>hi</p>".into()),
            base_title: Some("Hero".into()),
            focus: Some("the CTA".into()),
            annotations: Some(vec![
                Annotation { element: Some("button".into()), comment: "bigger".into() },
                Annotation { element: None, comment: "warmer".into() },
            ]),
            instruction: "go".into(),
            ..Default::default()
        };
        let t = describe_task(&req);
        assert!(t.starts_with("Base design (\"Hero\"):\n<base_html>\n<p>hi</p>\n</base_html>\n\nChange ONLY this component/region: the CTA. Return the full page"));
        assert!(t.contains(":\n1. [button] bigger\n2. warmer\n\nInstruction from the designer: go"));
    }

    #[test]
    fn task_combine_component_default_instruction() {
        let req = VariationRequest {
            base_html: Some("ignored".into()),
            bases: Some(vec![
                BaseDesign { id: "a".into(), title: "A".into(), html: "<a>".into() },
                BaseDesign { id: "b".into(), title: "".into(), html: "<b>".into() },
            ]),
            focus: Some("nav".into()),
            scope: Some(Scope::Component),
            ..Default::default()
        };
        let t = describe_task(&req);
        assert!(t.starts_with("Combine these 2 designs into ONE new design."));
        assert!(t.contains("Design 1 (\"A\"):\n<base_html>\n<a>\n</base_html>\n\nDesign 2:\n<base_html>\n<b>\n</base_html>"));
        assert!(!t.contains("ignored"));
        assert!(t.contains("Focus ONLY on this component/region: nav. Each output document"));
        assert!(t.ends_with("Instruction from the designer: Explore meaningfully different directions."));
    }

    #[test]
    fn plan_build_and_system() {
        assert!(plan_prompt("T", 1).starts_with("T\n\nPropose exactly 1 design direction"));
        assert!(plan_prompt("T", 4).starts_with("T\n\nPropose exactly 4 clearly distinct"));
        let b = build_prompt("T", &Direction { title: "Dock".into(), brief: "Bottom".into() });
        assert_eq!(
            b,
            "T\n\nBuild this direction:\nTitle: Dock\nBrief: Bottom\n\nSubmit the title, a one-or-two sentence rationale aimed at a stakeholder, and the complete HTML document."
        );
        assert!(DESIGN_SYSTEM.starts_with("You are a senior product designer who prototypes in code.\n"));
        assert!(DESIGN_SYSTEM.ends_with("\"Variant\" or \"Direction\"."));
        assert_eq!(design_system(""), *DESIGN_SYSTEM);
        assert!(design_system("G").ends_with("\n\nG"));
    }

    #[test]
    fn edit_spokes_decisions() {
        let d = EditTarget { id: "d".into(), title: "Card".into(), html: "<p>".into(), space_id: "s".into() };
        let req = EditRequest { design_id: "d".into(), instruction: "blue".into(), focus: Some("h1".into()), ..Default::default() };
        assert_eq!(
            describe_edit(&d, &req),
            "Design to edit (\"Card\"):\n<html_document>\n<p>\n</html_document>\n\nChange ONLY this element/region: h1. Everything outside it stays identical.\n\nEdit instruction: blue"
        );
        assert!(edit_system("").ends_with("in \"rationale\"."));
        let s = spokes_prompt(&"§".repeat(200_000), Some("Home"));
        assert!(s.starts_with("Page (\"Home\"):\n<page_html>\n"));
        assert_eq!(s.matches('§').count(), SPOKES_MAX_HTML);
        let c = DecisionCandidate { id: "d1".into(), title: "T".into(), rationale: "R".into(), html: "<b> x </b>".into() };
        assert_eq!(
            decisions_prompt(&[c], Some("ctx")),
            "<candidate designId=\"d1\">\n<title>T</title>\n<rationale>R</rationale>\n<html><b> x </b></html>\n</candidate>\n\nNote from the designer: ctx\n\nSurface the core decisions for the stakeholder poll."
        );
    }
}
