//! CSV by hand: RFC 4180 quoting, `\r\n`, and the guard against formulas
//! (BK-20, BK-21).

/// One cell of a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Cell {
    /// Text: guarded against formulas and quoted when needed.
    Text(String),
    /// A number the library produced: written as it is.
    Number(String),
    /// Nothing.
    Empty,
}

impl Cell {
    pub(crate) fn text(s: impl Into<String>) -> Self {
        Self::Text(s.into())
    }
}

/// BK-21: a text field that a spreadsheet could take for a formula gets a
/// `'` in front.
fn guard(s: &str) -> String {
    if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_string()
    }
}

/// BK-20: quote a field when it holds a comma, a quote, a CR or a LF.
fn quote(s: &str) -> String {
    if s.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// BK-20, BK-21: the text of a cell as written.
pub(crate) fn render(cell: &Cell) -> String {
    match cell {
        Cell::Text(s) => quote(&guard(s)),
        Cell::Number(n) => n.clone(),
        Cell::Empty => String::new(),
    }
}

/// BK-20: append a row and its `\r\n`.
pub(crate) fn push_row(out: &mut String, cells: &[Cell]) {
    let mut first = true;
    for cell in cells {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(&render(cell));
    }
    out.push_str("\r\n");
}
