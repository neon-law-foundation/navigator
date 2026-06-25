//! Public testimonial reads.
//!
//! The website should not know the publication rules. This module is the
//! narrow seam: it returns only testimonials with explicit sender consent
//! and staff publication approval, joined to the Person and Project needed
//! for attribution.

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, RelationTrait};
use uuid::Uuid;

use crate::entity::{person, project, testimonial};
use crate::Db;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishedTestimonial {
    pub id: Uuid,
    pub project_id: Uuid,
    pub project_name: String,
    pub person_id: Uuid,
    pub person_name: String,
    pub person_title: Option<String>,
    pub profile_image_url: Option<String>,
    pub product_code: Option<String>,
    pub quote: String,
    pub attribution_label: Option<String>,
}

#[derive(Debug, sea_orm::FromQueryResult)]
struct PublishedRow {
    id: Uuid,
    project_id: Uuid,
    project_name: String,
    person_id: Uuid,
    person_name: String,
    person_title: Option<String>,
    profile_image_url: Option<String>,
    product_code: Option<String>,
    quote: String,
    attribution_label: Option<String>,
}

impl From<PublishedRow> for PublishedTestimonial {
    fn from(row: PublishedRow) -> Self {
        Self {
            id: row.id,
            project_id: row.project_id,
            project_name: row.project_name,
            person_id: row.person_id,
            person_name: row.person_name,
            person_title: row.person_title,
            profile_image_url: row.profile_image_url,
            product_code: row.product_code,
            quote: row.quote,
            attribution_label: row.attribution_label,
        }
    }
}

/// Published testimonials for the homepage, across every product.
pub async fn published_for_home(
    db: &Db,
    limit: u64,
) -> Result<Vec<PublishedTestimonial>, sea_orm::DbErr> {
    published_query()
        .order_by_asc(testimonial::Column::DisplayOrder)
        .order_by_desc(testimonial::Column::PublishedAt)
        .limit(limit)
        .into_model::<PublishedRow>()
        .all(db)
        .await
        .map(|rows| rows.into_iter().map(Into::into).collect())
}

/// Published testimonials tagged for one product (`nexus`, `litigation`,
/// etc.). Used by the corresponding `/services/<product>` page.
pub async fn published_for_product(
    db: &Db,
    product_code: &str,
    limit: u64,
) -> Result<Vec<PublishedTestimonial>, sea_orm::DbErr> {
    published_query()
        .filter(testimonial::Column::ProductCode.eq(product_code))
        .order_by_asc(testimonial::Column::DisplayOrder)
        .order_by_desc(testimonial::Column::PublishedAt)
        .limit(limit)
        .into_model::<PublishedRow>()
        .all(db)
        .await
        .map(|rows| rows.into_iter().map(Into::into).collect())
}

fn published_query() -> sea_orm::Select<testimonial::Entity> {
    testimonial::Entity::find()
        .filter(testimonial::Column::ConsentedAt.is_not_null())
        .filter(testimonial::Column::PublishedAt.is_not_null())
        .join(
            sea_orm::JoinType::InnerJoin,
            testimonial::Relation::Person.def(),
        )
        .join(
            sea_orm::JoinType::InnerJoin,
            testimonial::Relation::Project.def(),
        )
        .select_only()
        .column(testimonial::Column::Id)
        .column(testimonial::Column::ProjectId)
        .column(testimonial::Column::PersonId)
        .column(testimonial::Column::ProductCode)
        .column(testimonial::Column::Quote)
        .column(testimonial::Column::AttributionLabel)
        .column_as(person::Column::Name, "person_name")
        .column_as(person::Column::Title, "person_title")
        .column_as(person::Column::ProfileImageUrl, "profile_image_url")
        .column_as(project::Column::Name, "project_name")
}

#[cfg(test)]
mod tests {
    use sea_orm::{ActiveModelTrait, ActiveValue};

    use super::{published_for_home, published_for_product};
    use crate::entity::{person, product, project, testimonial};
    use crate::test_support::{dri_person, pg, seed_entity};

    async fn fixture() -> crate::Db {
        let db = pg().await;
        let entity_id = seed_entity(&db).await;
        let dri = dri_person(&db).await;
        product::ActiveModel {
            code: ActiveValue::Set("nexus".into()),
            display_name: ActiveValue::Set("Neon Law Nexus".into()),
            list_price_cents: ActiveValue::Set(222_200),
            currency: ActiveValue::Set("USD".into()),
            cadence: ActiveValue::Set("monthly".into()),
            billing_kind: ActiveValue::Set(product::BILLING_KIND_RECURRING.into()),
            account_code: ActiveValue::Set("200".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        product::ActiveModel {
            code: ActiveValue::Set("litigation".into()),
            display_name: ActiveValue::Set("1337 Lawyers".into()),
            list_price_cents: ActiveValue::Set(133_700),
            currency: ActiveValue::Set("USD".into()),
            cadence: ActiveValue::Set("hourly".into()),
            billing_kind: ActiveValue::Set(product::BILLING_KIND_HOURLY.into()),
            account_code: ActiveValue::Set("200".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let sender = person::ActiveModel {
            name: ActiveValue::Set("A. Client".into()),
            email: ActiveValue::Set("testimonial@example.com".into()),
            title: ActiveValue::Set(Some("Founder".into())),
            profile_image_url: ActiveValue::Set(Some("/images/testimonial.webp".into())),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let project = project::ActiveModel {
            name: ActiveValue::Set("Published matter".into()),
            status: ActiveValue::Set("closed".into()),
            entity_id: ActiveValue::Set(entity_id),
            staff_dri_person_id: ActiveValue::Set(Some(dri)),
            client_dri_person_id: ActiveValue::Set(Some(dri)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        for (product_code, quote, published_at, order) in [
            (
                "nexus",
                "Nexus kept our legal work moving.",
                Some("2026-06-24T00:00:00Z"),
                2,
            ),
            (
                "litigation",
                "Litigation counsel gave us leverage.",
                Some("2026-06-24T00:00:00Z"),
                1,
            ),
            ("nexus", "Draft quote.", None, 0),
        ] {
            testimonial::ActiveModel {
                project_id: ActiveValue::Set(project.id),
                person_id: ActiveValue::Set(sender.id),
                product_code: ActiveValue::Set(Some(product_code.into())),
                quote: ActiveValue::Set(quote.into()),
                attribution_label: ActiveValue::Set(None),
                consented_at: ActiveValue::Set(Some("2026-06-23T00:00:00Z".into())),
                published_at: ActiveValue::Set(published_at.map(str::to_string)),
                display_order: ActiveValue::Set(order),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        db
    }

    #[tokio::test]
    async fn published_reads_require_consent_and_publication() {
        let db = fixture().await;
        let rows = published_for_home(&db, 10).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].quote, "Litigation counsel gave us leverage.");
        assert_eq!(rows[0].project_name, "Published matter");
        assert_eq!(rows[0].person_title.as_deref(), Some("Founder"));
        assert_eq!(
            rows[0].profile_image_url.as_deref(),
            Some("/images/testimonial.webp")
        );
        assert!(!rows.iter().any(|row| row.quote == "Draft quote."));
    }

    #[tokio::test]
    async fn product_reads_only_return_that_product() {
        let db = fixture().await;
        let nexus = published_for_product(&db, "nexus", 10).await.unwrap();
        assert_eq!(nexus.len(), 1);
        assert_eq!(nexus[0].quote, "Nexus kept our legal work moving.");
        assert_eq!(nexus[0].product_code.as_deref(), Some("nexus"));
    }
}
