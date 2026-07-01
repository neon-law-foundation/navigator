//! Database access for the Neon Law Navigator data layer.
//!
//! Built on SeaORM (Postgres backend). SeaORM handles pooling,
//! prepared statements, and migrations; we just expose a thin
//! `connect` + `ping` API the router state depends on.

use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use sea_orm_migration::MigratorTrait;

use crate::{migration::Migrator, DbConfig};

/// The handle every router consumer shares. `DatabaseConnection` is
/// `Clone` (Arc-backed) so it drops into axum's `State` directly.
pub type Db = DatabaseConnection;

/// Open the connection described by `cfg`.
pub async fn connect(cfg: &DbConfig) -> Result<Db, DbErr> {
    let mut opts = ConnectOptions::new(cfg.to_url());
    opts.sqlx_logging(false);
    Database::connect(opts).await
}

/// One-line liveness probe used by `/health`. Returns the SeaORM
/// error verbatim so callers can log it and decide on the response.
pub async fn ping(db: &Db) -> Result<(), DbErr> {
    db.ping().await
}

/// Bring the database forward to the latest schema. Idempotent.
pub async fn migrate(db: &Db) -> Result<(), DbErr> {
    Migrator::up(db, None).await
}

#[cfg(test)]
mod tests {
    use super::{migrate, ping};
    use crate::entity::person;
    use crate::test_support::pg;
    use sea_orm::{
        ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    };

    #[tokio::test]
    async fn fresh_postgres_pings_ok() {
        let db = pg().await;
        ping(&db).await.expect("ping should succeed");
    }

    #[tokio::test]
    async fn closing_a_clone_makes_subsequent_pings_fail() {
        let db = pg().await;
        // SeaORM's `DatabaseConnection` is Arc-backed; closing any
        // clone tears down the shared pool.
        db.clone().close().await.unwrap();
        assert!(ping(&db).await.is_err());
    }

    #[tokio::test]
    async fn migrate_creates_persons_table_and_allows_insert_and_query() {
        let db = pg().await;

        let libra = person::ActiveModel {
            name: ActiveValue::Set("Libra".into()),
            email: ActiveValue::Set("libra@example.com".into()),
            ..Default::default()
        };
        libra.insert(&db).await.expect("insert should succeed");

        let all = person::Entity::find()
            .all(&db)
            .await
            .expect("query should succeed");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "Libra");
        assert_eq!(all[0].email, "libra@example.com");
    }

    #[tokio::test]
    async fn migrate_is_idempotent() {
        let db = pg().await;
        // Running again must not error — schema already exists.
        migrate(&db).await.unwrap();
    }

    #[tokio::test]
    async fn migrate_creates_jurisdiction_entity_type_and_entity_tables() {
        use crate::entity::{entity, entity_type, jurisdiction};
        let db = pg().await;

        // Seed a jurisdiction + entity_type, then create an entity that
        // foreign-keys into both.
        let nv = jurisdiction::ActiveModel {
            name: ActiveValue::Set("Nevada".into()),
            code: ActiveValue::Set("NV".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let llc = entity_type::ActiveModel {
            name: ActiveValue::Set("LLC".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        entity::ActiveModel {
            name: ActiveValue::Set("Acme Holdings".into()),
            entity_type_id: ActiveValue::Set(llc.id),
            jurisdiction_id: ActiveValue::Set(nv.id),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let all = entity::Entity::find().all(&db).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "Acme Holdings");
        assert_eq!(all[0].entity_type_id, llc.id);
        assert_eq!(all[0].jurisdiction_id, nv.id);
    }

    #[tokio::test]
    async fn jurisdiction_code_is_unique() {
        use crate::entity::jurisdiction;
        let db = pg().await;

        let make = |code: &str| jurisdiction::ActiveModel {
            name: ActiveValue::Set("X".into()),
            code: ActiveValue::Set(code.into()),
            ..Default::default()
        };
        make("NV").insert(&db).await.unwrap();
        let err = make("NV").insert(&db).await;
        assert!(err.is_err(), "duplicate jurisdiction code must be rejected");
    }

    #[tokio::test]
    async fn entity_type_name_is_unique() {
        use crate::entity::entity_type;
        let db = pg().await;

        let make = |name: &str| entity_type::ActiveModel {
            name: ActiveValue::Set(name.into()),
            ..Default::default()
        };
        make("LLC").insert(&db).await.unwrap();
        let err = make("LLC").insert(&db).await;
        assert!(err.is_err(), "duplicate entity_type name must be rejected");
    }

    #[tokio::test]
    async fn workflow_migration_creates_template_question_answer_notation() {
        use crate::entity::{answer, notation, project, question, template};
        let db = pg().await;

        let libra = crate::entity::person::ActiveModel {
            name: ActiveValue::Set("Libra".into()),
            email: ActiveValue::Set("libra@example.com".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let tmpl = template::ActiveModel {
            code: ActiveValue::Set("trusts__nevada".into()),
            title: ActiveValue::Set("Nevada Trust".into()),
            respondent_type: ActiveValue::Set("entity".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let q = question::ActiveModel {
            code: ActiveValue::Set("trustee_name".into()),
            prompt: ActiveValue::Set("Who is the trustee?".into()),
            answer_type: ActiveValue::Set("string".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        answer::ActiveModel {
            question_id: ActiveValue::Set(q.id),
            person_id: ActiveValue::Set(libra.id),
            value: ActiveValue::Set(answer::primitive("Nick Shook")),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let __dri = crate::test_support::dri_person(&db).await;
        let proj = project::ActiveModel {
            name: ActiveValue::Set("Libra trust".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(crate::test_support::seed_entity(&db).await),
            staff_dri_person_id: ActiveValue::Set(Some(__dri)),
            client_dri_person_id: ActiveValue::Set(Some(__dri)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        notation::ActiveModel {
            template_id: ActiveValue::Set(tmpl.id),
            person_id: ActiveValue::Set(libra.id),
            entity_id: ActiveValue::Set(None),
            project_id: ActiveValue::Set(proj.id),
            state: ActiveValue::Set("staff_review".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        assert_eq!(template::Entity::find().all(&db).await.unwrap().len(), 1);
        assert_eq!(question::Entity::find().all(&db).await.unwrap().len(), 1);
        assert_eq!(answer::Entity::find().all(&db).await.unwrap().len(), 1);
        assert_eq!(notation::Entity::find().all(&db).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn mail_migration_creates_address_mailroom_letter() {
        use crate::entity::{address, letter, mailroom};
        let db = pg().await;

        let addr = address::ActiveModel {
            person_id: ActiveValue::Set(None),
            entity_id: ActiveValue::Set(None),
            line1: ActiveValue::Set("123 Main St".into()),
            line2: ActiveValue::Set(None),
            city: ActiveValue::Set("Reno".into()),
            region: ActiveValue::Set("NV".into()),
            postal_code: ActiveValue::Set("89501".into()),
            country: ActiveValue::Set("US".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let mr = mailroom::ActiveModel {
            name: ActiveValue::Set("HQ".into()),
            address_id: ActiveValue::Set(addr.id),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        letter::ActiveModel {
            mailroom_id: ActiveValue::Set(mr.id),
            direction: ActiveValue::Set("incoming".into()),
            sender: ActiveValue::Set("IRS".into()),
            recipient: ActiveValue::Set("Acme".into()),
            summary: ActiveValue::Set("Form 990 reminder".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        assert_eq!(address::Entity::find().all(&db).await.unwrap().len(), 1);
        assert_eq!(mailroom::Entity::find().all(&db).await.unwrap().len(), 1);
        assert_eq!(letter::Entity::find().all(&db).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn project_migration_creates_project_and_role_tables() {
        use crate::entity::{
            entity, entity_type, jurisdiction, person, person_entity_role, person_project_role,
            project,
        };
        let db = pg().await;

        let libra = person::ActiveModel {
            name: ActiveValue::Set("Libra".into()),
            email: ActiveValue::Set("libra@example.com".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let nv = jurisdiction::ActiveModel {
            name: ActiveValue::Set("Nevada".into()),
            code: ActiveValue::Set("NV".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let llc = entity_type::ActiveModel {
            name: ActiveValue::Set("LLC".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let acme = entity::ActiveModel {
            name: ActiveValue::Set("Acme".into()),
            entity_type_id: ActiveValue::Set(llc.id),
            jurisdiction_id: ActiveValue::Set(nv.id),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let __dri = crate::test_support::dri_person(&db).await;
        let proj = project::ActiveModel {
            name: ActiveValue::Set("2026 audit".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(acme.id),
            staff_dri_person_id: ActiveValue::Set(Some(__dri)),
            client_dri_person_id: ActiveValue::Set(Some(__dri)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        person_entity_role::ActiveModel {
            person_id: ActiveValue::Set(libra.id),
            entity_id: ActiveValue::Set(acme.id),
            role: ActiveValue::Set("manager".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        person_project_role::ActiveModel {
            person_id: ActiveValue::Set(libra.id),
            project_id: ActiveValue::Set(proj.id),
            participation: ActiveValue::Set("attorney".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        assert_eq!(project::Entity::find().all(&db).await.unwrap().len(), 1);
        assert_eq!(
            person_entity_role::Entity::find()
                .all(&db)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            person_project_role::Entity::find()
                .all(&db)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn template_code_is_unique() {
        use crate::entity::template;
        let db = pg().await;
        let make = |code: &str| template::ActiveModel {
            code: ActiveValue::Set(code.into()),
            title: ActiveValue::Set("t".into()),
            respondent_type: ActiveValue::Set("entity".into()),
            ..Default::default()
        };
        make("dup").insert(&db).await.unwrap();
        assert!(make("dup").insert(&db).await.is_err());
    }

    /// Helper for the journal tests: seed a Libra and a fresh
    /// retainer Notation so the events can FK into a real row.
    async fn seed_notation_for_event_tests(db: &super::Db) -> uuid::Uuid {
        use crate::entity::{notation, person, project, template};
        let libra = person::ActiveModel {
            name: ActiveValue::Set("Libra".into()),
            email: ActiveValue::Set("libra@example.com".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let tmpl = template::ActiveModel {
            code: ActiveValue::Set("onboarding__retainer".into()),
            title: ActiveValue::Set("Retainer".into()),
            respondent_type: ActiveValue::Set("person_and_entity".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let __dri = crate::test_support::dri_person(db).await;
        let proj = project::ActiveModel {
            name: ActiveValue::Set("Libra retainer".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(crate::test_support::seed_entity(db).await),
            staff_dri_person_id: ActiveValue::Set(Some(__dri)),
            client_dri_person_id: ActiveValue::Set(Some(__dri)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        notation::ActiveModel {
            template_id: ActiveValue::Set(tmpl.id),
            person_id: ActiveValue::Set(libra.id),
            entity_id: ActiveValue::Set(None),
            project_id: ActiveValue::Set(proj.id),
            state: ActiveValue::Set("BEGIN".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn notation_events_journal_appends_in_id_order() {
        use crate::entity::notation_event::{self, MACHINE_QUESTIONNAIRE};
        let db = pg().await;
        let nid = seed_notation_for_event_tests(&db).await;

        let make = |from: &str, to: &str, recorded: &str| notation_event::ActiveModel {
            notation_id: ActiveValue::Set(nid),
            machine_kind: ActiveValue::Set(MACHINE_QUESTIONNAIRE.into()),
            from_state: ActiveValue::Set(from.into()),
            to_state: ActiveValue::Set(to.into()),
            condition: ActiveValue::Set("_".into()),
            payload: ActiveValue::Set(Some(format!(r#"{{"answer_value":"{to}"}}"#))),
            recorded_at: ActiveValue::Set(recorded.into()),
            ..Default::default()
        };

        make("BEGIN", "client_name", "2026-05-21T10:00:00Z")
            .insert(&db)
            .await
            .unwrap();
        make("client_name", "client_email", "2026-05-21T10:01:00Z")
            .insert(&db)
            .await
            .unwrap();

        let all = notation_event::Entity::find()
            .filter(notation_event::Column::NotationId.eq(nid))
            .order_by_asc(notation_event::Column::Id)
            .all(&db)
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].to_state, "client_name");
        assert_eq!(all[1].to_state, "client_email");
        // Payload survives round-trip as opaque JSON text.
        assert_eq!(
            all[0].payload.as_deref(),
            Some(r#"{"answer_value":"client_name"}"#)
        );
    }

    #[tokio::test]
    async fn latest_for_kind_returns_the_most_recent_event_for_that_machine() {
        use crate::entity::notation_event::{
            self, latest_for_kind, MACHINE_QUESTIONNAIRE, MACHINE_WORKFLOW,
        };
        let db = pg().await;
        let nid = seed_notation_for_event_tests(&db).await;

        // Three questionnaire events; one workflow event interleaved.
        for (kind, from, to, t) in [
            (MACHINE_QUESTIONNAIRE, "BEGIN", "client_name", "10:00"),
            (
                MACHINE_QUESTIONNAIRE,
                "client_name",
                "client_email",
                "10:01",
            ),
            (MACHINE_WORKFLOW, "BEGIN", "intake_persisted", "10:02"),
            (
                MACHINE_QUESTIONNAIRE,
                "client_email",
                "project_name",
                "10:03",
            ),
        ] {
            notation_event::ActiveModel {
                notation_id: ActiveValue::Set(nid),
                machine_kind: ActiveValue::Set((*kind).into()),
                from_state: ActiveValue::Set(from.to_string()),
                to_state: ActiveValue::Set(to.to_string()),
                condition: ActiveValue::Set("_".into()),
                payload: ActiveValue::Set(None),
                recorded_at: ActiveValue::Set(format!("2026-05-21T{t}:00Z")),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }

        let q = latest_for_kind(&db, nid, MACHINE_QUESTIONNAIRE)
            .await
            .unwrap()
            .expect("questionnaire should have events");
        assert_eq!(q.to_state, "project_name");
        assert_eq!(q.recorded_at, "2026-05-21T10:03:00Z");

        let w = latest_for_kind(&db, nid, MACHINE_WORKFLOW)
            .await
            .unwrap()
            .expect("workflow should have events");
        assert_eq!(w.to_state, "intake_persisted");
    }

    #[tokio::test]
    async fn latest_for_kind_returns_none_when_machine_has_not_started() {
        use crate::entity::notation_event::{latest_for_kind, MACHINE_QUESTIONNAIRE};
        let db = pg().await;
        let nid = seed_notation_for_event_tests(&db).await;
        let none = latest_for_kind(&db, nid, MACHINE_QUESTIONNAIRE)
            .await
            .unwrap();
        assert!(none.is_none());
    }

    #[tokio::test]
    async fn is_complete_flips_to_true_when_latest_event_lands_at_end() {
        use crate::entity::notation_event::{self, is_complete, MACHINE_QUESTIONNAIRE};
        let db = pg().await;
        let nid = seed_notation_for_event_tests(&db).await;

        notation_event::ActiveModel {
            notation_id: ActiveValue::Set(nid),
            machine_kind: ActiveValue::Set(MACHINE_QUESTIONNAIRE.into()),
            from_state: ActiveValue::Set("BEGIN".into()),
            to_state: ActiveValue::Set("client_name".into()),
            condition: ActiveValue::Set("_".into()),
            payload: ActiveValue::Set(None),
            recorded_at: ActiveValue::Set("2026-05-21T10:00:00Z".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        assert!(!is_complete(&db, nid, MACHINE_QUESTIONNAIRE).await.unwrap());

        notation_event::ActiveModel {
            notation_id: ActiveValue::Set(nid),
            machine_kind: ActiveValue::Set(MACHINE_QUESTIONNAIRE.into()),
            from_state: ActiveValue::Set("client_name".into()),
            to_state: ActiveValue::Set("END".into()),
            condition: ActiveValue::Set("_".into()),
            payload: ActiveValue::Set(None),
            recorded_at: ActiveValue::Set("2026-05-21T10:01:00Z".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        assert!(is_complete(&db, nid, MACHINE_QUESTIONNAIRE).await.unwrap());
    }

    #[tokio::test]
    async fn notation_events_are_isolated_across_notations() {
        // Two Notations both walk a questionnaire; queries by
        // notation_id must not bleed into each other.
        use crate::entity::notation_event::{self, latest_for_kind, MACHINE_QUESTIONNAIRE};
        let db = pg().await;
        let a = seed_notation_for_event_tests(&db).await;

        // Second notation reusing the first person/template.
        let b = crate::entity::notation::ActiveModel {
            template_id: ActiveValue::Set(
                crate::entity::template::Entity::find()
                    .one(&db)
                    .await
                    .unwrap()
                    .unwrap()
                    .id,
            ),
            person_id: ActiveValue::Set(
                crate::entity::person::Entity::find()
                    .one(&db)
                    .await
                    .unwrap()
                    .unwrap()
                    .id,
            ),
            entity_id: ActiveValue::Set(None),
            project_id: ActiveValue::Set(
                crate::entity::project::Entity::find()
                    .one(&db)
                    .await
                    .unwrap()
                    .unwrap()
                    .id,
            ),
            state: ActiveValue::Set("BEGIN".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;

        for (nid, to) in [(a, "client_name"), (b, "client_email")] {
            notation_event::ActiveModel {
                notation_id: ActiveValue::Set(nid),
                machine_kind: ActiveValue::Set(MACHINE_QUESTIONNAIRE.into()),
                from_state: ActiveValue::Set("BEGIN".into()),
                to_state: ActiveValue::Set(to.into()),
                condition: ActiveValue::Set("_".into()),
                payload: ActiveValue::Set(None),
                recorded_at: ActiveValue::Set("2026-05-21T10:00:00Z".into()),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }

        assert_eq!(
            latest_for_kind(&db, a, MACHINE_QUESTIONNAIRE)
                .await
                .unwrap()
                .unwrap()
                .to_state,
            "client_name"
        );
        assert_eq!(
            latest_for_kind(&db, b, MACHINE_QUESTIONNAIRE)
                .await
                .unwrap()
                .unwrap()
                .to_state,
            "client_email"
        );
    }

    #[tokio::test]
    async fn person_email_is_unique() {
        let db = pg().await;

        let make = |email: &str| person::ActiveModel {
            name: ActiveValue::Set("X".into()),
            email: ActiveValue::Set(email.into()),
            ..Default::default()
        };
        make("dup@example.com").insert(&db).await.unwrap();
        let err = make("dup@example.com").insert(&db).await;
        assert!(err.is_err(), "duplicate email should be rejected");
    }
}
