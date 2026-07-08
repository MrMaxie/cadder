use tui_term::vt100::Screen;

pub(crate) const MAX_LOG_LINES: usize = 1_000;

pub struct LogStore {
  screen: Option<Screen>,
  scrollback: usize,
  follow_tail: bool,
  viewport: TerminalSize,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct TerminalSize {
  rows: u16,
  cols: u16,
}

impl TerminalSize {
  fn new(rows: u16, cols: u16) -> Self {
    Self {
      rows: if rows == 0 { 1 } else { rows },
      cols: if cols == 0 { 1 } else { cols },
    }
  }

  pub(crate) const fn rows(self) -> u16 {
    self.rows
  }

  pub(crate) const fn cols(self) -> u16 {
    self.cols
  }
}

impl LogStore {
  pub fn new() -> Self {
    Self {
      screen: None,
      scrollback: 0,
      follow_tail: true,
      viewport: TerminalSize::new(1, 1),
    }
  }

  pub fn poll(&mut self) {}

  pub fn set_viewport(&mut self, rows: u16, cols: u16) {
    let viewport = TerminalSize::new(rows, cols);
    let viewport_changed = self.viewport != viewport;
    self.viewport = viewport;

    if !viewport_changed {
      return;
    }

    if let Some(screen) = &mut self.screen {
      screen.set_size(viewport.rows(), viewport.cols());
    }
    self.clamp_scrollback();
  }

  pub fn screen(&self) -> Option<Screen> {
    let mut screen = self.screen.clone()?;
    if !self.follow_tail {
      screen.set_scrollback(self.scrollback.min(available_scrollback(&screen)));
    }
    Some(screen)
  }

  pub fn scroll_up(&mut self, amount: usize) {
    let available_scrollback = self.current_available_scrollback();
    self.scrollback = self
      .scrollback
      .saturating_add(amount)
      .min(available_scrollback);
    self.follow_tail = self.scrollback == 0;
  }

  pub fn scroll_down(&mut self, amount: usize) {
    self.scrollback = self.scrollback.saturating_sub(amount);
    self.follow_tail = self.scrollback == 0;
  }

  fn current_available_scrollback(&self) -> usize {
    self.screen.as_ref().map(available_scrollback).unwrap_or(0)
  }

  fn clamp_scrollback(&mut self) {
    self.scrollback = self.scrollback.min(self.current_available_scrollback());
    self.follow_tail = self.scrollback == 0;
  }
}

fn available_scrollback(screen: &Screen) -> usize {
  let mut screen = screen.clone();
  screen.set_scrollback(MAX_LOG_LINES);
  screen.scrollback()
}
