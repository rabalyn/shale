//! The SVG picture of a drawing (SITE-037): what `svg::scene_svg` writes for the shapes, texts,
//! pictures and links of a scene.

use super::svg::{base64, scene_svg, size};
use crate::drawing::{ElementLink, Scene, SAMPLE_SCENE};
use std::collections::HashMap;

/// A scene of these elements (JSON objects), as an Excalidraw file has it.
fn scene(elements: &[&str]) -> Scene {
    Scene::parse(&format!("{{\"type\":\"excalidraw\",\"version\":2,\"elements\":[{}],\"appState\":{{\"viewBackgroundColor\":\"#ffffff\"}},\"files\":{{}}}}", elements.join(","))).unwrap()
}

fn nobody(_: &ElementLink) -> Option<String> {
    None
}

const BOX: &str = r##"{"id":"r1","type":"rectangle","x":0,"y":0,"width":200,"height":100,"strokeColor":"#1971c2","backgroundColor":"#a5d8ff","fillStyle":"solid","roughness":0,"seed":1,"link":"[[Public]]"}"##;
const LABEL: &str = r##"{"id":"t1","type":"text","x":10,"y":30,"width":400,"height":30,"text":"See [[Public]] and [[Hidden|my friend]]","fontSize":20,"fontFamily":2,"containerId":"r1","textAlign":"center"}"##;

// spec: SITE-037
#[test]
fn site_037_a_scene_is_drawn_with_its_shapes_and_texts_the_same_every_time() {
    let s = Scene::parse(SAMPLE_SCENE).unwrap();
    let svg = scene_svg(&s, &HashMap::new(), &nobody).unwrap();
    assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 "), "{svg}");
    let (w, h) = size(&s).unwrap();
    assert!(svg.contains(&format!("viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\"")), "{svg}");
    assert!(svg.contains("<polygon") && svg.contains("<polyline") && svg.contains("stroke=\"rgb(25,113,194)\""), "the shapes, in the colours of the drawing: {svg}");
    assert!(svg.contains(">Idea</text>") && svg.contains(">Note</text>") && svg.contains("text-anchor=\"middle\""), "the labels, centred: {svg}");
    assert_eq!(svg, scene_svg(&s, &HashMap::new(), &nobody).unwrap(), "the hand-drawn look does not change (DRAW-002)");
    assert!(svg.ends_with("</svg>\n") && !svg.contains("<script") && !svg.contains("href=\"http"), "nothing is loaded from another site");
    assert_eq!(svg.matches("<svg").count(), 1);
}

// spec: SITE-037
#[test]
fn site_037_a_scene_with_nothing_to_draw_has_no_picture() {
    assert!(scene_svg(&scene(&[]), &HashMap::new(), &nobody).is_none());
    assert!(size(&scene(&[])).is_none());
    let deleted = r##"{"id":"d","type":"rectangle","x":0,"y":0,"width":9,"height":9,"isDeleted":true}"##;
    assert!(scene_svg(&scene(&[deleted]), &HashMap::new(), &nobody).is_none(), "a deleted element is not drawn");
}

// spec: SITE-037, SITE-007
#[test]
fn site_037_the_text_of_a_link_is_its_shown_text_and_only_a_page_the_site_has_is_a_link() {
    let s = scene(&[BOX, LABEL]);
    let svg = scene_svg(&s, &HashMap::new(), &|l| match l {
        ElementLink::Note { target, .. } if target == "Public" => Some("/public/".into()),
        _ => None,
    })
    .unwrap();
    assert!(svg.contains("<a href=\"/public/\" target=\"_top\">"), "the shape and its label are one link: {svg}");
    assert_eq!(svg.matches("<a href=\"/public/\"").count(), 2, "{svg}");
    let (shape, label) = (svg.split("<a href").nth(1).unwrap().split("</a>").next().unwrap(), svg.split("<a href").nth(2).unwrap().split("</a>").next().unwrap());
    assert!(shape.contains("<polygon") && label.contains("<text"), "the shape and its label are links to the same page: {svg}");
    assert!(svg.contains("See Public and my friend"), "a link in a text is its shown text, `[[…]]` is not: {svg}");
    assert!(!svg.contains("[[") && !svg.contains("]]"));
    // nothing is a link when no page is found for it, and the name of the note stays out
    let plain = scene_svg(&s, &HashMap::new(), &nobody).unwrap();
    assert!(!plain.contains("<a ") && !plain.contains("href") && !plain.contains("Hidden"), "{plain}");
}

// spec: SITE-037
#[test]
fn site_037_text_is_escaped_labels_wrap_in_their_shape_and_a_free_text_keeps_its_lines() {
    let long = r##"{"id":"t2","type":"text","x":0,"y":0,"width":90,"height":60,"text":"alpha beta gamma delta epsilon","fontSize":20,"fontFamily":1,"containerId":"r1"}"##;
    let free = r##"{"id":"t3","type":"text","x":300,"y":0,"width":80,"height":40,"text":"a <b> & \"c\"\nsecond line","fontSize":16,"fontFamily":3,"textAlign":"right","strokeColor":"#e03131","opacity":50}"##;
    let svg = scene_svg(&scene(&[BOX, long, free]), &HashMap::new(), &nobody).unwrap();
    assert!(svg.matches("<text").count() >= 4, "the label wraps at the width of its shape: {svg}");
    assert!(svg.contains(">a &lt;b&gt; &amp; &quot;c&quot;</text>") && svg.contains(">second line</text>"), "{svg}");
    assert!(svg.contains("text-anchor=\"end\"") && svg.contains("fill=\"rgb(225,48,48)\" fill-opacity=\"0.5\""), "{svg}");
    assert!(svg.contains("monospace") && svg.contains("cursive") && !svg.contains("@font-face") && !svg.contains("url("), "the fonts of the viewer, none is loaded: {svg}");
}

// spec: SITE-037
#[test]
fn site_037_a_picture_in_a_drawing_is_written_into_the_svg_as_a_png() {
    let pic = r##"{"id":"p1","type":"image","x":10,"y":20,"width":40,"height":30,"fileId":"f1","opacity":100}"##;
    let s = scene(&[pic]);
    let mut pictures = HashMap::new();
    pictures.insert("f1".to_string(), image::RgbaImage::from_pixel(4, 3, image::Rgba([200, 30, 30, 255])));
    let svg = scene_svg(&s, &pictures, &nobody).unwrap();
    assert!(svg.contains("<image href=\"data:image/png;base64,iVBORw0KGgo") && svg.contains("x=\"20\" y=\"20\" width=\"40\" height=\"30\""), "{svg}");
    // a picture that is not there is the dashed box of the drawing view
    let svg = scene_svg(&s, &HashMap::new(), &nobody).unwrap();
    assert!(!svg.contains("<image") && svg.contains("<line"), "{svg}");
}

// spec: SITE-037
#[test]
fn site_037_base64_is_the_standard_padded_alphabet() {
    for (plain, coded) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
        assert_eq!(base64(plain.as_bytes()), coded);
    }
}
