use ratatui::style::{Color, Modifier, Style};

pub(crate) const THEME: Theme = Theme::cadder();

#[derive(Debug, Clone, Copy)]
pub(crate) struct Theme {
  colors: Colors,
  tabs: TabAccents,
}

#[derive(Debug, Clone, Copy)]
struct Colors {
  backdrop: Color,
  surface: Color,
  surface_elevated: Color,
  surface_selected: Color,
  border: Color,
  border_strong: Color,
  text: Color,
  text_muted: Color,
  text_soft: Color,
  mint: Color,
  mint_soft: Color,
  dim: Color,
}

#[derive(Debug, Clone, Copy)]
struct TabAccents {
  domains: Color,
  settings: Color,
}

impl Theme {
  pub const fn cadder() -> Self {
    let colors = Colors {
      backdrop: Color::Rgb(4, 15, 14),
      surface: Color::Rgb(7, 24, 22),
      surface_elevated: Color::Rgb(10, 33, 30),
      surface_selected: Color::Rgb(12, 55, 49),
      border: Color::Rgb(26, 102, 91),
      border_strong: Color::Rgb(49, 226, 198),
      text: Color::Rgb(223, 255, 249),
      text_muted: Color::Rgb(139, 190, 181),
      text_soft: Color::Rgb(187, 231, 224),
      mint: Color::Rgb(58, 232, 203),
      mint_soft: Color::Rgb(125, 244, 221),
      dim: Color::Rgb(45, 85, 80),
    };

    Self {
      tabs: TabAccents {
        domains: colors.mint,
        settings: colors.mint,
      },
      colors,
    }
  }

  pub const fn domains_accent(self) -> Color {
    self.tabs.domains
  }

  pub const fn settings_accent(self) -> Color {
    self.tabs.settings
  }

  pub fn inactive_tab(self) -> Style {
    Style::new().fg(self.colors.text_muted)
  }

  pub fn active_tab(self) -> Style {
    Style::new()
      .fg(self.colors.mint_soft)
      .bg(self.colors.surface_selected)
      .add_modifier(Modifier::BOLD)
  }

  pub fn shortcut_key(self) -> Style {
    Style::new()
      .fg(self.colors.mint_soft)
      .add_modifier(Modifier::BOLD)
  }

  pub fn shortcut_description(self) -> Style {
    Style::new().fg(self.colors.text_muted)
  }

  pub fn footer(self) -> Style {
    Style::new().bg(self.colors.backdrop)
  }

  pub fn header(self) -> Style {
    Style::new().bg(self.colors.backdrop)
  }

  pub fn header_title(self) -> Style {
    Style::new()
      .fg(self.colors.text)
      .bg(self.colors.backdrop)
      .add_modifier(Modifier::BOLD)
  }

  pub fn service_online(self) -> Style {
    Style::new()
      .fg(self.colors.mint_soft)
      .bg(self.colors.backdrop)
      .add_modifier(Modifier::BOLD)
  }

  pub fn service_offline(self) -> Style {
    Style::new()
      .fg(self.colors.dim)
      .bg(self.colors.backdrop)
      .add_modifier(Modifier::DIM)
  }

  pub fn service_separator(self) -> Style {
    Style::new()
      .fg(self.colors.text_muted)
      .bg(self.colors.backdrop)
  }

  pub fn dim_background(self) -> Style {
    Style::new()
      .fg(self.colors.dim)
      .bg(self.colors.backdrop)
      .add_modifier(Modifier::DIM)
  }

  pub fn table(self) -> Style {
    Style::new().bg(self.colors.surface)
  }

  pub fn table_border(self) -> Style {
    Style::new().fg(self.colors.border)
  }

  pub fn table_row(self) -> Style {
    Style::new().fg(self.colors.text_soft)
  }

  pub fn table_disabled_row(self) -> Style {
    Style::new().fg(self.colors.text_muted)
  }

  pub fn table_header(self) -> Style {
    Style::new()
      .fg(self.colors.mint_soft)
      .bg(self.colors.surface_selected)
      .add_modifier(Modifier::BOLD)
  }

  pub fn table_highlight(self, _accent: Color) -> Style {
    Style::new().add_modifier(Modifier::BOLD)
  }

  pub fn selected_marker(self) -> Style {
    Style::new()
      .fg(self.colors.mint_soft)
      .add_modifier(Modifier::BOLD)
  }

  pub fn disabled_marker(self) -> Style {
    Style::new().fg(self.colors.text_muted)
  }

  pub fn overlay(self) -> Style {
    Style::new().bg(self.colors.surface_elevated)
  }

  pub fn overlay_border(self) -> Style {
    Style::new().fg(self.colors.border_strong)
  }

  pub fn overlay_title(self) -> Style {
    Style::new()
      .fg(self.colors.mint_soft)
      .add_modifier(Modifier::BOLD)
  }

  pub fn text(self) -> Style {
    Style::new().fg(self.colors.text)
  }

  pub fn accent_text(self) -> Style {
    Style::new()
      .fg(self.colors.mint_soft)
      .add_modifier(Modifier::BOLD)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn theme_exposes_consistent_semantic_styles() {
    let theme = Theme::cadder();
    assert_eq!(theme.domains_accent(), theme.settings_accent());
    assert_eq!(theme.inactive_tab().fg, Some(theme.colors.text_muted));
    assert_eq!(theme.active_tab().fg, Some(theme.colors.mint_soft));
    assert_eq!(theme.active_tab().bg, Some(theme.colors.surface_selected));
    assert_eq!(theme.shortcut_key().fg, Some(theme.colors.mint_soft));
    assert_eq!(
      theme.shortcut_description().fg,
      Some(theme.colors.text_muted)
    );
    assert_eq!(theme.footer().bg, Some(theme.colors.backdrop));
    assert_eq!(theme.header().bg, Some(theme.colors.backdrop));
    assert_eq!(theme.header_title().fg, Some(theme.colors.text));
    assert_eq!(theme.service_online().fg, Some(theme.colors.mint_soft));
    assert_eq!(theme.service_offline().fg, Some(theme.colors.dim));
    assert_eq!(theme.service_separator().fg, Some(theme.colors.text_muted));
    assert_eq!(theme.dim_background().fg, Some(theme.colors.dim));
    assert_eq!(theme.table().bg, Some(theme.colors.surface));
    assert_eq!(theme.table_border().fg, Some(theme.colors.border));
    assert_eq!(theme.table_row().fg, Some(theme.colors.text_soft));
    assert_eq!(theme.table_disabled_row().fg, Some(theme.colors.text_muted));
    assert_eq!(theme.table_header().fg, Some(theme.colors.mint_soft));
    assert!(
      theme
        .table_highlight(Color::Red)
        .add_modifier
        .contains(Modifier::BOLD)
    );
    assert_eq!(theme.selected_marker().fg, Some(theme.colors.mint_soft));
    assert_eq!(theme.disabled_marker().fg, Some(theme.colors.text_muted));
    assert_eq!(theme.overlay().bg, Some(theme.colors.surface_elevated));
    assert_eq!(theme.overlay_border().fg, Some(theme.colors.border_strong));
    assert_eq!(theme.overlay_title().fg, Some(theme.colors.mint_soft));
    assert_eq!(theme.text().fg, Some(theme.colors.text));
    assert_eq!(theme.accent_text().fg, Some(theme.colors.mint_soft));
  }
}
