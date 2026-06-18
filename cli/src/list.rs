//! List commands: read each entity type from the database and print
//! a stable tabular form to stdout. One function per entity so each
//! has its own header and column order.
//!
//! Rendering goes through [`comfy_table`] for layout and
//! [`crate::palette`] for color. comfy-table's `tty` feature handles
//! the "drop ANSI when not a terminal" check for the table itself;
//! the palette helpers do the same for header/summary text.

use comfy_table::{presets::UTF8_FULL, Cell, Color, ContentArrangement, Table};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use store::entity::{
    entity as entities, entity_type, jurisdiction, letter, person, project, question, template,
};

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
        .load_preset(UTF8_FULL)
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

pub async fn list_questions(db: &DatabaseConnection) -> anyhow::Result<()> {
    let rows = question::Entity::find()
        .order_by_asc(question::Column::Code)
        .all(db)
        .await?;
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

pub async fn list_templates(db: &DatabaseConnection) -> anyhow::Result<()> {
    // The public catalog is the workspace-shared templates; project-
    // scoped templates (`project_id IS NOT NULL`) are hidden here.
    let rows = template::Entity::find()
        .filter(template::Column::ProjectId.is_null())
        .order_by_asc(template::Column::Code)
        .all(db)
        .await?;
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

pub async fn list_jurisdictions(db: &DatabaseConnection) -> anyhow::Result<()> {
    let rows = jurisdiction::Entity::find()
        .order_by_asc(jurisdiction::Column::Code)
        .all(db)
        .await?;
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

pub async fn list_persons(db: &DatabaseConnection) -> anyhow::Result<()> {
    let rows = person::Entity::find()
        .order_by_asc(person::Column::Email)
        .all(db)
        .await?;
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

pub async fn list_entities(db: &DatabaseConnection) -> anyhow::Result<()> {
    let rows = entities::Entity::find()
        .order_by_asc(entities::Column::Name)
        .all(db)
        .await?;
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

pub async fn list_entity_types(db: &DatabaseConnection) -> anyhow::Result<()> {
    let rows = entity_type::Entity::find()
        .order_by_asc(entity_type::Column::Name)
        .all(db)
        .await?;
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

pub async fn list_projects(db: &DatabaseConnection) -> anyhow::Result<()> {
    let rows = project::Entity::find()
        .order_by_asc(project::Column::Name)
        .all(db)
        .await?;
    if rows.is_empty() {
        print_empty("projects");
        return Ok(());
    }
    let mut table = fresh_table(&["name", "status"]);
    for p in &rows {
        table.add_row(vec![id_cell(&p.name), Cell::new(&p.status)]);
    }
    println!("{table}");
    print_summary(rows.len());
    Ok(())
}

pub async fn list_letters(db: &DatabaseConnection) -> anyhow::Result<()> {
    let rows = letter::Entity::find()
        .order_by_asc(letter::Column::Id)
        .all(db)
        .await?;
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
