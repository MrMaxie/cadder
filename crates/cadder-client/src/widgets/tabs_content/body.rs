use std::net::IpAddr;

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Margin, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
  Block, Borders, Cell, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
  Table, TableState, Widget,
};

use crate::data::{DomainRowKind, DomainTableRow, SettingsTableRow};
use crate::widgets::theme::THEME;

const SELECTOR_COLUMN_WIDTH: u16 = 5;
const PRIMARY_COLUMN_MIN_WIDTH: u16 = 20;
const COLUMN_SPACING: u16 = 1;

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

    let trailing_available_width = inner
      .width
      .saturating_sub(SELECTOR_COLUMN_WIDTH + PRIMARY_COLUMN_MIN_WIDTH + 2 * COLUMN_SPACING);
    let trailing_column_width = fit_trailing_column_width(
      self.rows.trailing_column_content_width(),
      trailing_available_width,
    );
    let header = has_header.then(|| Row::new(self.rows.headers()).style(THEME.table_header()));
    let table = Table::new(
      self.rows.into_rows(),
      [
        Constraint::Length(SELECTOR_COLUMN_WIDTH),
        Constraint::Min(PRIMARY_COLUMN_MIN_WIDTH),
        Constraint::Length(trailing_column_width),
      ],
    )
    .column_spacing(COLUMN_SPACING)
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

  fn trailing_column_content_width(&self) -> usize {
    let row_width = match self {
      Self::Domains(rows) => rows
        .iter()
        .filter(|row| row.kind() == DomainRowKind::Domain)
        .map(|row| Line::from(display_upstream(row.endpoint())).width())
        .max(),
      Self::Settings(rows) => rows.iter().map(|row| Line::from(row.value()).width()).max(),
    }
    .unwrap_or_default();

    row_width.max(Line::from(self.headers()[2]).width())
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
      Cell::from(project_path_line(
        row.name(),
        row.name_emphasis_start().unwrap_or_default(),
        visually_enabled,
      )),
      Cell::from(""),
    ])
    .style(row_style),
    DomainRowKind::Domain => Row::new([
      Cell::from(checkbox(row.enabled(), visually_enabled)),
      Cell::from(format!("  {}", row.name())),
      Cell::from(right_aligned(display_upstream(row.endpoint()))),
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

fn project_path_line(path: &str, emphasis_start: usize, visually_enabled: bool) -> Line<'static> {
  let emphasis_start = emphasis_start.min(path.len());
  let emphasis_start = if path.is_char_boundary(emphasis_start) {
    emphasis_start
  } else {
    0
  };
  let (prefix, emphasized) = path.split_at(emphasis_start);

  Line::from(vec![
    Span::styled(
      prefix.to_string(),
      THEME.table_disabled_row().add_modifier(Modifier::DIM),
    ),
    Span::styled(emphasized.to_string(), project_name_style(visually_enabled)),
  ])
}

fn display_upstream(upstream: &str) -> String {
  let Some((host, port)) = upstream.rsplit_once(':') else {
    return upstream.to_string();
  };
  if port.parse::<u16>().is_err() {
    return upstream.to_string();
  }

  let host = host
    .strip_prefix('[')
    .and_then(|host| host.strip_suffix(']'))
    .unwrap_or(host);
  let is_loopback = host.eq_ignore_ascii_case("localhost")
    || host
      .parse::<IpAddr>()
      .is_ok_and(|address| address.is_loopback());

  if is_loopback {
    format!(":{port}")
  } else {
    upstream.to_string()
  }
}

fn right_aligned(value: String) -> Line<'static> {
  Line::from(value).alignment(Alignment::Right)
}

fn fit_trailing_column_width(content_width: usize, available_width: u16) -> u16 {
  u16::try_from(content_width)
    .unwrap_or(u16::MAX)
    .min(available_width)
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::data::DataModel;
  use cadder_ipc::{
    ActivationState, ConfigState, DomainName, EntrypointInstanceIdentity, EntrypointRegistration,
    GuiStateSnapshot, LogStreamIdentity, OwnerProcessIdentity, RegisteredDomain, RuntimeState,
    SourcePath,
  };
  use chrono::Utc;

  fn registration(
    id: &str,
    path: &str,
    enabled: bool,
    domains: &[(&str, bool, &str)],
  ) -> EntrypointRegistration {
    let now = Utc::now();
    EntrypointRegistration {
      registration_id: id.to_string(),
      entrypoint_instance: EntrypointInstanceIdentity {
        instance_id: id.to_string(),
        started_at_utc: now,
        shim_session_nonce: format!("{id}-nonce"),
      },
      source_working_directory: SourcePath::new(path, None),
      source_config_path: SourcePath::new(format!("{path}/Caddyfile"), None),
      registered_domains: domains
        .iter()
        .map(|(name, active, upstream)| RegisteredDomain {
          name: DomainName::parse(*name),
          activation_state: ActivationState::from_enabled(*active),
          upstream: Some((*upstream).to_string()),
          log_stream: LogStreamIdentity::domain(name),
        })
        .collect(),
      activation_state: ActivationState::from_enabled(enabled),
      owner_process: OwnerProcessIdentity {
        process_id: 42,
        process_start_time_utc: now,
        shim_session_nonce: format!("{id}-nonce"),
        executable_path: None,
      },
      log_stream: LogStreamIdentity::entrypoint(id),
      shim_run: None,
      created_at_utc: now,
      last_heartbeat_utc: now,
    }
  }

  fn buffer_text(buffer: &Buffer) -> String {
    buffer
      .content
      .chunks(usize::from(buffer.area.width))
      .map(|row| {
        row
          .iter()
          .map(ratatui::buffer::Cell::symbol)
          .collect::<String>()
      })
      .collect::<Vec<_>>()
      .join("\n")
  }

  #[test]
  fn trailing_column_grows_to_fit_its_longest_value() {
    let endpoint = "[ffff:ffff:ffff:ffff:ffff:ffff:255.255.255.255]:65535";
    let content_width = Line::from(endpoint).width();

    assert_eq!(
      fit_trailing_column_width(content_width, u16::MAX),
      u16::try_from(content_width).unwrap()
    );
  }

  #[test]
  fn trailing_column_is_limited_only_by_the_available_terminal_width() {
    assert_eq!(fit_trailing_column_width(usize::MAX, 31), 31);
  }

  #[test]
  fn loopback_upstreams_show_only_the_port() {
    for upstream in [
      "localhost:3000",
      "LOCALHOST:3000",
      "127.0.0.1:3000",
      "[::1]:3000",
      "::1:3000",
    ] {
      assert_eq!(display_upstream(upstream), ":3000");
    }
  }

  #[test]
  fn non_loopback_upstreams_remain_explicit() {
    assert_eq!(display_upstream("192.168.1.20:3000"), "192.168.1.20:3000");
    assert_eq!(display_upstream("example.test:3000"), "example.test:3000");
    assert_eq!(display_upstream("not-an-endpoint"), "not-an-endpoint");
    assert_eq!(display_upstream("example.test:http"), "example.test:http");
  }

  #[test]
  fn native_tables_render_dynamic_domain_and_settings_rows_with_scrollbars() {
    let mut model = DataModel::default();
    model.replace_snapshot(GuiStateSnapshot {
      captured_at_utc: Utc::now(),
      registrations: vec![
        registration(
          "entry-1",
          "D:/Projects/project-1",
          true,
          &[
            ("app.localhost", true, "127.0.0.1:51809"),
            ("api.example.test", false, "192.168.1.20:3000"),
          ],
        ),
        registration(
          "entry-2",
          "D:/Projects/project-2",
          false,
          &[("admin.localhost", true, "[::1]:65535")],
        ),
      ],
      runtime: RuntimeState::idle(),
      config: ConfigState::idle(),
      storage: None,
    });

    let area = Rect::new(0, 0, 80, 12);
    let mut buffer = Buffer::empty(area);
    let mut state = TableState::default().with_selected(Some(0));
    TableBody::new(
      THEME.domains_accent(),
      TableBodyRows::Domains(model.domain_rows()),
    )
    .render(area, &mut buffer, &mut state);
    let text = buffer_text(&buffer);
    assert!(text.contains("Entrypoint / domain"));
    assert!(text.contains("project-1"));
    assert!(text.contains("app.localhost"));
    assert!(text.contains(":51809"));
    assert!(!text.contains("127.0.0.1"));
    assert!(text.contains("192.168.1.20:3000"));

    let small_area = Rect::new(0, 0, 80, 8);
    let mut small_buffer = Buffer::empty(small_area);
    let mut state = TableState::default().with_selected(Some(4));
    TableBody::new(
      THEME.domains_accent(),
      TableBodyRows::Domains(model.domain_rows()),
    )
    .render(small_area, &mut small_buffer, &mut state);
    assert!(state.offset() > 0);

    let settings = model.status_rows("Connected");
    let mut buffer = Buffer::empty(Rect::new(0, 0, 60, 12));
    let mut state = TableState::default().with_selected(Some(0));
    TableBody::new(THEME.settings_accent(), TableBodyRows::Settings(settings)).render(
      Rect::new(0, 0, 60, 12),
      &mut buffer,
      &mut state,
    );
    let text = buffer_text(&buffer);
    assert!(text.contains("Component"));
    assert!(text.contains("Daemon"));
    assert!(text.contains("Connected"));
  }

  #[test]
  fn scroll_margin_handles_empty_small_and_large_viewports() {
    let mut unselected = TableState::default();
    apply_scroll_margin(&mut unselected, 20, 5);
    assert_eq!(unselected.offset(), 0);

    let mut small = TableState::default().with_selected(Some(2));
    *small.offset_mut() = 9;
    apply_scroll_margin(&mut small, 3, 5);
    assert_eq!(small.offset(), 0);

    let mut large = TableState::default().with_selected(Some(18));
    apply_scroll_margin(&mut large, 20, 5);
    assert_eq!(large.offset(), 15);
    large.select(Some(1));
    apply_scroll_margin(&mut large, 20, 5);
    assert_eq!(large.offset(), 0);

    let unicode = project_path_line("żółć/project", 1, true);
    assert_eq!(unicode.width(), Line::from("żółć/project").width());
  }
}
