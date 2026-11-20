//! The bundled UI font really applies (worker-6's parity pass saw Inter metrics).

use rune::TextStyle;
use rune::text::TextSystem;

const GSF: &[u8] = include_bytes!("../assets/fonts/GoogleSansFlex.ttf");

fn width(fonts: &[&'static [u8]], families: (&str, &str), s: &str, weight: f32) -> f32 {
    TextSystem::new(fonts, families).measure(s, TextStyle::new(14.0, weight), 1.0).0
}

#[test]
fn google_sans_flex_is_the_sans_slot_with_its_own_metrics_and_weights() {
    let s = "Its variations";
    let gsf = width(&[GSF], ("Google Sans Flex", "ui-monospace"), s, 500.0);
    let inter = width(&[], ("Inter", "Inter"), s, 500.0);
    let wrong = width(&[GSF], ("Google Sans", "ui-monospace"), s, 500.0);
    eprintln!("Its variations @14/500: Google Sans Flex {gsf:.2}, Inter {inter:.2}, misnamed family {wrong:.2}");
    let regular = width(&[GSF], ("Google Sans Flex", "ui-monospace"), s, 400.0);
    let bold = width(&[GSF], ("Google Sans Flex", "ui-monospace"), s, 700.0);
    eprintln!("Google Sans Flex 400 {regular:.2} / 500 {gsf:.2} / 700 {bold:.2}");
    assert!((gsf - inter).abs() > 0.5, "Google Sans Flex measures like Inter: {gsf} vs {inter}");
    assert!(bold > regular, "the wght axis applies: 400 {regular} vs 700 {bold}");
}
