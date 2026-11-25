//! The icons the camera's views show, drawn by the client in its theme's
//! colour: each a 16 × 16 SVG of plain shapes.

use vrc_we_sdk::Icon;

/// Each icon's name and the shapes inside its SVG.
const SHAPES: [(&str, &str); 38] = [
    (
        "forward",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M8 14V2.5M3.5 7L8 2.5 12.5 7"/>"##,
    ),
    (
        "back",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M8 2v11.5M3.5 9L8 13.5 12.5 9"/>"##,
    ),
    (
        "left",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M14 8H2.5M7 3.5L2.5 8 7 12.5"/>"##,
    ),
    (
        "right",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M2 8h11.5M9 3.5L13.5 8 9 12.5"/>"##,
    ),
    (
        "rise",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M8 11V2M4.5 5.5L8 2l3.5 3.5M2.5 14.2h11"/>"##,
    ),
    (
        "sink",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M8 2v9M4.5 7.5L8 11l3.5-3.5M2.5 14.2h11"/>"##,
    ),
    (
        "pan_left",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M14 10.2A6 6.8 0 0 0 3.4 5.8"/><path d="M1.4 4.1L1.1 9.6 5.4 7.5z"/><circle cx="8" cy="11.2" r="1.5"/>"##,
    ),
    (
        "pan_right",
        r##"<g transform="matrix(-1 0 0 1 16 0)"><path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M14 10.2A6 6.8 0 0 0 3.4 5.8"/><path d="M1.4 4.1L1.1 9.6 5.4 7.5z"/><circle cx="8" cy="11.2" r="1.5"/></g>"##,
    ),
    (
        "tilt_up",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M5.8 14A6.8 6 0 0 0 10.2 3.4"/><path d="M11.9 1.4L6.4 1.1 8.5 5.4z"/><circle cx="4.8" cy="8" r="1.5"/>"##,
    ),
    (
        "tilt_down",
        r##"<g transform="matrix(1 0 0 -1 0 16)"><path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M5.8 14A6.8 6 0 0 0 10.2 3.4"/><path d="M11.9 1.4L6.4 1.1 8.5 5.4z"/><circle cx="4.8" cy="8" r="1.5"/></g>"##,
    ),
    (
        "roll_left",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M11.1 12.7A5.4 5.4 0 1 0 3.2 6.6"/><path d="M0.7 5.8L2.4 10.9 5.6 7.4z"/><circle cx="8.4" cy="8" r="1.5"/>"##,
    ),
    (
        "roll_right",
        r##"<g transform="matrix(-1 0 0 1 16 0)"><path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M11.1 12.7A5.4 5.4 0 1 0 3.2 6.6"/><path d="M0.7 5.8L2.4 10.9 5.6 7.4z"/><circle cx="8.4" cy="8" r="1.5"/></g>"##,
    ),
    (
        "level",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M1 8h3.5M11.5 8H15M8 5.2a2.8 2.8 0 1 0 0 5.6 2.8 2.8 0 0 0 0-5.6"/>"##,
    ),
    (
        "capture",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M1.5 5h3l1.3-2h4.4l1.3 2h3v8.5h-13zM8 6.8a2.4 2.4 0 1 0 0 4.8 2.4 2.4 0 0 0 0-4.8"/>"##,
    ),
    (
        "timer",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M6 1.2h4M8 3.6a5.5 5.5 0 1 0 0 11 5.5 5.5 0 0 0 0-11M8 6.4v2.9l2 1.2"/>"##,
    ),
    (
        "power",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M8 1.5v6M4.4 3.8a5.6 5.6 0 1 0 7.2 0"/>"##,
    ),
    (
        "stream",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M5.4 5.4a3.7 3.7 0 0 0 0 5.2M10.6 5.4a3.7 3.7 0 0 1 0 5.2M3.2 3.2a6.8 6.8 0 0 0 0 9.6M12.8 3.2a6.8 6.8 0 0 1 0 9.6"/><circle cx="8" cy="8" r="1.5"/>"##,
    ),
    (
        "emoji",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M8 1.8a6.2 6.2 0 1 0 0 12.4A6.2 6.2 0 0 0 8 1.8M5.2 9.6a3.4 3.4 0 0 0 5.6 0"/><circle cx="5.8" cy="6.4" r="1"/><circle cx="10.2" cy="6.4" r="1"/>"##,
    ),
    (
        "layers",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M8 1.8l6 3.1-6 3.1-6-3.1zM2 8.2l6 3.1 6-3.1M2 11.3l6 3.1 6-3.1"/>"##,
    ),
    (
        "print",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M2.8 1.8h10.4v12.4H2.8zM5 4h6v6H5z"/>"##,
    ),
    (
        "drone",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M4.5 4.5l7 7M11.5 4.5l-7 7M3.5 1.5a2 2 0 1 0 0 4 2 2 0 0 0 0-4M12.5 1.5a2 2 0 1 0 0 4 2 2 0 0 0 0-4M3.5 10.5a2 2 0 1 0 0 4 2 2 0 0 0 0-4M12.5 10.5a2 2 0 1 0 0 4 2 2 0 0 0 0-4"/>"##,
    ),
    ("play", r##"<path d="M4 2l10 6-10 6z"/>"##),
    (
        "pause",
        r##"<path d="M3.5 2.5h3v11h-3zM9.5 2.5h3v11h-3z"/>"##,
    ),
    ("stop", r##"<path d="M3.5 3.5h9v9h-9z"/>"##),
    (
        "add",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M8 2.5v11M2.5 8h11"/>"##,
    ),
    (
        "duplicate",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M5.5 5.5h8.5V14H5.5zM10.5 5.5V2H2v8.5h3.5"/>"##,
    ),
    (
        "go",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M1.5 8h8.5M6.5 4.5L10 8l-3.5 3.5M13.8 3v10"/>"##,
    ),
    (
        "earlier",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M2.5 3v10M13.5 8H6M9 5L6 8l3 3"/>"##,
    ),
    (
        "later",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M13.5 3v10M2.5 8H10M7 5l3 3-3 3"/>"##,
    ),
    (
        "keyframe",
        r##"<path d="M6 2.5l5 5-5 5-5-5z"/><path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M13 9.5v5M10.5 12h5"/>"##,
    ),
    (
        "insert",
        r##"<path d="M4 4.5L7.5 8 4 11.5.5 8z"/><path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M9.5 8H15M12.5 5.2V10.8"/>"##,
    ),
    (
        "target",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M8 3.2a4.8 4.8 0 1 0 0 9.6 4.8 4.8 0 0 0 0-9.6M8 1v3M8 12v3M1 8h3M12 8h3"/>"##,
    ),
    (
        "pin",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M8 14.5S3 9.8 3 6a5 5 0 0 1 10 0c0 3.8-5 8.5-5 8.5zM8 4.4a1.7 1.7 0 1 0 0 3.4 1.7 1.7 0 0 0 0-3.4"/>"##,
    ),
    ("key", r##"<path d="M8 1.5L14.5 8 8 14.5 1.5 8z"/>"##),
    (
        "fly",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M1.8 7.4l12.4-5.6-4.9 12.4L7 9z"/>"##,
    ),
    (
        "lens",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M8 1.8a6.2 6.2 0 1 0 0 12.4A6.2 6.2 0 0 0 8 1.8M8 5.2a2.8 2.8 0 1 0 0 5.6 2.8 2.8 0 0 0 0-5.6"/>"##,
    ),
    (
        "straight",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M2.5 13.5l11-11"/>"##,
    ),
    (
        "smooth",
        r##"<path fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M2 13C7 13 9 3 14 3"/>"##,
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
