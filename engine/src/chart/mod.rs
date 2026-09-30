//! # Chart Rendering
//!
//! Engine-native chart generation. Each chart type produces a flat list of
//! `ChartPrimitive` drawing commands. The PDF renderer iterates the list to
//! emit vector graphics directly — no SVG intermediary.

pub mod area;
pub mod bar;
pub mod dot;
pub mod line;
pub mod pie;

use crate::font::metrics::StandardFontMetrics;
use crate::font::StandardFont;
use crate::style::Color;

/// A drawing primitive emitted by chart builders.
#[derive(Debug, Clone)]
pub enum ChartPrimitive {
    /// A filled rectangle.
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        fill: Color,
    },
    /// A stroked line segment.
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        stroke: Color,
        width: f64,
    },
    /// A stroked polyline (connected line segments).
    Polyline {
        points: Vec<(f64, f64)>,
        stroke: Color,
        width: f64,
    },
    /// A filled closed polygon.
    FilledPath {
        points: Vec<(f64, f64)>,
        fill: Color,
        opacity: f64,
    },
    /// A filled circle.
    Circle {
        cx: f64,
        cy: f64,
        r: f64,
        fill: Color,
    },
    /// A filled arc sector (pie slice).
    ArcSector {
        cx: f64,
        cy: f64,
        r: f64,
        start_angle: f64,
        end_angle: f64,
        fill: Color,
    },
    /// A text label.
    Label {
        text: String,
        x: f64,
        y: f64,
        font_size: f64,
        color: Color,
        anchor: TextAnchor,
    },
    /// A text label rotated to read bottom to top, centred on (x, y): a
    /// y-axis title.
    VerticalLabel {
        text: String,
        x: f64,
        y: f64,
        font_size: f64,
        color: Color,
    },
}

/// Text horizontal alignment for labels.
#[derive(Debug, Clone, Copy)]
pub enum TextAnchor {
    Left,
    Center,
    Right,
}

// ── Constants ──────────────────────────────────────────────────

/// Default color palette for chart series/slices.
pub const DEFAULT_COLORS: &[&str] = &[
    "#1a365d", "#2b6cb0", "#3182ce", "#4299e1", "#63b3ed", "#90cdf4", "#e53e3e", "#dd6b20",
    "#38a169", "#805ad5",
];

pub const Y_AXIS_WIDTH: f64 = 28.0;
pub const X_AXIS_HEIGHT: f64 = 20.0;
pub const AXIS_LABEL_FONT: f64 = 8.0;
pub const LABEL_MARGIN: f64 = 4.0;
pub const TITLE_FONT: f64 = 11.0;
pub const TITLE_HEIGHT: f64 = 20.0;
pub const GRID_COLOR: Color = Color {
    r: 0.88,
    g: 0.88,
    b: 0.88,
    a: 1.0,
};
pub const AXIS_COLOR: Color = Color {
    r: 0.4,
    g: 0.4,
    b: 0.4,
    a: 1.0,
};
pub const LABEL_COLOR: Color = Color {
    r: 0.3,
    g: 0.3,
    b: 0.3,
    a: 1.0,
};

// ── Helpers ────────────────────────────────────────────────────

/// Helvetica metrics for measuring label widths.
fn helvetica_metrics() -> StandardFontMetrics {
    StandardFont::Helvetica.metrics()
}

/// Measure the width of a label string in Helvetica at the given font size.
pub fn measure_label(text: &str, font_size: f64) -> f64 {
    helvetica_metrics().measure_string(text, font_size, 0.0)
}

/// Round a range maximum to a "nice" number for axis ticks.
pub fn nice_number(value: f64) -> f64 {
    if value <= 0.0 {
        return 1.0;
    }
    let exp = value.log10().floor();
    let frac = value / 10.0_f64.powf(exp);
    let nice = if frac <= 1.0 {
        1.0
    } else if frac <= 2.0 {
        2.0
    } else if frac <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * 10.0_f64.powf(exp)
}

/// Format a number compactly (1000 → "1K", 1000000 → "1M").
pub fn format_number(value: f64) -> String {
    if value.abs() >= 1_000_000.0 {
        format!("{:.1}M", value / 1_000_000.0)
    } else if value.abs() >= 1_000.0 {
        format!("{:.1}K", value / 1_000.0)
    } else if value == value.floor() {
        format!("{}", value as i64)
    } else {
        format!("{:.1}", value)
    }
}

/// Lighten a hex color toward white by the given factor (0.0=unchanged, 1.0=white).
pub fn lighten_color(color: &Color, factor: f64) -> Color {
    Color {
        r: color.r + (1.0 - color.r) * factor,
        g: color.g + (1.0 - color.g) * factor,
        b: color.b + (1.0 - color.b) * factor,
        a: color.a,
    }
}

/// Parse a hex color string (#RGB or #RRGGBB) to a Color.
pub fn parse_hex_color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).unwrap_or(0);
            Color::rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0)
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
            Color::rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0)
        }
        _ => Color::BLACK,
    }
}

/// Get a color from the default palette by index, or parse a custom color string.
pub fn resolve_color(custom: Option<&str>, index: usize) -> Color {
    match custom {
        Some(c) => parse_hex_color(c),
        None => parse_hex_color(DEFAULT_COLORS[index % DEFAULT_COLORS.len()]),
    }
}
