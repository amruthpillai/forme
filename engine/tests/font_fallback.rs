use forme::font::{fallback::segment_by_font, FontContext, FontRegistry};
use forme::style::{Color, FontStyle, Hyphens, TextDecoration};
use forme::text::{StyledChar, TextLayout};

#[test]
fn registered_font_covers_supplementary_characters() {
    let mut registry = FontRegistry::new();
    registry.register(
        "Emoji",
        400,
        false,
        include_bytes!("fixtures/fonts/NotoEmoji-Regular.ttf").to_vec(),
    );
    let font = registry.resolve("Emoji", 400, false);
    for ch in ['💻', '🚀', '😀'] {
        assert!(font.has_char(ch), "registered font lost {ch}");
        assert_eq!(
            registry
                .resolve_for_char("Helvetica, Emoji", ch, 400, false)
                .1,
            "Emoji"
        );
    }
}

#[test]
fn fallback_keeps_shaping_controls_with_the_base_font() {
    let registry = FontRegistry::new();
    for text in ["П\u{200d}П\u{fe0f}", "П\u{200c}П\u{e0100}"] {
        let chars: Vec<_> = text.chars().collect();
        let runs = segment_by_font(&chars, "Helvetica, Noto Sans", 400, false, &registry);
        assert_eq!(runs.len(), 1, "split shaping cluster in {text:?}");
        assert_eq!(runs[0].family, "Noto Sans");
        assert_eq!(runs[0].end, chars.len());
    }
}

#[test]
fn styled_fallback_uses_shaped_widths_for_line_breaking() {
    let mut context = FontContext::new();
    context.registry_mut().register(
        "Devanagari",
        400,
        false,
        include_bytes!("fixtures/fonts/NotoSansDevanagari-Regular.ttf").to_vec(),
    );
    let layout = TextLayout::new();
    let measure = |family: &str, width: f64| {
        let chars: Vec<_> = "नमस्तेनमस्ते"
            .chars()
            .map(|ch| StyledChar {
                ch,
                font_family: family.to_string(),
                font_size: 14.0,
                font_weight: 400,
                font_style: FontStyle::Normal,
                color: Color::BLACK,
                href: None,
                text_decoration: TextDecoration::None,
                letter_spacing: 0.0,
                word_spacing: 0.0,
            })
            .collect();
        layout.break_runs_into_lines(&context, &chars, width, Hyphens::None, None)
    };
    let direct = measure("Devanagari", 1000.0);
    let fallback = measure("Noto Sans, Devanagari", 1000.0);
    assert!(
        (direct[0].width - fallback[0].width).abs() < 0.001,
        "same font measured differently: {} vs {}",
        direct[0].width,
        fallback[0].width
    );
    assert_eq!(
        measure("Noto Sans, Devanagari", direct[0].width + 0.001).len(),
        1
    );
}
