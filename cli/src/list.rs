//! List commands: read each entity type from the database and print
//! a stable tabular form to stdout. One function per entity so each
//! has its own header and column order.
//!
//! Rendering goes through [`comfy_table`] for layout and
//! [`crate::palette`] for color. comfy-table's `tty` feature handles
//! the "drop ANSI when not a terminal" check for the table itself;
//! the palette helpers do the same for header/summary text.

use comfy_table::{presets::UTF8_FULL, Cell, Color, ContentArrangement, Table};

use crate::palette::{self, CYAN_300, CYAN_500};

fn header_color() -> Color {
    Color::Rgb {
        r: CYAN_300.0,
        g: CYAN_300.1,
        b: CYAN_300.2,
    }
}

fn highlight_color() -> Color {
    Color::Rgb {
        r: CYAN_500.0,
        g: CYAN_500.1,
        b: CYAN_500.2,
    }
}

fn fresh_table(headers: &[&str]) -> Table {
    let mut table = Table::new();
    table
        .load_style(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(headers.iter().map(|h| {
            Cell::new(h)
                .fg(header_color())
                .add_attribute(comfy_table::Attribute::Bold)
        }));
    table
}

/// Cell styled as the primary identifier (cyan-500). Used for the
/// leftmost column of every table.
fn id_cell<T: std::fmt::Display>(value: T) -> Cell {
    Cell::new(value).fg(highlight_color())
}

fn print_empty(noun: &str) {
    println!(
        "{}",
        palette::dim(format!("0 rows — no {noun} in this database."))
    );
}

fn print_summary(count: usize) {
    println!();
    println!("{}", palette::dim(format!("{count} row(s).")));
}

pub async fn list_questions(surreal: &store::surreal::SurrealDb) -> anyhow::Result<()> {
    let rows = store::questions::list_all(surreal).await?;
    if rows.is_empty() {
        print_empty("questions");
        return Ok(());
    }
    let mut table = fresh_table(&["code", "answer_type", "prompt"]);
    for q in &rows {
        table.add_row(vec![
            id_cell(&q.code),
            Cell::new(&q.answer_type),
            Cell::new(&q.prompt),
        ]);
    }
    println!("{table}");
    print_summary(rows.len());
    Ok(())
}

pub async fn list_templates(surreal: &store::surreal::SurrealDb) -> anyhow::Result<()> {
    // The public catalog is the workspace-shared templates; project-
    // scoped ones are hidden here. The table lives in `SurrealDB` since
    // ENG-121, and `list_current` returns both scopes in code order.
    let rows: Vec<_> = store::templates::list_current(surreal)
        .await?
        .into_iter()
        .filter(|t| t.project_id.is_none())
        .collect();
    if rows.is_empty() {
        print_empty("templates");
        return Ok(());
    }
    let mut table = fresh_table(&["code", "respondent_type", "title"]);
    for t in &rows {
        table.add_row(vec![
            id_cell(&t.code),
            Cell::new(&t.respondent_type),
            Cell::new(&t.title),
        ]);
    }
    println!("{table}");
    print_summary(rows.len());
    Ok(())
}

pub async fn list_jurisdictions(surreal: &store::surreal::SurrealDb) -> anyhow::Result<()> {
    // The table lives in SurrealDB (ENG-20); the listing keeps its
    // code-ascending order by sorting the name-ordered read.
    let mut rows = store::jurisdictions::list_all(surreal).await?;
    rows.sort_by(|a, b| a.code.cmp(&b.code));
    if rows.is_empty() {
        print_empty("jurisdictions");
        return Ok(());
    }
    let mut table = fresh_table(&["code", "name"]);
    for j in &rows {
        table.add_row(vec![id_cell(&j.code), Cell::new(&j.name)]);
    }
    println!("{table}");
    print_summary(rows.len());
    Ok(())
}

pub async fn list_persons(surreal: &store::surreal::SurrealDb) -> anyhow::Result<()> {
    let mut rows = store::persons::list_directory(surreal, "", "", &[]).await?;
    rows.sort_by(|a, b| a.email.cmp(&b.email));
    if rows.is_empty() {
        print_empty("persons");
        return Ok(());
    }
    let mut table = fresh_table(&["email", "name", "role"]);
    for p in &rows {
        table.add_row(vec![
            id_cell(&p.email),
            Cell::new(&p.name),
            Cell::new(p.role.as_str()),
        ]);
    }
    println!("{table}");
    print_summary(rows.len());
    Ok(())
}

pub async fn list_entities(surreal: &store::surreal::SurrealDb) -> anyhow::Result<()> {
    // The table lives in SurrealDB (ENG-120); `all` orders by name.
    let rows = store::entities::all(surreal).await?;
    if rows.is_empty() {
        print_empty("entities");
        return Ok(());
    }
    let mut table = fresh_table(&["name", "et_id", "jur_id"]);
    for e in &rows {
        table.add_row(vec![
            id_cell(&e.name),
            Cell::new(e.entity_type_id),
            Cell::new(e.jurisdiction_id),
        ]);
    }
    println!("{table}");
    print_summary(rows.len());
    Ok(())
}

pub async fn list_entity_types(surreal: &store::surreal::SurrealDb) -> anyhow::Result<()> {
    // The table lives in SurrealDB (ENG-20); `list` orders by name.
    let rows = store::entity_types::list(surreal, &[]).await?;
    if rows.is_empty() {
        print_empty("entity_types");
        return Ok(());
    }
    let mut table = fresh_table(&["name"]);
    for et in &rows {
        table.add_row(vec![id_cell(&et.name)]);
    }
    println!("{table}");
    print_summary(rows.len());
    Ok(())
}

pub async fn list_projects(surreal: &store::surreal::SurrealDb) -> anyhow::Result<()> {
    let rows = store::projects::all(surreal).await?;
    if rows.is_empty() {
        print_empty("projects");
        return Ok(());
    }
    let mut table = fresh_table(&["code", "name", "status"]);
    for p in &rows {
        table.add_row(vec![
            id_cell(&p.code),
            Cell::new(&p.name),
            Cell::new(&p.status),
        ]);
    }
    println!("{table}");
    print_summary(rows.len());
    Ok(())
}

pub async fn list_letters(db: &store::surreal::SurrealDb) -> anyhow::Result<()> {
    let rows = store::letters::list_all(db).await?;
    if rows.is_empty() {
        print_empty("letters");
        return Ok(());
    }
    let mut table = fresh_table(&["direction", "sender", "subject"]);
    for l in &rows {
        table.add_row(vec![
            id_cell(&l.direction),
            Cell::new(&l.sender),
            Cell::new(&l.summary),
        ]);
    }
    println!("{table}");
    print_summary(rows.len());
    Ok(())
}
