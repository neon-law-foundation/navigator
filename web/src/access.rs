//! Row-level visibility helpers.
//!
//! `Role decides the tier; participation decides the scope.` This
//! module is the one place where that mapping turns into SQL:
//! [`visible_projects`] returns the slice of `projects` a given
//! [`Role`] + `person_id` should see, and every project-list / detail
//! handler funnels through it.
//!
//! See [`docs/access-model.md`](../../../docs/access-model.md).

use sea_orm::{ColumnTrait, DbErr, EntityTrait, QueryFilter, QueryOrder};
use store::entity::person::Role;
use store::entity::{person_project_role, project};
use store::Db;
use uuid::Uuid;

/// All projects this person is allowed to see.
///
/// - [`Role::Admin`] → every project, unfiltered. The bypass is silent
///   per the access-model doc; no audit row is written here.
/// - [`Role::Staff`] / [`Role::Client`] → only projects with a matching
///   `person_project_roles` row for `person_id`. A staff member who
///   has not been assigned to a matter does not see it.
///
/// `person_id` is `Option` because some test paths build sessions
/// without a linked persons row. When it's `None`, non-admin callers
/// see nothing — fail-closed.
pub async fn visible_projects(
    db: &Db,
    person_id: Option<Uuid>,
    role: Role,
) -> Result<Vec<project::Model>, DbErr> {
    if role == Role::Admin {
        return project::Entity::find()
            .order_by_asc(project::Column::Name)
            .all(db)
            .await;
    }
    let Some(pid) = person_id else {
        return Ok(Vec::new());
    };
    let memberships = person_project_role::Entity::find()
        .filter(person_project_role::Column::PersonId.eq(pid))
        .all(db)
        .await?;
    if memberships.is_empty() {
        return Ok(Vec::new());
    }
    let project_ids: Vec<Uuid> = memberships.into_iter().map(|m| m.project_id).collect();
    project::Entity::find()
        .filter(project::Column::Id.is_in(project_ids))
        .order_by_asc(project::Column::Name)
        .all(db)
        .await
}

/// `true` iff the caller may see the given project. Single-row
/// counterpart to [`visible_projects`] — same semantics, one
/// `SELECT 1` instead of loading every membership row.
///
/// Project-detail handlers call this *before* fetching the project
/// itself so an unauthorised caller never even pulls the row into
/// the response.
pub async fn can_see_project(
    db: &Db,
    person_id: Option<Uuid>,
    role: Role,
    project_id: Uuid,
) -> Result<bool, DbErr> {
    if role == Role::Admin {
        return Ok(true);
    }
    let Some(pid) = person_id else {
        return Ok(false);
    };
    let hit = person_project_role::Entity::find()
        .filter(person_project_role::Column::PersonId.eq(pid))
        .filter(person_project_role::Column::ProjectId.eq(project_id))
        .one(db)
        .await?;
    Ok(hit.is_some())
}

#[cfg(test)]
mod tests {
    use super::{can_see_project, visible_projects};
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use store::entity::person::Role;
    use store::entity::{person, person_project_role, project};
    use store::test_support::pg;
    use uuid::Uuid;

    async fn seed_project(db: &store::Db, name: &str) -> Uuid {
        project::ActiveModel {
            name: ActiveValue::Set(name.into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(store::test_support::seed_entity(db).await),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_person(db: &store::Db, email: &str) -> Uuid {
        person::ActiveModel {
            name: ActiveValue::Set(email.into()),
            email: ActiveValue::Set(email.into()),
            role: ActiveValue::Set(Role::Client),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn link(db: &store::Db, person_id: Uuid, project_id: Uuid, participation: &str) {
        person_project_role::ActiveModel {
            person_id: ActiveValue::Set(person_id),
            project_id: ActiveValue::Set(project_id),
            participation: ActiveValue::Set(participation.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn admin_sees_every_project_regardless_of_membership() {
        let db = pg().await;
        let libra = seed_person(&db, "libra@example.com").await;
        let _ = seed_project(&db, "alpha").await;
        let _ = seed_project(&db, "bravo").await;
        // Libra has no person_project_roles rows.

        let rows = visible_projects(&db, Some(libra), Role::Admin)
            .await
            .unwrap();
        assert_eq!(rows.len(), 2, "admin sees every project");
    }

    #[tokio::test]
    async fn staff_sees_only_projects_they_have_a_participation_on() {
        let db = pg().await;
        let libra = seed_person(&db, "libra@example.com").await;
        let visible = seed_project(&db, "alpha").await;
        let _hidden = seed_project(&db, "bravo").await;
        link(&db, libra, visible, "paralegal").await;

        let rows = visible_projects(&db, Some(libra), Role::Staff)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "alpha");
    }

    #[tokio::test]
    async fn client_sees_only_projects_they_have_a_participation_on() {
        let db = pg().await;
        let libra = seed_person(&db, "libra@example.com").await;
        let visible = seed_project(&db, "alpha").await;
        let _hidden = seed_project(&db, "bravo").await;
        link(&db, libra, visible, "client").await;

        let rows = visible_projects(&db, Some(libra), Role::Client)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "alpha");
    }

    #[tokio::test]
    async fn non_admin_with_no_person_id_sees_nothing() {
        let db = pg().await;
        let _ = seed_project(&db, "alpha").await;
        let rows = visible_projects(&db, None, Role::Staff).await.unwrap();
        assert!(rows.is_empty(), "missing person_id fails closed");
    }

    #[tokio::test]
    async fn admin_with_no_person_id_still_sees_everything() {
        let db = pg().await;
        let _ = seed_project(&db, "alpha").await;
        let rows = visible_projects(&db, None, Role::Admin).await.unwrap();
        assert_eq!(rows.len(), 1, "admin bypass doesn't require person_id");
    }

    #[tokio::test]
    async fn can_see_project_admin_bypass() {
        let db = pg().await;
        let p = seed_project(&db, "alpha").await;
        assert!(can_see_project(&db, None, Role::Admin, p).await.unwrap());
    }

    #[tokio::test]
    async fn can_see_project_client_with_participation() {
        let db = pg().await;
        let libra = seed_person(&db, "libra@example.com").await;
        let p = seed_project(&db, "alpha").await;
        link(&db, libra, p, "client").await;
        assert!(can_see_project(&db, Some(libra), Role::Client, p)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn can_see_project_client_without_participation() {
        let db = pg().await;
        let libra = seed_person(&db, "libra@example.com").await;
        let p = seed_project(&db, "alpha").await;
        assert!(!can_see_project(&db, Some(libra), Role::Client, p)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn can_see_project_staff_without_person_id_fails_closed() {
        let db = pg().await;
        let p = seed_project(&db, "alpha").await;
        assert!(!can_see_project(&db, None, Role::Staff, p).await.unwrap());
    }
}
