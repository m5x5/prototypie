use super::*;

fn spec(vw: u32, vh: Option<u32>) -> ThumbSpec {
    ThumbSpec { viewport_w: vw, viewport_h: vh, out_w: 760 }
}

#[test]
fn layout_heights_follow_the_card_boxes() {
    assert_eq!(spec(1280, None).layout_h(), 960);
    assert_eq!(spec(390, None).layout_h(), 737);
    assert_eq!(spec(1024, Some(768)).layout_h(), 768);
}

#[test]
fn plain_page_captures_the_4_3_viewport() {
    let p = plan(&spec(1280, None), &Measured { doc_w: 1280.0, doc_h: 3000.0, crop: None });
    assert_eq!(p.rect, [0.0, 0.0, 1280.0, 960.0]);
    assert_eq!((p.out_w, p.out_h), (760, 570));
    assert_eq!(p.crop, None);
}

#[test]
fn crop_applies_only_to_non_phone_non_fixed() {
    let m = Measured { doc_w: 1280.0, doc_h: 900.0, crop: Some([400.0, 100.0, 380.0, 500.0]) };
    let p = plan(&spec(1280, None), &m);
    assert_eq!(p.rect, [400.0, 100.0, 380.0, 500.0]);
    assert_eq!(p.crop, Some(p.rect));
    // 380 css px at most 2 px each.
    assert_eq!((p.out_w, p.out_h), (760, 1000));
    assert_eq!(plan(&spec(1280, Some(800)), &m).crop, None);
    assert_eq!(plan(&spec(390, None), &m).crop, None);
}

#[test]
fn tall_crop_is_capped_at_three_widths() {
    let m = Measured { doc_w: 1280.0, doc_h: 5000.0, crop: Some([0.0, 0.0, 300.0, 2000.0]) };
    assert_eq!(plan(&spec(1280, None), &m).rect, [0.0, 0.0, 300.0, 900.0]);
}

#[test]
fn buckets_and_reuse() {
    assert_eq!(bucket(1), 256);
    assert_eq!(bucket(760), 768);
    assert!(serves(768, 900));
    assert!(!serves(768, 1000));
}

#[test]
fn preview_css_goes_before_head_end() {
    let out = for_preview("<html><HEAD><title>x</title></HEAD><body></body></html>", true);
    assert!(out.contains("transparent!important}</style></HEAD>"));
    assert!(for_preview("<p>hi</p>", false).starts_with("<style data-prototypie-preview>"));
}
