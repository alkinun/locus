use std::io::{self, Stdout};
use std::panic;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use crate::model::{ChunkKind, RankedChunk, SearchResult};
use crate::output::{GroupedResults, group_ranked_results};
use crate::query::{AnalyzedQuery, QueryIntent};
use crate::search::SearchSession;

const SEARCH_LIMIT: usize = 40;
const DEBOUNCE: Duration = Duration::from_millis(75);

pub fn run_tui(repo_path: PathBuf) -> Result<()> {
    let repo_path = repo_path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", repo_path.display()))?;
    if !repo_path.join(".locus").join("index").exists() {
        println!(
            "No locus index found at {}\n\nRun:\n  locus index {}",
            repo_path.join(".locus").join("index").display(),
            repo_path.display()
        );
        return Ok(());
    }

    let session = SearchSession::open(&repo_path)?;
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        previous_hook(info);
    }));
    let mut terminal = setup_terminal()?;
    let result = run_loop(&mut terminal, repo_path, session);
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout)).map_err(Into::into)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    repo_path: PathBuf,
    session: SearchSession,
) -> Result<()> {
    let mut app = TuiApp::new(repo_path, session.chunk_count());
    let mut pending_search_at: Option<Instant> = None;

    loop {
        terminal.draw(|frame| render(frame, &mut app))?;

        if pending_search_at.is_some_and(|at| at <= Instant::now()) {
            app.search(&session);
            pending_search_at = None;
        }

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                match app.handle_key(key) {
                    Action::Quit => break,
                    Action::SearchChanged => pending_search_at = Some(Instant::now() + DEBOUNCE),
                    Action::None => {}
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
struct TuiApp {
    repo_path: PathBuf,
    query: String,
    analyzed: Option<AnalyzedQuery>,
    results: Vec<RankedChunk>,
    grouped_results: Option<GroupedResults>,
    selected: usize,
    result_scroll: usize,
    grouped: bool,
    last_search_ms: Option<u128>,
    status: String,
    chunk_count: usize,
}

impl TuiApp {
    fn new(repo_path: PathBuf, chunk_count: usize) -> Self {
        Self {
            repo_path,
            query: String::new(),
            analyzed: None,
            results: Vec::new(),
            grouped_results: None,
            selected: 0,
            result_scroll: 0,
            grouped: false,
            last_search_ms: None,
            status: String::new(),
            chunk_count,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'q'))
        {
            return Action::Quit;
        }
        match key.code {
            KeyCode::Down => {
                let max = self.visible_len().saturating_sub(1);
                self.selected = (self.selected + 1).min(max);
                Action::None
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                Action::None
            }
            KeyCode::Char(ch) => {
                self.query.push(ch);
                Action::SearchChanged
            }
            KeyCode::Backspace => {
                self.query.pop();
                if self.query.trim().len() <= 1 {
                    self.results.clear();
                    self.grouped_results = None;
                    self.analyzed = None;
                    self.selected = 0;
                    self.result_scroll = 0;
                }
                Action::SearchChanged
            }
            KeyCode::Esc => {
                self.query.clear();
                self.results.clear();
                self.grouped_results = None;
                self.analyzed = None;
                self.selected = 0;
                self.result_scroll = 0;
                self.status.clear();
                Action::None
            }
            KeyCode::Tab => {
                self.grouped = !self.grouped;
                self.grouped_results = Some(group_ranked_results(&self.results));
                self.selected = self.selected.min(self.visible_len().saturating_sub(1));
                self.result_scroll = 0;
                Action::None
            }
            _ => Action::None,
        }
    }

    fn search(&mut self, session: &SearchSession) {
        if self.query.trim().len() <= 1 {
            return;
        }
        match session.search(&self.query, SEARCH_LIMIT) {
            Ok(summary) => {
                self.last_search_ms = Some(summary.elapsed.as_millis());
                self.analyzed = Some(summary.analyzed);
                self.results = summary.results;
                self.grouped_results = Some(group_ranked_results(&self.results));
                self.selected = self.selected.min(self.visible_len().saturating_sub(1));
                self.result_scroll = self
                    .result_scroll
                    .min(result_list_item_count(self).saturating_sub(1));
                self.status.clear();
            }
            Err(err) => self.status = err.to_string(),
        }
    }

    fn visible_len(&self) -> usize {
        if self.grouped {
            flatten_grouped(self.grouped_results.as_ref()).len()
        } else {
            self.results.len()
        }
    }

    fn selected_result(&self) -> Option<VisibleResult> {
        let results = if self.grouped {
            flatten_grouped(self.grouped_results.as_ref())
        } else {
            self.results
                .iter()
                .map(VisibleResult::from_ranked)
                .collect()
        };
        results.get(self.selected).cloned()
    }

    fn update_result_scroll(&mut self, viewport_height: usize) {
        if viewport_height == 0 || self.results.is_empty() {
            self.result_scroll = 0;
            return;
        }
        let Some((selected_top, selected_bottom)) = selected_list_item_range(self) else {
            self.result_scroll = 0;
            return;
        };
        if selected_top < self.result_scroll {
            self.result_scroll = selected_top;
        } else {
            let viewport_bottom = self.result_scroll + viewport_height - 1;
            if selected_bottom > viewport_bottom {
                self.result_scroll = selected_bottom + 1 - viewport_height;
            }
        }
        let max_scroll = result_list_item_count(self).saturating_sub(viewport_height);
        self.result_scroll = self.result_scroll.min(max_scroll);
    }
}

enum Action {
    None,
    SearchChanged,
    Quit,
}

#[derive(Debug, Clone)]
pub struct VisibleResult {
    pub file_path: String,
    pub kind: ChunkKind,
    pub symbol: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
    pub score: f32,
    pub reason: String,
    pub text: String,
}

impl VisibleResult {
    fn from_ranked(result: &RankedChunk) -> Self {
        Self {
            file_path: result.chunk.file_path.display().to_string(),
            kind: result.chunk.kind,
            symbol: result.chunk.symbol.clone(),
            start_line: result.chunk.start_line,
            end_line: result.chunk.end_line,
            score: result.score,
            reason: result.reason.clone(),
            text: result.chunk.text.clone(),
        }
    }

    fn from_result(result: &SearchResult) -> Self {
        Self {
            file_path: result.file_path.clone(),
            kind: result.kind,
            symbol: result.symbol.clone(),
            start_line: result.start_line,
            end_line: result.end_line,
            score: result.score,
            reason: result.reason.clone(),
            text: result.text.clone(),
        }
    }
}

fn render(frame: &mut ratatui::Frame<'_>, app: &mut TuiApp) {
    let layout = tui_layout(frame.area());

    render_header(frame, layout.header, app);
    render_query(frame, layout.query, app);
    render_results(frame, layout.results, app);
    render_preview(frame, layout.preview, app);
    render_footer(frame, layout.footer, app);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TuiLayout {
    header: Rect,
    query: Rect,
    results: Rect,
    preview: Rect,
    footer: Rect,
    side_by_side: bool,
}

fn tui_layout(area: Rect) -> TuiLayout {
    let header_height = area.height.min(1);
    let query_height = area.height.saturating_sub(header_height).min(3);
    let footer_height = area
        .height
        .saturating_sub(header_height + query_height)
        .min(1);
    let content_height = area
        .height
        .saturating_sub(header_height + query_height + footer_height);
    let mut y = area.y;
    let header = Rect::new(area.x, y, area.width, header_height);
    y = y.saturating_add(header_height);
    let query = Rect::new(area.x, y, area.width, query_height);
    y = y.saturating_add(query_height);
    let footer = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(footer_height),
        area.width,
        footer_height,
    );
    let content = Rect::new(area.x, y, area.width, content_height);
    let side_by_side = content.width >= 100 && content.height >= 8;

    let (results, preview) = if side_by_side {
        let results_width = ((content.width as usize * 45) / 100)
            .clamp(44, content.width.saturating_sub(48) as usize)
            as u16;
        let preview_width = content.width.saturating_sub(results_width);
        (
            Rect::new(content.x, content.y, results_width, content.height),
            Rect::new(
                content.x.saturating_add(results_width),
                content.y,
                preview_width,
                content.height,
            ),
        )
    } else {
        let preview_height = if content_height >= 12 {
            (content_height / 3).clamp(5, content_height.saturating_sub(6))
        } else if content_height >= 8 {
            3
        } else {
            0
        };
        let results_height = content_height.saturating_sub(preview_height);
        (
            Rect::new(content.x, content.y, content.width, results_height),
            Rect::new(
                content.x,
                content.y.saturating_add(results_height),
                content.width,
                preview_height,
            ),
        )
    };

    TuiLayout {
        header,
        query,
        results,
        preview,
        footer,
        side_by_side,
    }
}

fn render_header(frame: &mut ratatui::Frame<'_>, area: Rect, app: &TuiApp) {
    let header = format!(
        "locus · {} · index ready · {} chunks · grouped {}",
        shorten_path(&app.repo_path, area.width.saturating_sub(45) as usize),
        app.chunk_count,
        if app.grouped { "on" } else { "off" }
    );
    frame.render_widget(Paragraph::new(header), area);
}

fn render_query(frame: &mut ratatui::Frame<'_>, area: Rect, app: &TuiApp) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let input_y = area.y + area.height.saturating_sub(2).min(1);
    let input_area = Rect::new(area.x, input_y, area.width, 1);
    let summary_area = Rect::new(
        area.x,
        input_y.saturating_add(1),
        area.width,
        area.y
            .saturating_add(area.height)
            .saturating_sub(input_y.saturating_add(1)),
    );
    let query = if app.query.is_empty() {
        Line::from(Span::styled(
            "Start typing to search this repo",
            Style::default().fg(Color::Gray),
        ))
    } else {
        Line::from(vec![
            Span::styled(" ❯ ", Style::default().fg(Color::Cyan)),
            Span::styled(app.query.as_str(), Style::default().fg(Color::White)),
        ])
    };
    let summary = app
        .analyzed
        .as_ref()
        .map(|analyzed| analysis_summary(analyzed, app.last_search_ms))
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(query).style(Style::default().bg(Color::Rgb(28, 32, 36))),
        input_area,
    );
    frame.render_widget(
        Paragraph::new(summary).style(Style::default().fg(Color::DarkGray)),
        summary_area,
    );
}

fn render_results(frame: &mut ratatui::Frame<'_>, area: Rect, app: &mut TuiApp) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let block = Block::default()
        .title(" Results ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.query.trim().is_empty() {
        frame.render_widget(
            Paragraph::new(
                "Examples: ranking implementation · symbol detection · tests for chunking",
            )
            .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }
    if app.results.is_empty() {
        frame.render_widget(Paragraph::new("No results"), inner);
        return;
    }

    let items = if app.grouped {
        grouped_items(app)
    } else {
        flat_items(app)
    };
    app.update_result_scroll(inner.height as usize);
    let mut state = ListState::default().with_offset(app.result_scroll);
    state.select(selected_list_item_index(app));
    frame.render_stateful_widget(List::new(items), inner, &mut state);
}

fn flat_items(app: &TuiApp) -> Vec<ListItem<'static>> {
    app.results
        .iter()
        .enumerate()
        .flat_map(|(idx, result)| {
            result_lines(
                &VisibleResult::from_ranked(result),
                idx == app.selected,
                None,
            )
        })
        .collect()
}

fn grouped_items(app: &TuiApp) -> Vec<ListItem<'static>> {
    let mut items = Vec::new();
    let mut selected_idx = 0usize;
    if let Some(grouped) = &app.grouped_results {
        for (title, group) in [
            ("Primary", grouped.primary.as_slice()),
            ("Supporting", grouped.supporting.as_slice()),
            ("Tests", grouped.tests.as_slice()),
            ("Docs", grouped.docs.as_slice()),
            ("Config", grouped.config.as_slice()),
        ] {
            if group.is_empty() {
                continue;
            }
            items.push(ListItem::new(Line::from(Span::styled(
                title,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))));
            for result in group {
                let visible = VisibleResult::from_result(result);
                items.extend(result_lines(
                    &visible,
                    selected_idx == app.selected,
                    Some("  "),
                ));
                selected_idx += 1;
            }
        }
    }
    items
}

fn selected_list_item_index(app: &TuiApp) -> Option<usize> {
    if app.results.is_empty() {
        return None;
    }
    if !app.grouped {
        return Some(app.selected.saturating_mul(RESULT_ITEM_HEIGHT));
    }

    let Some(grouped) = &app.grouped_results else {
        return None;
    };
    let mut result_idx = 0usize;
    let mut item_idx = 0usize;
    for group in [
        grouped.primary.as_slice(),
        grouped.supporting.as_slice(),
        grouped.tests.as_slice(),
        grouped.docs.as_slice(),
        grouped.config.as_slice(),
    ] {
        if group.is_empty() {
            continue;
        }
        item_idx += 1;
        for _ in group {
            if result_idx == app.selected {
                return Some(item_idx);
            }
            result_idx += 1;
            item_idx += RESULT_ITEM_HEIGHT;
        }
    }
    None
}

fn selected_list_item_range(app: &TuiApp) -> Option<(usize, usize)> {
    let top = selected_list_item_index(app)?;
    Some((top, top + RESULT_ITEM_HEIGHT.saturating_sub(1)))
}

const RESULT_ITEM_HEIGHT: usize = 3;

fn result_list_item_count(app: &TuiApp) -> usize {
    if !app.grouped {
        return app.results.len() * RESULT_ITEM_HEIGHT;
    }
    let Some(grouped) = &app.grouped_results else {
        return 0;
    };
    [
        grouped.primary.as_slice(),
        grouped.supporting.as_slice(),
        grouped.tests.as_slice(),
        grouped.docs.as_slice(),
        grouped.config.as_slice(),
    ]
    .iter()
    .filter(|group| !group.is_empty())
    .map(|group| 1 + group.len() * RESULT_ITEM_HEIGHT)
    .sum()
}

fn result_lines(
    result: &VisibleResult,
    selected: bool,
    prefix: Option<&str>,
) -> Vec<ListItem<'static>> {
    let bullet = if selected { "●" } else { " " };
    let prefix = prefix.unwrap_or("");
    let symbol = result.symbol.as_deref().unwrap_or("-");
    let row = format!(
        "{prefix}{bullet} {:<28} {:<7} {:<22} {:>5.1}",
        format!(
            "{}:{}-{}",
            result.file_path, result.start_line, result.end_line
        ),
        kind_label(result.kind),
        truncate(symbol, 22),
        result.score
    );
    let reason = format!("{prefix}  {}", truncate(&result.reason, 92));
    let style = if selected {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    vec![
        ListItem::new(Line::from(Span::styled(row, style))),
        ListItem::new(Line::from(Span::styled(
            reason,
            Style::default().fg(Color::DarkGray),
        ))),
        ListItem::new(Line::raw("")),
    ]
}

fn render_preview(frame: &mut ratatui::Frame<'_>, area: Rect, app: &TuiApp) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let selected = app.selected_result();
    let title = selected
        .as_ref()
        .map(|result| {
            format!(
                " Preview: {}:{}-{}{} ",
                result.file_path,
                result.start_line,
                result.end_line,
                result
                    .symbol
                    .as_ref()
                    .map(|symbol| format!(" · {symbol}"))
                    .unwrap_or_default()
            )
        })
        .unwrap_or_else(|| " Preview ".to_string());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let text = selected
        .as_ref()
        .map(|result| line_numbered_preview(&result.text, result.start_line, inner.height as usize))
        .unwrap_or_else(|| "Select a result to preview it".to_string());
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
}

fn render_footer(frame: &mut ratatui::Frame<'_>, area: Rect, app: &TuiApp) {
    let help = if app.status.is_empty() {
        "Type to search · ↑/↓ select · Tab grouped/flat · Esc clear · Ctrl-C/Ctrl-Q quit"
            .to_string()
    } else {
        app.status.clone()
    };
    frame.render_widget(
        Paragraph::new(help).style(Style::default().fg(if app.status.is_empty() {
            Color::DarkGray
        } else {
            Color::Red
        })),
        area,
    );
}

pub fn kind_label(kind: ChunkKind) -> &'static str {
    match kind {
        ChunkKind::Function => "fn",
        ChunkKind::Method => "method",
        ChunkKind::Class => "class",
        ChunkKind::Struct => "struct",
        ChunkKind::Enum => "enum",
        ChunkKind::Trait => "trait",
        ChunkKind::Impl => "impl",
        ChunkKind::Test => "test",
        ChunkKind::MarkdownSection => "docs",
        ChunkKind::Config => "config",
        ChunkKind::Module | ChunkKind::Unknown => "chunk",
    }
}

fn analysis_summary(analyzed: &AnalyzedQuery, elapsed: Option<u128>) -> String {
    let mut parts = vec![intent_label(analyzed.intent).to_string()];
    if !analyzed.expansions.is_empty() {
        parts.push(format!(
            "expanded: {}",
            truncate(
                &analyzed
                    .expansions
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                56
            )
        ));
    }
    if let Some(ms) = elapsed {
        parts.push(format!("{ms}ms"));
    }
    parts.join(" · ")
}

fn intent_label(intent: QueryIntent) -> &'static str {
    match intent {
        QueryIntent::FindImplementation => "implementation",
        QueryIntent::FindDefinition => "definition",
        QueryIntent::FindUsage => "usage",
        QueryIntent::FindTests => "tests",
        QueryIntent::ExplainCapability => "capability",
        QueryIntent::FindConfig => "config",
        QueryIntent::Unknown => "search",
    }
}

pub fn line_numbered_preview(text: &str, start_line: usize, max_lines: usize) -> String {
    text.lines()
        .take(max_lines)
        .enumerate()
        .map(|(idx, line)| format!("{:>4} │ {}", start_line + idx, line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn flatten_grouped(grouped: Option<&GroupedResults>) -> Vec<VisibleResult> {
    let Some(grouped) = grouped else {
        return Vec::new();
    };
    grouped
        .primary
        .iter()
        .chain(grouped.supporting.iter())
        .chain(grouped.tests.iter())
        .chain(grouped.docs.iter())
        .chain(grouped.config.iter())
        .map(VisibleResult::from_result)
        .collect()
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!("{}…", text.chars().take(keep).collect::<String>())
}

fn shorten_path(path: &Path, max_chars: usize) -> String {
    let text = path.display().to_string();
    if text.chars().count() <= max_chars {
        text
    } else {
        format!(
            "…{}",
            text.chars()
                .rev()
                .take(max_chars.saturating_sub(1))
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SearchResult;

    #[test]
    fn formats_kind_labels() {
        assert_eq!(kind_label(ChunkKind::Function), "fn");
        assert_eq!(kind_label(ChunkKind::MarkdownSection), "docs");
        assert_eq!(kind_label(ChunkKind::Unknown), "chunk");
    }

    #[test]
    fn formats_line_numbered_preview() {
        let preview = line_numbered_preview("one\ntwo\nthree", 41, 2);
        assert_eq!(preview, "  41 │ one\n  42 │ two");
    }

    #[test]
    fn flattens_grouped_results_in_display_order() {
        let grouped = GroupedResults {
            primary: vec![result("src/lib.rs", ChunkKind::Function)],
            supporting: vec![result("src/model.rs", ChunkKind::Struct)],
            tests: vec![result("tests/search.rs", ChunkKind::Test)],
            docs: vec![result("README.md", ChunkKind::MarkdownSection)],
            config: vec![result("Cargo.toml", ChunkKind::Config)],
        };
        let flat = flatten_grouped(Some(&grouped));
        assert_eq!(flat.len(), 5);
        assert_eq!(flat[0].file_path, "src/lib.rs");
        assert_eq!(flat[4].file_path, "Cargo.toml");
    }

    #[test]
    fn truncates_long_text() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
    }

    #[test]
    fn printable_navigation_letters_are_query_input() {
        let mut app = TuiApp::new(PathBuf::from("/repo"), 0);

        assert!(matches!(
            app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
            Action::SearchChanged
        ));
        assert!(matches!(
            app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE)),
            Action::SearchChanged
        ));
        assert!(matches!(
            app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            Action::SearchChanged
        ));
        assert_eq!(app.query, "jkq");
    }

    #[test]
    fn control_q_quits() {
        let mut app = TuiApp::new(PathBuf::from("/repo"), 0);

        assert!(matches!(
            app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            Action::Quit
        ));
        assert!(app.query.is_empty());
    }

    #[test]
    fn layout_splits_results_and_preview_side_by_side_on_wide_screens() {
        let layout = tui_layout(Rect::new(0, 0, 120, 24));
        assert!(layout.side_by_side);
        assert_eq!(layout.results.y, layout.preview.y);
        assert_eq!(layout.results.height, layout.preview.height);
        assert_eq!(layout.results.x + layout.results.width, layout.preview.x);
        assert_eq!(
            layout.preview.x + layout.preview.width,
            layout.footer.x + layout.footer.width
        );
        assert_eq!(layout.results.y + layout.results.height, layout.footer.y);
        assert!(layout.results.width >= 44);
        assert!(layout.preview.width >= 48);
    }

    #[test]
    fn layout_stacks_results_above_preview_on_narrow_screens() {
        let layout = tui_layout(Rect::new(0, 0, 80, 24));
        assert!(!layout.side_by_side);
        assert_eq!(layout.results.y + layout.results.height, layout.preview.y);
        assert_eq!(layout.preview.y + layout.preview.height, layout.footer.y);
        assert!(layout.results.height >= 6);
        assert!(layout.preview.height >= 5);
    }

    #[test]
    fn layout_drops_preview_before_overlapping_on_short_screens() {
        let layout = tui_layout(Rect::new(0, 0, 80, 10));
        assert_eq!(layout.results.y + layout.results.height, layout.preview.y);
        assert_eq!(layout.preview.height, 0);
        assert_eq!(layout.results.y + layout.results.height, layout.footer.y);
    }

    #[test]
    fn maps_flat_selection_to_first_result_line() {
        let mut app = TuiApp::new(PathBuf::from("/repo"), 2);
        app.results = vec![
            ranked("src/a.rs", ChunkKind::Function),
            ranked("src/b.rs", ChunkKind::Function),
        ];
        app.selected = 1;

        assert_eq!(selected_list_item_index(&app), Some(3));
    }

    #[test]
    fn maps_grouped_selection_past_headers() {
        let mut app = TuiApp::new(PathBuf::from("/repo"), 2);
        app.results = vec![
            ranked("src/a.rs", ChunkKind::Function),
            ranked("src/model.rs", ChunkKind::Struct),
        ];
        app.grouped = true;
        app.grouped_results = Some(group_ranked_results(&app.results));
        app.selected = 1;

        assert_eq!(selected_list_item_index(&app), Some(5));
    }

    #[test]
    fn flat_scroll_holds_position_while_moving_back_up_inside_viewport() {
        let mut app = TuiApp::new(PathBuf::from("/repo"), 4);
        app.results = vec![
            ranked("src/a.rs", ChunkKind::Function),
            ranked("src/b.rs", ChunkKind::Function),
            ranked("src/c.rs", ChunkKind::Function),
            ranked("src/d.rs", ChunkKind::Function),
        ];

        app.selected = 1;
        app.update_result_scroll(6);
        assert_eq!(app.result_scroll, 0);

        app.selected = 2;
        app.update_result_scroll(6);
        assert_eq!(app.result_scroll, 3);

        app.selected = 1;
        app.update_result_scroll(6);
        assert_eq!(app.result_scroll, 3);

        app.selected = 0;
        app.update_result_scroll(6);
        assert_eq!(app.result_scroll, 0);
    }

    #[test]
    fn grouped_scroll_holds_position_while_moving_back_up_inside_viewport() {
        let mut app = TuiApp::new(PathBuf::from("/repo"), 4);
        app.results = vec![
            ranked("src/a.rs", ChunkKind::Function),
            ranked("src/b.rs", ChunkKind::Function),
            ranked("src/model.rs", ChunkKind::Struct),
            ranked("src/other.rs", ChunkKind::Struct),
        ];
        app.grouped = true;
        app.grouped_results = Some(group_ranked_results(&app.results));

        app.selected = 1;
        app.update_result_scroll(7);
        assert_eq!(app.result_scroll, 0);

        app.selected = 2;
        app.update_result_scroll(7);
        assert_eq!(app.result_scroll, 4);

        app.selected = 1;
        app.update_result_scroll(7);
        assert_eq!(app.result_scroll, 4);

        app.selected = 0;
        app.update_result_scroll(7);
        assert_eq!(app.result_scroll, 1);
    }

    fn result(path: &str, kind: ChunkKind) -> SearchResult {
        SearchResult {
            rank: 1,
            score: 1.0,
            file_path: path.into(),
            language: "rust".into(),
            kind,
            symbol: Some("item".into()),
            signature: None,
            parent_symbol: None,
            start_line: 1,
            end_line: 2,
            reason: "reason".into(),
            text: "text".into(),
        }
    }

    fn ranked(path: &str, kind: ChunkKind) -> RankedChunk {
        RankedChunk {
            chunk: crate::model::CodeChunk {
                id: path.into(),
                repo_root: PathBuf::from("/repo"),
                file_path: PathBuf::from(path),
                language: "rust".into(),
                kind,
                symbol: Some("item".into()),
                signature: None,
                parent_symbol: None,
                start_line: 1,
                end_line: 2,
                doc_comment: String::new(),
                callees: Vec::new(),
                sibling_symbols: Vec::new(),
                text: "text".into(),
            },
            score: 1.0,
            reason: "reason".into(),
        }
    }
}
