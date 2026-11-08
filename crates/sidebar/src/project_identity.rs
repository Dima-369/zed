use gpui::{Hsla, Rgba};
use project::ProjectGroupKey;
use std::collections::HashMap;

const HUE_SLOTS: [f32; 12] = [
    25., 55., 85., 115., 145., 175., 205., 235., 265., 295., 325., 355.,
];
// Coprime with the slot count, so probing visits every slot.
const HUE_PROBE_STEP: usize = 5;
const FALLBACK_HUE: f32 = HUE_SLOTS[8];

/// Solid colors for one project group. They are defined in OKLCH rather than
/// taken from the theme's accents because the accents cluster (amber/gold/orange,
/// blue/indigo/iris), which would make neighboring projects hard to tell apart.
#[derive(Clone, Copy)]
pub(crate) struct ProjectColors {
    pub tile_background: Hsla,
    pub tile_foreground: Hsla,
    pub rail: Hsla,
    pub name_accent: Hsla,
    pub group_background: Hsla,
    pub row_hover: Hsla,
    pub row_selected: Hsla,
}

impl ProjectColors {
    pub fn new(hue: f32, is_active: bool, is_light_theme: bool) -> Self {
        let color = |lightness, chroma| oklch(lightness, chroma, hue);
        match (is_light_theme, is_active) {
            (false, false) => Self {
                tile_background: color(0.43, 0.10),
                tile_foreground: color(0.93, 0.07),
                rail: color(0.52, 0.09),
                name_accent: color(0.88, 0.12),
                group_background: color(0.325, 0.012),
                row_hover: color(0.37, 0.02),
                row_selected: color(0.47, 0.08),
            },
            (false, true) => Self {
                tile_background: color(0.82, 0.14),
                tile_foreground: color(0.24, 0.06),
                rail: color(0.74, 0.13),
                name_accent: color(0.88, 0.12),
                group_background: color(0.36, 0.05),
                row_hover: color(0.41, 0.06),
                row_selected: color(0.47, 0.08),
            },
            (true, false) => Self {
                tile_background: color(0.86, 0.07),
                tile_foreground: color(0.36, 0.10),
                rail: color(0.72, 0.09),
                name_accent: color(0.45, 0.14),
                group_background: color(0.95, 0.008),
                row_hover: color(0.91, 0.02),
                row_selected: color(0.84, 0.08),
            },
            (true, true) => Self {
                tile_background: color(0.52, 0.15),
                tile_foreground: color(0.98, 0.02),
                rail: color(0.55, 0.14),
                name_accent: color(0.45, 0.14),
                group_background: color(0.92, 0.04),
                row_hover: color(0.88, 0.06),
                row_selected: color(0.84, 0.08),
            },
        }
    }

    pub fn for_key(
        hues: &HashMap<ProjectGroupKey, f32>,
        key: &ProjectGroupKey,
        is_active: bool,
        is_light_theme: bool,
    ) -> Self {
        let hue = hues.get(key).copied().unwrap_or(FALLBACK_HUE);
        Self::new(hue, is_active, is_light_theme)
    }
}

/// Hashes folder names rather than full paths so a project keeps its color when
/// it moves on disk. Keys are visited in sorted order so that assignments don't
/// depend on the order of the groups in the sidebar.
pub(crate) fn assign_project_hues<'a>(
    keys: impl Iterator<Item = &'a ProjectGroupKey>,
) -> HashMap<ProjectGroupKey, f32> {
    let mut named_keys: Vec<(String, String, &ProjectGroupKey)> = keys
        .map(|key| {
            let paths = key.path_list().paths();
            let folder_names = paths
                .iter()
                .map(|path| {
                    path.file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy()
                })
                .collect::<Vec<_>>()
                .join(", ");
            let full_paths = paths
                .iter()
                .map(|path| path.to_string_lossy())
                .collect::<Vec<_>>()
                .join(", ");
            (folder_names, full_paths, key)
        })
        .collect();
    named_keys.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));

    let names: Vec<&str> = named_keys
        .iter()
        .map(|(name, _, _)| name.as_str())
        .collect();
    named_keys
        .iter()
        .zip(assign_hues_to_sorted_names(&names))
        .map(|((_, _, key), hue)| ((*key).clone(), hue))
        .collect()
}

fn assign_hues_to_sorted_names(names: &[&str]) -> Vec<f32> {
    let mut taken = [false; HUE_SLOTS.len()];
    names
        .iter()
        .map(|name| {
            let mut slot = fnv1a(name) as usize % HUE_SLOTS.len();
            if taken.contains(&false) {
                while taken[slot] {
                    slot = (slot + HUE_PROBE_STEP) % HUE_SLOTS.len();
                }
            }
            taken[slot] = true;
            HUE_SLOTS[slot]
        })
        .collect()
}

// `DefaultHasher` isn't guaranteed to be stable across Rust releases, and a
// project's color has to survive upgrades.
fn fnv1a(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ byte as u32).wrapping_mul(0x0100_0193)
    })
}

/// Two uppercase letters identifying a project: the initials of its first two
/// words, or the first two letters when the name is a single word.
pub(crate) fn project_monogram(name: &str) -> String {
    let words = project_name_word_starts(name);
    let monogram: String = match words.as_slice() {
        [first, second, ..] => [*first, *second].into_iter().collect(),
        [_] | [] => name
            .chars()
            .filter(|character| character.is_alphanumeric())
            .take(2)
            .collect(),
    };
    monogram.to_uppercase()
}

fn project_name_word_starts(name: &str) -> Vec<char> {
    let mut word_starts = Vec::new();
    let mut previous: Option<char> = None;
    for character in name.chars() {
        let starts_word = match previous {
            None => true,
            Some(previous) => {
                !previous.is_alphanumeric() || (previous.is_lowercase() && character.is_uppercase())
            }
        };
        if starts_word && character.is_alphabetic() {
            word_starts.push(character);
        }
        previous = Some(character);
    }
    word_starts
}

/// Byte length of the leading word of a project name, which renders brighter
/// than the rest. Names that don't begin with a lowercase-style word, such as
/// `RUST` or `.config`, are treated as all head.
pub(crate) fn project_name_head_len(name: &str) -> usize {
    let mut characters = name.char_indices().peekable();
    characters.next_if(|(_, character)| character.is_uppercase());
    let word_start = characters.peek().map(|(index, _)| *index);
    while characters
        .next_if(|(_, character)| character.is_lowercase() || character.is_numeric())
        .is_some()
    {}
    match (word_start, characters.peek()) {
        (Some(start), Some((end, _))) if *end > start => *end,
        _ => name.len(),
    }
}

/// Splits a leading status symbol, such as the `✳` that terminal programs put in
/// their titles, from the rest of the title.
pub(crate) fn split_leading_glyph(title: &str) -> Option<(&str, &str)> {
    let first = title.chars().next()?;
    if first.is_ascii() || first.is_alphanumeric() {
        return None;
    }
    let (glyph, rest) = title.split_at(first.len_utf8());
    let trimmed = rest.trim_start();
    (trimmed.len() < rest.len() && !trimmed.is_empty()).then_some((glyph, trimmed))
}

fn oklch(lightness: f32, chroma: f32, hue_degrees: f32) -> Hsla {
    let hue = hue_degrees.to_radians();
    let a = chroma * hue.cos();
    let b = chroma * hue.sin();

    let long = (lightness + 0.39633778 * a + 0.21580376 * b).powi(3);
    let medium = (lightness - 0.105561346 * a - 0.06385417 * b).powi(3);
    let short = (lightness - 0.08948418 * a - 1.2914855 * b).powi(3);

    Rgba {
        r: srgb_encode(4.0767417 * long - 3.3077116 * medium + 0.23096993 * short),
        g: srgb_encode(-1.268438 * long + 2.6097574 * medium - 0.34131938 * short),
        b: srgb_encode(-0.0041960863 * long - 0.7034186 * medium + 1.7076147 * short),
        a: 1.0,
    }
    .into()
}

fn srgb_encode(linear: f32) -> f32 {
    let linear = linear.clamp(0.0, 1.0);
    if linear <= 0.0031308 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monogram_uses_the_first_two_words() {
        assert_eq!(project_monogram("natuerlich-schlanker-ebook"), "NS");
        assert_eq!(project_monogram("AndroidInstagramReels"), "AI");
        assert_eq!(project_monogram("liberty-144-llc-business"), "LL");
        assert_eq!(project_monogram("foo_bar baz"), "FB");
    }

    #[test]
    fn monogram_falls_back_to_the_first_letters() {
        assert_eq!(project_monogram("dotfiles"), "DO");
        assert_eq!(project_monogram("144"), "14");
        assert_eq!(project_monogram(""), "");
    }

    #[test]
    fn head_is_the_first_word() {
        let head = |name: &'static str| &name[..project_name_head_len(name)];
        assert_eq!(head("rust-tradebot"), "rust");
        assert_eq!(head("AndroidInstagramReels"), "Android");
        assert_eq!(head("dotfiles"), "dotfiles");
        assert_eq!(head("über-projekt"), "über");
    }

    #[test]
    fn names_without_a_leading_word_are_all_head() {
        for name in ["RUST", ".config", "", "A"] {
            assert_eq!(project_name_head_len(name), name.len());
        }
    }

    #[test]
    fn leading_glyph_is_split_from_the_title() {
        assert_eq!(
            split_leading_glyph("✳ Alicia Butemann.ty"),
            Some(("✳", "Alicia Butemann.ty"))
        );
        assert_eq!(split_leading_glyph("◑  two"), Some(("◑", "two")));
    }

    #[test]
    fn ordinary_titles_keep_their_first_character() {
        for title in [
            "zsh",
            "~ /tmp",
            "Ünder way",
            "日本 語",
            "✳",
            "✳ ",
            "✳no-space",
            "",
        ] {
            assert_eq!(split_leading_glyph(title), None, "{title:?}");
        }
    }

    #[test]
    fn hues_are_distinct_until_the_slots_run_out() {
        let names: Vec<String> = (0..12).map(|index| format!("project-{index}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut hues = assign_hues_to_sorted_names(&names);
        hues.sort_by(f32::total_cmp);
        assert_eq!(hues, HUE_SLOTS);
    }

    #[test]
    fn more_projects_than_slots_do_not_hang() {
        let names: Vec<String> = (0..40).map(|index| format!("project-{index}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        assert_eq!(assign_hues_to_sorted_names(&names).len(), 40);
    }

    #[test]
    fn oklch_grays_are_neutral() {
        let gray = Rgba::from(oklch(0.5, 0.0, 0.0));
        assert!((gray.r - gray.g).abs() < 0.01 && (gray.g - gray.b).abs() < 0.01);
    }
}
