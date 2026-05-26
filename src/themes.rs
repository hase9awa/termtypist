use std::fs;
use std::path::Path;

use anyhow::Context;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub background: String,
    pub text: String,
    pub muted: String,
    pub main: String,
    pub error: String,
    pub warning: String,
    pub success: String,
    pub caret: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedTheme {
    pub name: String,
    pub background: Color,
    pub text: Color,
    pub muted: Color,
    pub main: Color,
    pub error: Color,
    pub warning: Color,
    pub caret: Color,
}

impl Theme {
    pub fn named(name: &str) -> Option<Self> {
        Self::available()
            .into_iter()
            .find(|theme| theme.name.eq_ignore_ascii_case(name))
    }

    pub fn available() -> Vec<Self> {
        merge_by_name(Self::builtins(), load_user_themes())
    }

    pub fn builtins() -> Vec<Self> {
        vec![
            Self::new("terminal", ["", "", "", "", "", "", "", ""]),
            Self::new(
                "default",
                [
                    "#1f2328", "#d6d6d6", "#6b7280", "#e2b714", "#ca4754", "#e2b714", "#7fb069",
                    "#e2b714",
                ],
            ),
            Self::new(
                "dark",
                [
                    "#2c2f33", "#d1d0c5", "#646669", "#e2b714", "#ca4754", "#e2b714", "#7fb069",
                    "#e2b714",
                ],
            ),
            Self::new(
                "light",
                [
                    "#f2f2f2", "#222222", "#8a8a8a", "#d9a400", "#b3261e", "#a06d00", "#2e7d32",
                    "#d9a400",
                ],
            ),
            Self::new(
                "dracula",
                [
                    "#282a36", "#f8f8f2", "#6272a4", "#bd93f9", "#ff5555", "#f1fa8c", "#50fa7b",
                    "#f1fa8c",
                ],
            ),
            Self::new(
                "catppuccin",
                [
                    "#1e1e2e", "#cdd6f4", "#6c7086", "#89b4fa", "#f38ba8", "#f9e2af", "#a6e3a1",
                    "#f9e2af",
                ],
            ),
            Self::new(
                "gruvbox",
                [
                    "#282828", "#ebdbb2", "#928374", "#fabd2f", "#fb4934", "#fe8019", "#b8bb26",
                    "#fabd2f",
                ],
            ),
            Self::new(
                "kanagawa",
                [
                    "#1f1f28", "#dcd7ba", "#727169", "#c8c093", "#c34043", "#ffa066", "#76946a",
                    "#c8c093",
                ],
            ),
            Self::new(
                "synthwave",
                [
                    "#1b1026", "#f6e8ff", "#8f7ca8", "#ff7edb", "#ff4f8b", "#f6c177", "#69f0ae",
                    "#36f9f6",
                ],
            ),
            Self::new(
                "matrix",
                [
                    "#07130b", "#d7ffe1", "#246b3a", "#00ff66", "#ff5f57", "#b8ff5c", "#3cff8f",
                    "#8cffb1",
                ],
            ),
            Self::new(
                "paper",
                [
                    "#f7f3e8", "#2d2926", "#8b8175", "#2f6f7e", "#9d2f2f", "#a45f13", "#3f7d4c",
                    "#2f6f7e",
                ],
            ),
            Self::new(
                "orbit",
                [
                    "#10121a", "#e8edf8", "#6f7b96", "#7dd3fc", "#fb7185", "#facc15", "#86efac",
                    "#c4b5fd",
                ],
            ),
        ]
    }

    pub fn write_default_files() -> anyhow::Result<()> {
        let dir = crate::config::Config::themes_dir()?;
        fs::create_dir_all(&dir)?;
        for theme in Self::builtins() {
            let path = dir.join(format!("{}.toml", theme.name));
            if path.exists() {
                continue;
            }
            fs::write(&path, toml::to_string_pretty(&theme)?)
                .with_context(|| format!("failed to write theme to {}", path.display()))?;
        }
        Ok(())
    }

    fn new(name: &str, colors: [&str; 8]) -> Self {
        Self {
            name: name.to_string(),
            background: colors[0].to_string(),
            text: colors[1].to_string(),
            muted: colors[2].to_string(),
            main: colors[3].to_string(),
            error: colors[4].to_string(),
            warning: colors[5].to_string(),
            success: colors[6].to_string(),
            caret: colors[7].to_string(),
        }
    }

    pub fn resolve(&self) -> ResolvedTheme {
        if self.name == "terminal" {
            return ResolvedTheme {
                name: self.name.clone(),
                background: Color::Reset,
                text: Color::Reset,
                muted: Color::DarkGray,
                main: Color::Yellow,
                error: Color::Red,
                warning: Color::Yellow,
                caret: Color::Yellow,
            };
        }

        ResolvedTheme {
            name: self.name.clone(),
            background: parse_hex(&self.background).unwrap_or(Color::Black),
            text: parse_hex(&self.text).unwrap_or(Color::White),
            muted: parse_hex(&self.muted).unwrap_or(Color::DarkGray),
            main: parse_hex(&self.main).unwrap_or(Color::Yellow),
            error: parse_hex(&self.error).unwrap_or(Color::Red),
            warning: parse_hex(&self.warning).unwrap_or(Color::Yellow),
            caret: parse_hex(&self.caret).unwrap_or(Color::Yellow),
        }
    }
}

fn load_user_themes() -> Vec<Theme> {
    let Ok(dir) = crate::config::Config::themes_dir() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| load_theme_file(&entry.path()).ok())
        .filter(is_valid_theme)
        .collect()
}

fn load_theme_file(path: &Path) -> anyhow::Result<Theme> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read theme at {}", path.display()))?;
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => serde_json::from_str(&raw)
            .with_context(|| format!("invalid theme json at {}", path.display())),
        Some("toml") => toml::from_str(&raw)
            .with_context(|| format!("invalid theme toml at {}", path.display())),
        _ => anyhow::bail!("unsupported theme format at {}", path.display()),
    }
}

fn is_valid_theme(theme: &Theme) -> bool {
    if theme.name.trim().is_empty() {
        return false;
    }
    if theme.name == "terminal" {
        return true;
    }
    [
        &theme.background,
        &theme.text,
        &theme.muted,
        &theme.main,
        &theme.error,
        &theme.warning,
        &theme.success,
        &theme.caret,
    ]
    .iter()
    .all(|color| parse_hex(color).is_some())
}

fn merge_by_name(mut base: Vec<Theme>, custom: Vec<Theme>) -> Vec<Theme> {
    for theme in custom {
        if let Some(existing) = base
            .iter_mut()
            .find(|item| item.name.eq_ignore_ascii_case(&theme.name))
        {
            *existing = theme;
        } else {
            base.push(theme);
        }
    }
    base
}

fn parse_hex(raw: &str) -> Option<Color> {
    let hex = raw.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::Theme;

    #[test]
    fn resolves_builtin_theme() {
        let theme = Theme::named("catppuccin").unwrap().resolve();
        assert_eq!(theme.name, "catppuccin");
    }
}
