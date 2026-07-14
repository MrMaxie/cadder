use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Margin, Rect};
use ratatui::style::Color;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
  Block, Borders, Cell, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
  Table, TableState, Widget,
};

use crate::data::{DomainRowKind, DomainTableRow, SettingsTableRow};
use crate::widgets::theme::THEME;

pub(crate) enum TableBodyRows {
  Domains(Vec<DomainTableRow>),
  Settings(Vec<SettingsTableRow>),
}

pub(crate) struct TableBody {
  accent: Color,
  rows: TableBodyRows,
}

impl TableBody {
  pub const fn new(accent: Color, rows: TableBodyRows) -> Self {
    Self { accent, rows }
  }
}

impl StatefulWidget for TableBody {
  type State = TableState;

  fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
    let block = Block::new()
      .borders(Borders::ALL)
      .border_style(THEME.table_border())
      .style(THEME.table());
    let inner = block.inner(area);
    block.render(area, buf);

    let row_count = self.rows.len();
    let has_header = self.rows.has_header();
    let reserved_header = if has_header { 2 } else { 0 };
    let viewport = usize::from(inner.height.saturating_sub(reserved_header)).max(1);
    apply_scroll_margin(state, row_count, viewport);

    let header = has_header.then(|| Row::new(self.rows.headers()).style(THEME.table_header()));
    let table = Table::new(
      self.rows.into_rows(),
      [
        Constraint::Length(5),
        Constraint::Min(20),
        Constraint::Length(14),
      ],
    )
    .column_spacing(1)
    .row_highlight_style(THEME.table_highlight(self.accent))
    .highlight_symbol("> ");
    let table = if let Some(header) = header {
      table.header(header.bottom_margin(1))
    } else {
      table
    };

    StatefulWidget::render(table, inner, buf, state);

    if row_count > viewport {
      let mut scrollbar_state = ScrollbarState::new(row_count)
        .viewport_content_length(viewport)
        .position(state.offset());
      StatefulWidget::render(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        inner.inner(Margin {
          vertical: if has_header { 1 } else { 0 },
          horizontal: 0,
        }),
        buf,
        &mut scrollbar_state,
      );
    }
  }
}

impl TableBodyRows {
  fn len(&self) -> usize {
    match self {
      Self::Domains(rows) => rows.len(),
      Self::Settings(rows) => rows.len(),
    }
  }

  const fn has_header(&self) -> bool {
    true
  }

  fn headers(&self) -> [&'static str; 3] {
    match self {
      Self::Domains(_) => ["", "Entrypoint / domain", "Upstream"],
      Self::Settings(_) => ["", "Component", "State"],
    }
  }

  fn into_rows(self) -> Vec<Row<'static>> {
    match self {
      Self::Domains(rows) => rows.into_iter().map(domain_row).collect(),
      Self::Settings(rows) => rows.into_iter().map(settings_row).collect(),
    }
  }
}

fn domain_row(row: DomainTableRow) -> Row<'static> {
  let visually_enabled = row.visually_enabled();
  let row_style = row_style(visually_enabled);
  let mut table_row = match row.kind() {
    DomainRowKind::Entrypoint => Row::new([
      Cell::from(checkbox(row.enabled(), visually_enabled)),
      Cell::from(Line::from(row.name().to_string()).style(project_name_style(visually_enabled))),
      Cell::from(right_aligned(format!(
        "{} domains",
        row.count().unwrap_or_default()
      ))),
    ])
    .style(row_style),
    DomainRowKind::Domain => Row::new([
      Cell::from(checkbox(row.enabled(), visually_enabled)),
      Cell::from(format!("  {}", row.name())),
      Cell::from(right_aligned(row.endpoint().to_string())),
    ])
    .style(row_style),
  };

  if row.spaced_before() {
    table_row = table_row.top_margin(1);
  }

  table_row
}

fn settings_row(row: SettingsTableRow) -> Row<'static> {
  Row::new([
    Cell::from(""),
    Cell::from(row.name().to_string()),
    Cell::from(right_aligned(row.value().to_string())),
  ])
  .style(THEME.table_row())
}

fn checkbox(checked: bool, visually_enabled: bool) -> Span<'static> {
  let marker = if checked { "[x]" } else { "[ ]" };
  Span::styled(marker, checkbox_style(visually_enabled))
}

fn row_style(enabled: bool) -> ratatui::style::Style {
  if enabled {
    THEME.table_row()
  } else {
    THEME.table_disabled_row()
  }
}

fn checkbox_style(enabled: bool) -> ratatui::style::Style {
  if enabled {
    THEME.selected_marker()
  } else {
    THEME.disabled_marker()
  }
}

fn project_name_style(visual_enabled: bool) -> ratatui::style::Style {
  if visual_enabled {
    THEME.accent_text()
  } else {
    THEME.table_disabled_row()
  }
}

fn right_aligned(value: String) -> Line<'static> {
  Line::from(value).alignment(Alignment::Right)
}

fn apply_scroll_margin(state: &mut TableState, len: usize, viewport: usize) {
  let Some(selected) = state.selected() else {
    return;
  };
  if len <= viewport {
    *state.offset_mut() = 0;
    return;
  }

  let margin = 2.min(viewport.saturating_sub(1));
  let max_offset = len.saturating_sub(viewport);
  let mut offset = state.offset().min(max_offset);
  if selected < offset + margin {
    offset = selected.saturating_sub(margin);
  } else if selected + margin >= offset + viewport {
    offset = selected
      .saturating_add(margin)
      .saturating_add(1)
      .saturating_sub(viewport)
      .min(max_offset);
  }
  *state.offset_mut() = offset;
}
