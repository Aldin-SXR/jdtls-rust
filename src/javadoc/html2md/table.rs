//! Port of flexmark-util-format 0.64.8 `MarkdownTable` (formatting only; no
//! offset tracking), with the default `TableFormatOptions`.

use super::line_appendable::{LineAppendable, F_WHITESPACE_REMOVAL};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

impl Align {
    /// `CellAlignment.getAlignment`
    pub fn parse(s: &str) -> Align {
        match s.to_ascii_uppercase().as_str() {
            "LEFT" => Align::Left,
            "CENTER" => Align::Center,
            "RIGHT" => Align::Right,
            _ => Align::None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub text: String,
    pub row_span: usize,
    pub column_span: usize,
    pub alignment: Align,
    /// `TableCell.NULL` (span placeholder)
    pub is_null: bool,
}

impl Cell {
    pub fn new(text: &str, row_span: usize, column_span: usize, alignment: Option<Align>) -> Cell {
        Cell {
            text: if text.is_empty() { " ".into() } else { text.into() },
            row_span,
            column_span,
            alignment: alignment.unwrap_or(Align::None),
            is_null: false,
        }
    }
    fn null() -> Cell {
        Cell { text: " ".into(), row_span: 1, column_span: 0, alignment: Align::None, is_null: true }
    }
}

#[derive(Clone, Debug, Default)]
struct Row {
    cells: Vec<Option<Cell>>,
}

impl Row {
    fn expand_to(&mut self, column: usize, cell: Option<Cell>) {
        while column >= self.cells.len() {
            self.cells.push(cell.clone());
        }
    }
    fn set(&mut self, column: usize, cell: Cell) {
        self.expand_to(column, None);
        self.cells[column] = Some(cell);
    }
    fn normalize(&mut self) {
        self.cells.retain(|c| matches!(c, Some(c) if !c.is_null));
    }
    fn spanned_columns(&self) -> usize {
        self.cells.iter().flatten().map(|c| c.column_span).sum()
    }
    fn cell(&self, i: usize) -> &Cell {
        self.cells[i].as_ref().unwrap()
    }
}

#[derive(Clone, Debug, Default)]
struct Section {
    rows: Vec<Row>,
    row: usize,
    column: usize,
}

impl Section {
    fn next_row(&mut self) {
        self.row += 1;
        self.column = 0;
    }
    fn get(&mut self, row: usize) -> &mut Row {
        while row >= self.rows.len() {
            self.rows.push(Row::default());
        }
        &mut self.rows[row]
    }
    fn normalize(&mut self) {
        for r in &mut self.rows {
            r.normalize();
        }
    }
    fn max_columns(&self) -> usize {
        self.rows.iter().map(|r| r.spanned_columns()).max().unwrap_or(0)
    }
}

// TableFormatOptions defaults (CharWidthProvider.NULL: every char has width 1)
const SPACE_WIDTH: isize = 1;
const SPACE_PAD: isize = 2;
const PIPE_WIDTH: isize = 1;
const COLON_WIDTH: isize = 1;
const DASH_WIDTH: isize = 1;
const MIN_SEPARATOR_COLUMN_WIDTH: isize = 3;
const MIN_SEPARATOR_DASHES: isize = 1;

fn width(s: &str) -> isize {
    s.encode_utf16().count() as isize
}

#[derive(Clone, Debug)]
pub struct MarkdownTable {
    header: Section,
    separator: Section,
    body: Section,
    caption: Option<String>,
    is_heading: bool,
    alignments: Vec<Option<Align>>,
    column_widths: Vec<isize>,
}

impl Default for MarkdownTable {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkdownTable {
    pub fn new() -> Self {
        MarkdownTable {
            header: Section::default(),
            separator: Section::default(),
            body: Section::default(),
            caption: None,
            is_heading: true,
            alignments: Vec::new(),
            column_widths: Vec::new(),
        }
    }

    pub fn get_header(&self) -> bool {
        self.is_heading
    }
    pub fn set_header(&mut self, h: bool) {
        self.is_heading = h;
    }
    pub fn body_row_count(&self) -> usize {
        self.body.rows.len()
    }
    pub fn set_caption(&mut self, caption: &str) {
        self.caption = Some(if caption.is_empty() { " ".into() } else { caption.into() });
    }

    pub fn next_row(&mut self) {
        if self.is_heading {
            self.header.next_row();
        } else {
            self.body.next_row();
        }
    }

    pub fn add_cell(&mut self, cell: Cell) {
        let section = if self.is_heading { &mut self.header } else { &mut self.body };
        let row = section.row;
        {
            let mut col = section.column;
            let current = section.get(row);
            while col < current.cells.len() && current.cells[col].is_some() {
                col += 1;
            }
            section.column = col;
        }
        let column = section.column;
        let mut row_span = 0;
        while row_span < cell.row_span {
            section.get(row + row_span).set(column, cell.clone());
            let mut column_span = 1;
            while column_span < cell.column_span {
                // TableSection.expandTo(row, column) pads with TableCell.NULL (a non-null cell)
                let r = section.get(row + row_span);
                r.expand_to(column + column_span, Some(Cell::null()));
                if r.cells[column + column_span].is_some() {
                    break;
                }
                r.cells[column + column_span] = Some(Cell::null());
                column_span += 1;
            }
            row_span += 1;
        }
        section.column += cell.column_span;
    }

    fn normalize(&mut self) {
        self.header.normalize();
        self.separator.normalize();
        self.body.normalize();
    }

    pub fn max_columns(&self) -> usize {
        self.header.max_columns().max(self.separator.max_columns()).max(self.body.max_columns())
    }

    fn cell_text_width(&self, text: &str) -> isize {
        width(text)
    }

    pub fn finalize_table(&mut self) {
        self.normalize();
        let sep_columns = self.max_columns();
        self.alignments = vec![None; sep_columns];
        self.column_widths = vec![0; sep_columns];
        let mut span_alignment = vec![false; sep_columns];

        for row in &self.header.rows {
            let mut j_span = 0;
            for k in 0..row.cells.len() {
                let cell = row.cell(k);
                if j_span < sep_columns
                    && (self.alignments[j_span].is_none() || cell.column_span == 1 && span_alignment[j_span])
                    && cell.alignment != Align::None
                {
                    self.alignments[j_span] = Some(cell.alignment);
                    if cell.column_span > 1 {
                        span_alignment[j_span] = true;
                    }
                }
                let w = self.cell_text_width(&cell.text) + SPACE_PAD + PIPE_WIDTH * cell.column_span as isize;
                if cell.column_span <= 1 && self.column_widths[j_span] < w {
                    self.column_widths[j_span] = w;
                }
                j_span += cell.column_span;
            }
        }
        for row in &self.body.rows {
            let mut j_span = 0;
            for k in 0..row.cells.len() {
                let cell = row.cell(k);
                let w = self.cell_text_width(&cell.text) + SPACE_PAD + PIPE_WIDTH * cell.column_span as isize;
                if cell.column_span <= 1 && self.column_widths[j_span] < w {
                    self.column_widths[j_span] = w;
                }
                j_span += cell.column_span;
            }
        }
        // separator column widths (no separator rows in converted tables)
        for j in 0..self.alignments.len() {
            let a = self.alignments[j];
            let colon = colon_count(a);
            let dash_count = 0isize.max(MIN_SEPARATOR_COLUMN_WIDTH - colon).max(MIN_SEPARATOR_DASHES);
            let w = dash_count * DASH_WIDTH + colon * COLON_WIDTH + PIPE_WIDTH;
            if self.column_widths[j] < w {
                self.column_widths[j] = w;
            }
        }
        // NOTE: flexmark's distribution of span widths is a no-op (the span lists alias and are
        // cleared before use), so spans never widen columns.
    }

    fn span_width(&self, col: usize, span: usize) -> isize {
        if span > 1 {
            (0..span).map(|i| self.column_widths.get(i + col).copied().unwrap_or(0)).sum()
        } else {
            self.column_widths.get(col).copied().unwrap_or(0)
        }
    }

    fn cell_text(&self, cell: &Cell, is_header: bool, w: isize, alignment: Option<Align>) -> String {
        let mut text = cell.text.clone();
        let length = width(&text);
        if length < w {
            let alignment = match alignment {
                None | Some(Align::None) => {
                    if is_header {
                        Align::Center
                    } else {
                        Align::Left
                    }
                }
                Some(a) => a,
            };
            let diff = w - length;
            let space_count = diff / SPACE_WIDTH;
            match alignment {
                Align::Left => {
                    if space_count > 0 {
                        text.push_str(&" ".repeat(space_count as usize));
                    }
                }
                Align::Right => {
                    if space_count > 0 {
                        text = format!("{}{}", " ".repeat(space_count as usize), text);
                    }
                }
                Align::Center => {
                    let count = space_count / 2;
                    if space_count > 0 {
                        text = format!(
                            "{}{}{}",
                            " ".repeat(count as usize),
                            text,
                            " ".repeat((space_count - count) as usize)
                        );
                    }
                }
                Align::None => {}
            }
        }
        text
    }

    pub fn append_table(&mut self, out: &mut LineAppendable) {
        out.push_options();
        out.remove_options(F_WHITESPACE_REMOVAL);
        self.finalize_table();

        let header = std::mem::take(&mut self.header.rows);
        self.append_rows(out, &header, true);
        self.header.rows = header;

        // separator
        for j in 0..self.alignments.len() {
            let a = self.alignments[j];
            let colon = colon_count(a);
            let diff = self.column_widths[j] - colon * COLON_WIDTH - PIPE_WIDTH;
            let mut dash_count = diff / DASH_WIDTH;
            let dashes_only = dash_count.max(MIN_SEPARATOR_COLUMN_WIDTH - colon).max(MIN_SEPARATOR_DASHES);
            if dash_count < dashes_only {
                dash_count = dashes_only;
            }
            if (diff - (dash_count + 1) * DASH_WIDTH).abs() < (diff - dash_count * DASH_WIDTH).abs() {
                dash_count += 1;
            }
            if j == 0 {
                out.append_char('|');
            }
            if matches!(a, Some(Align::Left) | Some(Align::Center)) {
                out.append_char(':');
            }
            out.append_repeat('-', dash_count.max(0) as usize);
            if matches!(a, Some(Align::Right) | Some(Align::Center)) {
                out.append_char(':');
            }
            out.append_char('|');
        }
        out.line();

        let body = std::mem::take(&mut self.body.rows);
        self.append_rows(out, &body, false);
        self.body.rows = body;

        if let Some(caption) = &self.caption {
            out.pop_options();
            out.push_options();
            out.line().append("[").append(caption).append("]");
            out.line();
        }
        out.pop_options();
    }

    fn append_rows(&self, out: &mut LineAppendable, rows: &[Row], is_header: bool) {
        for row in rows {
            let mut j = 0;
            let mut j_span = 0;
            for i in 0..row.cells.len() {
                let cell = row.cell(i);
                if j == 0 {
                    out.append_char('|');
                    if pipe_needs_space_after(cell) {
                        out.append_char(' ');
                    }
                } else if pipe_needs_space_after(cell) {
                    out.append_char(' ');
                }
                let cell_alignment = if is_header && cell.alignment != Align::None {
                    Some(cell.alignment)
                } else {
                    self.alignments.get(j_span).copied().flatten()
                };
                let w = self.span_width(j_span, cell.column_span) - SPACE_PAD - PIPE_WIDTH * cell.column_span as isize;
                let text = self.cell_text(cell, is_header, w, cell_alignment);
                out.append(&text);
                j += 1;
                j_span += cell.column_span;
                if pipe_needs_space_before(cell) {
                    out.append_char(' ');
                }
                out.append_repeat('|', cell.column_span);
            }
            if j > 0 {
                out.line();
            }
        }
    }
}

fn colon_count(a: Option<Align>) -> isize {
    match a {
        Some(Align::Left) | Some(Align::Right) => 1,
        Some(Align::Center) => 2,
        _ => 0,
    }
}

fn pipe_needs_space_before(cell: &Cell) -> bool {
    cell.text == " " || !cell.text.ends_with(' ')
}

fn pipe_needs_space_after(cell: &Cell) -> bool {
    cell.text == " " || !cell.text.starts_with(' ')
}
