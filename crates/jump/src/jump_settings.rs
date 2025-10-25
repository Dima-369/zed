use settings::Settings;

#[derive(Debug, Clone)]
pub struct JumpSettings {
    pub autojump: bool,
}

impl Settings for JumpSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let jump = content.jump.as_ref();
        JumpSettings {
            autojump: jump.and_then(|j| j.autojump).unwrap_or(false),
        }
    }
}
