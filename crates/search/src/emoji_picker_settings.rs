use settings::{RegisterSetting, Settings};

#[derive(Clone, RegisterSetting)]
pub struct EmojiPickerSettings {
    pub emoji_picker: Vec<String>,
}

impl Settings for EmojiPickerSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        Self {
            emoji_picker: content.emoji_picker.clone().unwrap_or_else(|| {
                vec![
                    "😄 smile".to_string(),
                    "😭 sad".to_string(),
                    "🤔 thinking".to_string(),
                ]
            }),
        }
    }
}
