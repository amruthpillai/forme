//! Dot plot (scatter plot) builder.

use super::*;
use crate::model::DotPlotGroup;

/// Configuration for dot plot rendering.
pub struct DotPlotConfig {
    pub x_min: Option<f64>,
    pub x_max: Option<f64>,
    pub y_min: Option<f64>,
    pub y_max: Option<f64>,
    pub x_label: Option<String>,
    pub y_label: Option<String>,
    pub show_legend: bool,
    pub dot_size: f64,
}

/// Build dot plot primitives from grouped data.
pub fn build(
    width: f64,
    height: f64,
    groups: &[DotPlotGroup],
    config: &DotPlotConfig,
) -> Vec<ChartPrimitive> {
    if groups.is_empty() {
        return vec![];
    }

    let mut primitives = Vec::new();

    // Legend space
    let legend_width = if config.show_legend { 80.0 } else { 0.0 };

    // An axis title gets its own line of room: the y title a column left of
    // the tick numbers, the x title a row below them. Without one the plot
    // keeps its old geometry.
    let title_room = AXIS_LABEL_FONT + LABEL_MARGIN;
    let y_title_room = if config.y_label.is_some() {
        title_room
    } else {
        0.0
    };
    let x_title_room = if config.x_label.is_some() {
        title_room
    } else {
        0.0
    };

    let plot_left = Y_AXIS_WIDTH + y_title_room;
    let plot_top = LABEL_MARGIN;
    let plot_right = width - LABEL_MARGIN - legend_width;
    let plot_bottom = height - X_AXIS_HEIGHT - x_title_room;
    let plot_width = plot_right - plot_left;
    let plot_height = plot_bottom - plot_top;

    if plot_width <= 0.0 || plot_height <= 0.0 {
        return vec![];
    }

    // Compute data bounds
    let all_points: Vec<(f64, f64)> = groups.iter().flat_map(|g| g.data.iter().copied()).collect();
    if all_points.is_empty() {
        return vec![];
    }

    let data_x_min = all_points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let data_x_max = all_points
        .iter()
        .map(|p| p.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let data_y_min = all_points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let data_y_max = all_points
        .iter()
        .map(|p| p.1)
        .fold(f64::NEG_INFINITY, f64::max);

    let x_min = config.x_min.unwrap_or(data_x_min.min(0.0));
    let x_max = config.x_max.unwrap_or(nice_number(data_x_max));
    let y_min = config.y_min.unwrap_or(data_y_min.min(0.0));
    let y_max = config.y_max.unwrap_or(nice_number(data_y_max));

    let x_range = (x_max - x_min).max(1.0);
    let y_range = (y_max - y_min).max(1.0);

    // Grid lines (5 ticks each axis)
    let ticks = 5;
    for i in 0..=ticks {
        let frac = i as f64 / ticks as f64;
        // Horizontal grid
        let y = plot_bottom - frac * plot_height;
        primitives.push(ChartPrimitive::Line {
            x1: plot_left,
            y1: y,
            x2: plot_right,
            y2: y,
            stroke: GRID_COLOR,
            width: 0.5,
        });
        // Y label
        let y_val = y_min + frac * y_range;
        primitives.push(ChartPrimitive::Label {
            text: format_number(y_val),
            x: plot_left - LABEL_MARGIN,
            y: y + AXIS_LABEL_FONT * 0.35,
            font_size: AXIS_LABEL_FONT,
            color: LABEL_COLOR,
            anchor: TextAnchor::Right,
        });
        // X label
        let x = plot_left + frac * plot_width;
        let x_val = x_min + frac * x_range;
        primitives.push(ChartPrimitive::Label {
            text: format_number(x_val),
            x,
            y: plot_bottom + AXIS_LABEL_FONT + LABEL_MARGIN,
            font_size: AXIS_LABEL_FONT,
            color: LABEL_COLOR,
            anchor: TextAnchor::Center,
        });
    }

    // Axes
    primitives.push(ChartPrimitive::Line {
        x1: plot_left,
        y1: plot_top,
        x2: plot_left,
        y2: plot_bottom,
        stroke: AXIS_COLOR,
        width: 1.0,
    });
    primitives.push(ChartPrimitive::Line {
        x1: plot_left,
        y1: plot_bottom,
        x2: plot_right,
        y2: plot_bottom,
        stroke: AXIS_COLOR,
        width: 1.0,
    });

    // Dots — slight offset for overlapping groups
    let n_groups = groups.len() as f64;
    for (gi, group) in groups.iter().enumerate() {
        let color = resolve_color(group.color.as_deref(), gi);
        let offset = if n_groups > 1.0 {
            (gi as f64 - (n_groups - 1.0) / 2.0) * config.dot_size * 0.4
        } else {
            0.0
        };

        for &(dx, dy) in &group.data {
            let px = plot_left + ((dx - x_min) / x_range) * plot_width + offset;
            let py = plot_bottom - ((dy - y_min) / y_range) * plot_height;
            primitives.push(ChartPrimitive::Circle {
                cx: px,
                cy: py,
                r: config.dot_size,
                fill: color,
            });
        }
    }

    // Axis titles
    if let Some(ref label) = config.y_label {
        // `yLabel` was accepted and documented but never drawn.
        primitives.push(ChartPrimitive::VerticalLabel {
            text: label.clone(),
            x: AXIS_LABEL_FONT / 2.0 + 1.0,
            y: plot_top + plot_height / 2.0,
            font_size: AXIS_LABEL_FONT,
            color: LABEL_COLOR,
        });
    }
    if let Some(ref label) = config.x_label {
        primitives.push(ChartPrimitive::Label {
            text: label.clone(),
            x: plot_left + plot_width / 2.0,
            y: height - 2.0,
            font_size: AXIS_LABEL_FONT,
            color: LABEL_COLOR,
            anchor: TextAnchor::Center,
        });
    }

    // Legend
    if config.show_legend {
        let legend_x = plot_right + LABEL_MARGIN;
        let legend_y_start = plot_top + LABEL_MARGIN;
        let swatch_size = 8.0;
        let line_height = 14.0;

        for (i, group) in groups.iter().enumerate() {
            let ly = legend_y_start + i as f64 * line_height;
            let color = resolve_color(group.color.as_deref(), i);

            primitives.push(ChartPrimitive::Circle {
                cx: legend_x + swatch_size / 2.0,
                cy: ly + swatch_size / 2.0,
                r: swatch_size / 2.0,
                fill: color,
            });
            primitives.push(ChartPrimitive::Label {
                text: group.name.clone(),
                x: legend_x + swatch_size + LABEL_MARGIN,
                y: ly + swatch_size - 1.0,
                font_size: AXIS_LABEL_FONT,
                color: LABEL_COLOR,
                anchor: TextAnchor::Left,
            });
        }
    }

    primitives
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groups() -> Vec<DotPlotGroup> {
        vec![DotPlotGroup {
            name: "p50".to_string(),
            color: None,
            data: vec![(1.0, 120.0), (2.0, 340.0), (3.0, 910.0)],
        }]
    }

    fn config(x_label: Option<&str>, y_label: Option<&str>) -> DotPlotConfig {
        DotPlotConfig {
            x_min: None,
            x_max: None,
            y_min: None,
            y_max: None,
            x_label: x_label.map(str::to_string),
            y_label: y_label.map(str::to_string),
            show_legend: false,
            dot_size: 3.0,
        }
    }

    /// Left edges of the right-anchored y tick labels.
    fn tick_label_left_edges(p: &[ChartPrimitive]) -> Vec<f64> {
        p.iter()
            .filter_map(|p| match p {
                ChartPrimitive::Label {
                    text,
                    x,
                    font_size,
                    anchor: TextAnchor::Right,
                    ..
                } => Some(x - measure_label(text, *font_size)),
                _ => None,
            })
            .collect()
    }

    /// `yLabel` was accepted, documented ("Latency (ms)") and never drawn.
    #[test]
    fn y_label_is_drawn_vertically_clear_of_the_tick_labels() {
        let p = build(300.0, 200.0, &groups(), &config(None, Some("Latency (ms)")));
        let title = p
            .iter()
            .find_map(|p| match p {
                ChartPrimitive::VerticalLabel {
                    text, x, font_size, ..
                } => Some((text.clone(), *x, *font_size)),
                _ => None,
            })
            .expect("the y-axis title is drawn");
        assert_eq!(title.0, "Latency (ms)");
        let title_right = title.1 + title.2 / 2.0;
        let ticks_left = tick_label_left_edges(&p)
            .into_iter()
            .fold(f64::INFINITY, f64::min);
        assert!(
            title_right < ticks_left,
            "title (right edge {title_right:.1}) must clear the tick labels (from {ticks_left:.1})"
        );
    }

    /// `xLabel` sat 6pt below the tick labels inside the same 20pt band, so
    /// the two touched. With a title the plot keeps a line of room for it.
    #[test]
    fn x_label_has_its_own_line_below_the_tick_labels() {
        let p = build(300.0, 200.0, &groups(), &config(Some("Run"), None));
        let baseline = |want_center: bool, text_is_title: bool| -> f64 {
            p.iter()
                .filter_map(|p| match p {
                    ChartPrimitive::Label {
                        text,
                        y,
                        anchor: TextAnchor::Center,
                        ..
                    } if want_center && (text == "Run") == text_is_title => Some(*y),
                    _ => None,
                })
                .fold(f64::MIN, f64::max)
        };
        let ticks = baseline(true, false);
        let title = baseline(true, true);
        // The title's cap height (~0.72em) must start below the tick
        // labels' descenders (~0.21em).
        let gap = (title - AXIS_LABEL_FONT * 0.72) - (ticks + AXIS_LABEL_FONT * 0.21);
        assert!(gap >= 2.0, "x title crowds the tick labels: gap {gap:.2}pt");
    }

    #[test]
    fn no_titles_leaves_the_plot_where_it_was() {
        let p = build(300.0, 200.0, &groups(), &config(None, None));
        assert!(!p
            .iter()
            .any(|p| matches!(p, ChartPrimitive::VerticalLabel { .. })));
        // The y axis still starts at Y_AXIS_WIDTH.
        let axis_x = p
            .iter()
            .find_map(|p| match p {
                ChartPrimitive::Line { x1, x2, width, .. }
                    if (x1 - x2).abs() < 1e-9 && *width == 1.0 =>
                {
                    Some(*x1)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(axis_x, Y_AXIS_WIDTH);
    }
}
