// `navigator erd` — introspect the migrated schema in Postgres and
// emit a Mermaid `erDiagram` block on stdout. The output renders
// directly in GitHub markdown and any Mermaid-aware viewer.
//
// With `--format svg`, emits a hand-written SVG instead. The SVG
// renderer is **deterministic by construction**: integer-only
// arithmetic, alphabetical iteration via [`BTreeMap`], no timestamps,
// no random IDs. Same schema in → byte-identical SVG out. That
// invariant is asserted by `cli/tests/erd_svg.rs`.
//
// We introspect via `information_schema` (column metadata) and
// `pg_catalog` (foreign-key references). Both are ANSI-ish Postgres
// surfaces; nothing in this module depends on SQLite.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use anyhow::Context;
use clap::ValueEnum;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    /// Mermaid `erDiagram` block (GitHub renders natively).
    Mermaid,
    /// Hand-written SVG, deterministic across runs.
    Svg,
}

#[derive(Clone)]
struct Column {
    name: String,
    ty: String,
    primary_key: bool,
}

#[derive(Clone)]
struct ForeignKey {
    from_column: String,
    to_table: String,
}

/// Introspected schema: table name → (columns, foreign keys). Sorted
/// alphabetically by table name (the [`BTreeMap`] guarantee), which
/// is the load-bearing property for deterministic SVG output.
type Schema = BTreeMap<String, (Vec<Column>, Vec<ForeignKey>)>;

pub async fn run(db: &DatabaseConnection, format: OutputFormat) -> anyhow::Result<()> {
    let schema = fetch_schema(db).await?;
    let out = match format {
        OutputFormat::Mermaid => render_mermaid(&schema),
        OutputFormat::Svg => render_svg(&schema),
    };
    println!("{out}");
    Ok(())
}

async fn fetch_schema(db: &DatabaseConnection) -> anyhow::Result<Schema> {
    let tables = list_tables(db).await.context("listing tables")?;
    let mut schema = Schema::new();
    for table in &tables {
        let cols = column_info(db, table)
            .await
            .with_context(|| format!("column_info({table})"))?;
        let fks = foreign_keys(db, table)
            .await
            .with_context(|| format!("foreign_keys({table})"))?;
        schema.insert(table.clone(), (cols, fks));
    }
    Ok(schema)
}

fn render_mermaid(schema: &Schema) -> String {
    let mut out = String::from("erDiagram\n");

    let fk_columns: BTreeMap<&str, Vec<&str>> = schema
        .iter()
        .map(|(table, (_, fks))| {
            (
                table.as_str(),
                fks.iter().map(|f| f.from_column.as_str()).collect(),
            )
        })
        .collect();

    for (table, (cols, _)) in schema {
        let _ = writeln!(out, "    {table} {{");
        for col in cols {
            let role = if col.primary_key {
                " PK"
            } else if fk_columns
                .get(table.as_str())
                .is_some_and(|fks| fks.contains(&col.name.as_str()))
            {
                " FK"
            } else {
                ""
            };
            let ty = if col.ty.is_empty() {
                "TEXT"
            } else {
                col.ty.as_str()
            };
            let _ = writeln!(out, "        {ty} {name}{role}", name = col.name);
        }
        let _ = writeln!(out, "    }}");
    }

    for (table, (_, fks)) in schema {
        for fk in fks {
            let _ = writeln!(
                out,
                "    {parent} ||--o{{ {child} : \"{col}\"",
                parent = fk.to_table,
                child = table,
                col = fk.from_column,
            );
        }
    }
    out
}

// === SVG renderer (deterministic) =========================================
//
// Layout: alphabetical row-major grid, 4 columns wide. Per-column widths
// scale with content; per-row heights scale with the tallest table in
// that row. Edges are straight lines from the FK column's right midpoint
// to the parent table's left edge at the title bar's midpoint. All
// dimensions are integers; all iteration goes through [`BTreeMap`] /
// [`BTreeSet`] so order is deterministic. No timestamps, no random IDs.

const CHAR_WIDTH: i32 = 8;
const ROW_HEIGHT: i32 = 22;
const TITLE_HEIGHT: i32 = 30;
const CELL_PAD: i32 = 12;
const CELL_GAP_X: i32 = 40;
const CELL_GAP_Y: i32 = 24;
const GRID_COLS: usize = 4;
const MARGIN: i32 = 30;
const FONT_SIZE: i32 = 13;

struct PlacedTable<'a> {
    name: &'a str,
    cols: &'a [Column],
    fk_columns: BTreeSet<&'a str>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

struct Layout<'a> {
    tables: Vec<PlacedTable<'a>>,
    canvas_w: i32,
    canvas_h: i32,
}

/// Cast a `usize` to `i32` for layout math. Every input is a small
/// count (table count, column index, grid dimension) bounded well
/// under `i32::MAX` in practice; the cast is deliberate because we
/// want integer arithmetic for byte-deterministic SVG output.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
const fn i32_of(n: usize) -> i32 {
    n as i32
}

fn normalize_pg_type(t: &str) -> String {
    let lower = t.to_lowercase();
    lower
        .replace("character varying", "varchar")
        .replace("timestamp with time zone", "timestamptz")
        .replace("timestamp without time zone", "timestamp")
        .replace("double precision", "double")
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

fn natural_table_size(name: &str, cols: &[Column], fk_set: &BTreeSet<&str>) -> (i32, i32) {
    let title_chars = i32_of(name.chars().count());
    let mut max_row_chars = title_chars;
    for c in cols {
        let role_len = if c.primary_key || fk_set.contains(c.name.as_str()) {
            3 // " PK" or " FK"
        } else {
            0
        };
        let ty_norm = normalize_pg_type(&c.ty);
        // "<name>  <type><role>" — two spaces between name and type for
        // breathing room in the rendered text.
        let chars = i32_of(c.name.chars().count()) + 2 + i32_of(ty_norm.chars().count()) + role_len;
        if chars > max_row_chars {
            max_row_chars = chars;
        }
    }
    let w = max_row_chars * CHAR_WIDTH + 2 * CELL_PAD;
    let h = TITLE_HEIGHT + ROW_HEIGHT * i32_of(cols.len());
    (w, h)
}

struct Sized<'a> {
    name: &'a str,
    cols: &'a [Column],
    fk_set: BTreeSet<&'a str>,
    w: i32,
    h: i32,
}

fn compute_layout(schema: &Schema) -> Layout<'_> {
    // First pass: compute every table's natural size and its FK column
    // set (needed both for size calculation and for later rendering).
    let mut sized: Vec<Sized<'_>> = Vec::new();
    for (name, (cols, fks)) in schema {
        let fk_set: BTreeSet<&str> = fks.iter().map(|f| f.from_column.as_str()).collect();
        let (w, h) = natural_table_size(name, cols, &fk_set);
        sized.push(Sized {
            name: name.as_str(),
            cols: cols.as_slice(),
            fk_set,
            w,
            h,
        });
    }

    // Second pass: per-grid-column widths and per-grid-row heights.
    let row_count = sized.len().div_ceil(GRID_COLS);
    let mut col_widths = [0i32; GRID_COLS];
    let mut row_heights = vec![0i32; row_count];
    for (i, s) in sized.iter().enumerate() {
        let col = i % GRID_COLS;
        let row = i / GRID_COLS;
        if s.w > col_widths[col] {
            col_widths[col] = s.w;
        }
        if s.h > row_heights[row] {
            row_heights[row] = s.h;
        }
    }

    // Third pass: place each table at its grid cell's top-left.
    let mut tables: Vec<PlacedTable<'_>> = Vec::with_capacity(sized.len());
    for (i, s) in sized.into_iter().enumerate() {
        let col = i % GRID_COLS;
        let row = i / GRID_COLS;
        let x = MARGIN + (0..col).map(|c| col_widths[c] + CELL_GAP_X).sum::<i32>();
        let y = MARGIN + (0..row).map(|r| row_heights[r] + CELL_GAP_Y).sum::<i32>();
        tables.push(PlacedTable {
            name: s.name,
            cols: s.cols,
            fk_columns: s.fk_set,
            x,
            y,
            w: s.w,
            h: s.h,
        });
    }

    let canvas_w =
        MARGIN * 2 + col_widths.iter().sum::<i32>() + CELL_GAP_X * (i32_of(GRID_COLS) - 1);
    let canvas_h = MARGIN * 2
        + row_heights.iter().sum::<i32>()
        + CELL_GAP_Y * (i32_of(row_heights.len()) - 1).max(0);

    Layout {
        tables,
        canvas_w,
        canvas_h,
    }
}

fn emit_edges(out: &mut String, schema: &Schema, by_name: &BTreeMap<&str, &PlacedTable<'_>>) {
    out.push_str(r#"<g class="edges">"#);
    out.push('\n');
    for (name, (_, fks)) in schema {
        let Some(src) = by_name.get(name.as_str()) else {
            continue;
        };
        for fk in fks {
            let Some(row_idx) = src.cols.iter().position(|c| c.name == fk.from_column) else {
                continue;
            };
            let Some(parent) = by_name.get(fk.to_table.as_str()) else {
                continue;
            };
            let x1 = src.x + src.w;
            let y1 = src.y + TITLE_HEIGHT + ROW_HEIGHT * i32_of(row_idx) + ROW_HEIGHT / 2;
            let x2 = parent.x;
            let y2 = parent.y + TITLE_HEIGHT / 2;
            let _ = writeln!(
                out,
                r#"<path class="e" d="M{x1},{y1} L{x2},{y2}" marker-end="url(#arrow)"/>"#
            );
        }
    }
    out.push_str("</g>\n");
}

fn emit_table(out: &mut String, p: &PlacedTable<'_>) {
    let _ = writeln!(out, r#"<g transform="translate({},{})">"#, p.x, p.y);
    let _ = writeln!(out, r#"<rect class="t" width="{}" height="{}"/>"#, p.w, p.h);
    let _ = writeln!(
        out,
        r#"<rect class="tt" width="{}" height="{}"/>"#,
        p.w, TITLE_HEIGHT
    );
    let _ = writeln!(
        out,
        r#"<text class="tn" x="{}" y="{}" text-anchor="middle">{}</text>"#,
        p.w / 2,
        TITLE_HEIGHT - 10,
        xml_escape(p.name)
    );
    for (i, col) in p.cols.iter().enumerate() {
        let y = TITLE_HEIGHT + ROW_HEIGHT * i32_of(i) + ROW_HEIGHT - 7;
        let ty_norm = normalize_pg_type(&col.ty);
        let (suffix, class) = if col.primary_key {
            (" PK", "cpk")
        } else if p.fk_columns.contains(col.name.as_str()) {
            (" FK", "cfk")
        } else {
            ("", "ct")
        };
        let _ = writeln!(
            out,
            r#"<text class="cn" x="{}" y="{}">{}</text>"#,
            CELL_PAD,
            y,
            xml_escape(&col.name)
        );
        let _ = writeln!(
            out,
            r#"<text class="{}" x="{}" y="{}" text-anchor="end">{}{}</text>"#,
            class,
            p.w - CELL_PAD,
            y,
            xml_escape(&ty_norm),
            xml_escape(suffix),
        );
    }
    out.push_str("</g>\n");
}

fn render_svg(schema: &Schema) -> String {
    let layout = compute_layout(schema);
    let Layout {
        tables,
        canvas_w,
        canvas_h,
    } = &layout;

    let by_name: BTreeMap<&str, &PlacedTable<'_>> = tables.iter().map(|p| (p.name, p)).collect();

    let mut out = String::new();
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{canvas_w}" height="{canvas_h}" viewBox="0 0 {canvas_w} {canvas_h}" font-family="ui-monospace, SFMono-Regular, Menlo, monospace" font-size="{FONT_SIZE}">"#,
    );
    out.push_str(
        r##"<defs><marker id="arrow" markerWidth="10" markerHeight="10" refX="9" refY="3" orient="auto" markerUnits="strokeWidth"><path d="M0,0 L0,6 L9,3 z" fill="#666"/></marker></defs>
"##,
    );
    out.push_str(
        "<style>.t{fill:#fff;stroke:#666;stroke-width:1}.tt{fill:#eef;stroke:#666;stroke-width:1}.tn{fill:#222;font-weight:600}.cn{fill:#222}.ct{fill:#888}.cpk{fill:#c33;font-weight:600}.cfk{fill:#36c}.e{fill:none;stroke:#888;stroke-width:1.2}</style>\n",
    );

    emit_edges(&mut out, schema, &by_name);

    out.push_str(r#"<g class="tables">"#);
    out.push('\n');
    for p in tables {
        emit_table(&mut out, p);
    }
    out.push_str("</g>\n");

    out.push_str("</svg>\n");
    out
}

async fn list_tables(db: &DatabaseConnection) -> anyhow::Result<Vec<String>> {
    let stmt = Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT tablename FROM pg_tables \
         WHERE schemaname = current_schema() \
         AND tablename != 'seaql_migrations' \
         ORDER BY tablename"
            .to_string(),
    );
    let rows = db.query_all(stmt).await?;
    rows.into_iter()
        .map(|r| {
            r.try_get::<String>("", "tablename")
                .map_err(anyhow::Error::from)
        })
        .collect()
}

async fn column_info(db: &DatabaseConnection, table: &str) -> anyhow::Result<Vec<Column>> {
    // `information_schema.columns` covers every backend; for the PK
    // role we join against `pg_index` so single- and composite-PK
    // columns light up correctly.
    let cols_stmt = Statement::from_string(
        DatabaseBackend::Postgres,
        format!(
            "SELECT column_name, data_type \
             FROM information_schema.columns \
             WHERE table_schema = current_schema() AND table_name = '{table}' \
             ORDER BY ordinal_position"
        ),
    );
    let pk_stmt = Statement::from_string(
        DatabaseBackend::Postgres,
        format!(
            "SELECT a.attname AS column_name \
             FROM pg_index i \
             JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) \
             WHERE i.indrelid = (current_schema() || '.\"{table}\"')::regclass \
             AND i.indisprimary"
        ),
    );
    let cols = db.query_all(cols_stmt).await?;
    let pk_rows = db.query_all(pk_stmt).await?;
    let pk_set: std::collections::HashSet<String> = pk_rows
        .into_iter()
        .filter_map(|r| r.try_get::<String>("", "column_name").ok())
        .collect();
    let mut out = Vec::with_capacity(cols.len());
    for row in cols {
        let name: String = row.try_get("", "column_name")?;
        let ty: String = row.try_get("", "data_type")?;
        let primary_key = pk_set.contains(&name);
        out.push(Column {
            name,
            ty: ty.to_uppercase(),
            primary_key,
        });
    }
    Ok(out)
}

async fn foreign_keys(db: &DatabaseConnection, table: &str) -> anyhow::Result<Vec<ForeignKey>> {
    // `pg_catalog`'s constraint introspection: pull every FK on
    // `table` along with the column it points at and the parent
    // table's name. `pg_index`/`pg_constraint` surfaces this without
    // a join into `information_schema`'s nine-table maze.
    let stmt = Statement::from_string(
        DatabaseBackend::Postgres,
        format!(
            "SELECT \
               a.attname AS from_column, \
               cl2.relname AS to_table \
             FROM pg_constraint con \
             JOIN pg_class cl ON cl.oid = con.conrelid \
             JOIN pg_namespace ns ON ns.oid = cl.relnamespace \
             JOIN pg_attribute a ON a.attrelid = cl.oid AND a.attnum = ANY(con.conkey) \
             JOIN pg_class cl2 ON cl2.oid = con.confrelid \
             WHERE con.contype = 'f' \
               AND ns.nspname = current_schema() \
               AND cl.relname = '{table}' \
             ORDER BY a.attnum"
        ),
    );
    let rows = db.query_all(stmt).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let from_column: String = row.try_get("", "from_column")?;
        let to_table: String = row.try_get("", "to_table")?;
        out.push(ForeignKey {
            from_column,
            to_table,
        });
    }
    Ok(out)
}

// Tests for the SVG renderer use synthetic schemas — pure-function
// checks that don't require a database.
#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str, pk: bool) -> Column {
        Column {
            name: name.into(),
            ty: ty.into(),
            primary_key: pk,
        }
    }

    fn fk(from: &str, to: &str) -> ForeignKey {
        ForeignKey {
            from_column: from.into(),
            to_table: to.into(),
        }
    }

    #[test]
    fn render_svg_is_byte_identical_across_runs() {
        let mut schema = Schema::new();
        schema.insert(
            "parents".into(),
            (
                vec![col("id", "uuid", true), col("name", "varchar", false)],
                vec![],
            ),
        );
        schema.insert(
            "children".into(),
            (
                vec![
                    col("id", "uuid", true),
                    col("parent_id", "uuid", false),
                    col("note", "text", false),
                ],
                vec![fk("parent_id", "parents")],
            ),
        );
        let first = render_svg(&schema);
        let second = render_svg(&schema);
        let third = render_svg(&schema);
        assert_eq!(first, second);
        assert_eq!(second, third);
    }

    #[test]
    fn render_svg_emits_expected_structure() {
        let mut schema = Schema::new();
        schema.insert(
            "people".into(),
            (
                vec![col("id", "uuid", true), col("name", "varchar", false)],
                vec![],
            ),
        );
        let svg = render_svg(&schema);
        assert!(svg.starts_with("<svg "));
        assert!(svg.ends_with("</svg>\n"));
        assert!(
            svg.contains(">people<"),
            "table name should appear in output"
        );
        assert!(svg.contains(">id<"), "id column should appear");
        assert!(svg.contains(" PK<"), "PK marker should appear");
    }

    #[test]
    fn xml_escape_handles_all_five_entities() {
        assert_eq!(xml_escape("<a>"), "&lt;a&gt;");
        assert_eq!(xml_escape("\"&'"), "&quot;&amp;&apos;");
    }

    #[test]
    fn normalize_pg_type_shortens_long_names() {
        assert_eq!(normalize_pg_type("CHARACTER VARYING"), "varchar");
        assert_eq!(normalize_pg_type("timestamp with time zone"), "timestamptz");
        assert_eq!(normalize_pg_type("UUID"), "uuid");
    }
}
