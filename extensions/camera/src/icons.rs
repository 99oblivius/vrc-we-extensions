//! The icons the camera's views show, drawn by the client in its theme's
//! colour: each a 16 × 16 SVG of plain shapes.

use vrc_we_sdk::Icon;

/// Each icon's name and the shapes inside its SVG.
const SHAPES: [(&str, &str); 30] = [
    ("up", r#"<path d="M8 3l5 8H3z"/>"#),
    ("down", r#"<path d="M8 13L3 5h10z"/>"#),
    ("left", r#"<path d="M3 8l8-5v10z"/>"#),
    ("right", r#"<path d="M13 8L5 3v10z"/>"#),
    ("rise", r#"<path d="M3 2h10v2H3zM8 5l4 5H9v4H7v-4H4z"/>"#),
    ("sink", r#"<path d="M3 12h10v2H3zM8 11L4 6h3V2h2v4h3z"/>"#),
    (
        "tilt_up",
        r#"<path d="M3 11l5-5 5 5-1.5 1.5L8 9l-3.5 3.5z"/>"#,
    ),
    (
        "tilt_down",
        r#"<path d="M3 5l5 5 5-5-1.5-1.5L8 7 4.5 3.5z"/>"#,
    ),
    (
        "pan_left",
        r#"<path d="M11 3L6 8l5 5-1.5 1.5L3 8l6.5-6.5z"/>"#,
    ),
    (
        "pan_right",
        r#"<path d="M5 3l5 5-5 5 1.5 1.5L13 8 6.5 1.5z"/>"#,
    ),
    (
        "roll_left",
        r#"<path d="M8 2a6 6 0 1 1-5.7 7.9l1.9-.6A4 4 0 1 0 8 4v2L4 3l4-3z"/>"#,
    ),
    (
        "roll_right",
        r#"<path d="M8 2a6 6 0 1 0 5.7 7.9l-1.9-.6A4 4 0 1 1 8 4v2l4-3-4-3z"/>"#,
    ),
    (
        "capture",
        r#"<path fill-rule="evenodd" d="M1 5h3l1.5-2h5L12 5h3v9H1zM8 6.5a3 3 0 1 0 0 6 3 3 0 0 0 0-6z"/>"#,
    ),
    (
        "timer",
        r#"<path fill-rule="evenodd" d="M6 0h4v2H6zM8 3a6.5 6.5 0 1 0 0 13A6.5 6.5 0 0 0 8 3zm0 2a4.5 4.5 0 1 1 0 9 4.5 4.5 0 0 1 0-9zM7 6h2v4H7z"/>"#,
    ),
    (
        "close",
        r#"<path d="M3.4 2L8 6.6 12.6 2 14 3.4 9.4 8l4.6 4.6-1.4 1.4L8 9.4 3.4 14 2 12.6 6.6 8 2 3.4z"/>"#,
    ),
    ("play", r#"<path d="M4 2l10 6-10 6z"/>"#),
    ("pause", r#"<path d="M3 2h4v12H3zM9 2h4v12H9z"/>"#),
    ("stop", r#"<path d="M3 3h10v10H3z"/>"#),
    ("edit", r#"<path d="M11 1l4 4-9 9H2v-4zM2 15h12v1H2z"/>"#),
    ("add", r#"<path d="M7 2h2v5h5v2H9v5H7V9H2V7h5z"/>"#),
    (
        "delete",
        r#"<path d="M5 1h6v2h4v2H1V3h4zM2 6h12l-1 10H3z"/>"#,
    ),
    (
        "duplicate",
        r#"<path fill-rule="evenodd" d="M5 1h10v10H5zm2 2v6h6V3zM1 5h3v2H3v6h6v-1h2v3H1z"/>"#,
    ),
    ("record", r#"<circle cx="8" cy="8" r="6"/>"#),
    ("go", r#"<path d="M1 7h9V4l5 4-5 4V9H1zM14 2h1v12h-1z"/>"#),
    ("earlier", r#"<path d="M2 2h2v12H2zM14 3v10L6 8z"/>"#),
    ("later", r#"<path d="M12 2h2v12h-2zM2 3v10l8-5z"/>"#),
    (
        "keyframe",
        r#"<path d="M6 1l5 5-5 5-5-5zM12 9h2v2h2v2h-2v2h-2v-2h-2v-2h2z"/>"#,
    ),
    (
        "target",
        r#"<path fill-rule="evenodd" d="M8 3a5 5 0 1 1 0 10A5 5 0 0 1 8 3zm0 2a3 3 0 1 0 0 6 3 3 0 0 0 0-6z"/><path d="M7 0h2v3H7zM7 13h2v3H7zM0 7h3v2H0zM13 7h3v2h-3zM7 7h2v2H7z"/>"#,
    ),
    (
        "level",
        r#"<path fill-rule="evenodd" d="M0 5h16v6H0zm2 2v2h12V7z"/><path d="M6.5 7h3v2h-3z"/>"#,
    ),
    (
        "pin",
        r#"<path fill-rule="evenodd" d="M8 1a5 5 0 0 1 5 5c0 4-5 9-5 9S3 10 3 6a5 5 0 0 1 5-5zm0 3a2 2 0 1 0 0 4 2 2 0 0 0 0-4z"/>"#,
    ),
];

/// The icons its manifest packages.
pub(crate) fn icons() -> Vec<Icon> {
    SHAPES
        .iter()
        .map(|(name, shapes)| Icon {
            name: (*name).to_owned(),
            svg: format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">{shapes}</svg>"#
            ),
        })
        .collect()
}
