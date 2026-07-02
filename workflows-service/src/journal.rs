//! Re-exports of the Postgres journal helpers from
//! [`store::entity::notation_event`]. Kept as a thin module so
//! existing call sites inside this crate (and the relocated unit
//! tests below) read naturally.

pub use store::entity::notation_event::{
    answer_payload, append_event, workflow_payload, TransitionRecord,
};

#[cfg(test)]
mod tests {
    use super::{answer_payload, append_event, TransitionRecord};
    use sea_orm::{
        ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
        QueryOrder,
    };
    use store::entity::{notation, notation_event, person, template};

    async fn fresh_db() -> DatabaseConnection {
        store::test_support::pg().await
    }

    async fn seed_notation(db: &DatabaseConnection) -> (uuid::Uuid, uuid::Uuid, uuid::Uuid) {
        use store::entity::project;
        let alice = person::ActiveModel {
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
        let __dri = store::test_support::dri_person(db).await;
        let proj = project::ActiveModel {
            name: ActiveValue::Set("Libra retainer".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(store::test_support::seed_entity(db).await),
            staff_dri_person_id: ActiveValue::Set(Some(__dri)),
            client_dri_person_id: ActiveValue::Set(Some(__dri)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let notation_id = notation::ActiveModel {
            template_id: ActiveValue::Set(tmpl.id),
            person_id: ActiveValue::Set(alice.id),
            entity_id: ActiveValue::Set(None),
            project_id: ActiveValue::Set(proj.id),
            state: ActiveValue::Set("BEGIN".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id;
        (notation_id, alice.id, tmpl.id)
    }

    #[tokio::test]
    async fn append_event_inserts_one_row_with_the_expected_columns() {
        let db = fresh_db().await;
        let (nid, person_id, template_id) = seed_notation(&db).await;
        let row = append_event(
            &db,
            TransitionRecord {
                notation_id: nid,
                acting_person_id: Some(person_id),
                machine_kind: "questionnaire",
                from_state: "BEGIN",
                to_state: "client_name",
                condition: "_",
                payload_json: Some(answer_payload("Libra")),
                recorded_at: "2026-05-21T10:00:00+00:00",
            },
        )
        .await
        .unwrap();
        assert_eq!(row.machine_kind, "questionnaire");
        assert_eq!(row.acting_person_id, person_id);
        assert_eq!(row.template_version_id, template_id);
        assert_eq!(row.payload.as_deref(), Some(r#"{"answer_value":"Libra"}"#));
    }

    #[tokio::test]
    async fn append_event_preserves_insert_order_across_repeated_calls() {
        let db = fresh_db().await;
        let (nid, person_id, _) = seed_notation(&db).await;
        for (from, to) in [
            ("BEGIN", "client_name"),
            ("client_name", "client_email"),
            ("client_email", "project_name"),
        ] {
            append_event(
                &db,
                TransitionRecord {
                    notation_id: nid,
                    acting_person_id: Some(person_id),
                    machine_kind: "questionnaire",
                    from_state: from,
                    to_state: to,
                    condition: "_",
                    payload_json: None,
                    recorded_at: "2026-05-21T10:00:00+00:00",
                },
            )
            .await
            .unwrap();
        }
        let rows = notation_event::Entity::find()
            .filter(notation_event::Column::NotationId.eq(nid))
            .order_by_asc(notation_event::Column::Id)
            .all(&db)
            .await
            .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].to_state, "project_name");
    }

    #[tokio::test]
    async fn workflow_event_payload_records_actor_and_pinned_template_version() {
        let db = fresh_db().await;
        let (nid, person_id, template_id) = seed_notation(&db).await;
        let row = append_event(
            &db,
            TransitionRecord {
                notation_id: nid,
                acting_person_id: Some(person_id),
                machine_kind: notation_event::MACHINE_WORKFLOW,
                from_state: "BEGIN",
                to_state: "staff_review",
                condition: "intake_submitted",
                payload_json: None,
                recorded_at: "2026-05-21T10:00:00+00:00",
            },
        )
        .await
        .unwrap();
        let payload: serde_json::Value =
            serde_json::from_str(row.payload.as_deref().unwrap()).unwrap();
        assert_eq!(
            payload["acting_person_id"].as_str().unwrap(),
            person_id.to_string()
        );
        assert_eq!(
            payload["template_version_id"].as_str().unwrap(),
            template_id.to_string()
        );
    }

    #[tokio::test]
    async fn notation_events_cannot_be_updated_or_deleted() {
        let db = fresh_db().await;
        let (nid, person_id, _) = seed_notation(&db).await;
        let row = append_event(
            &db,
            TransitionRecord {
                notation_id: nid,
                acting_person_id: Some(person_id),
                machine_kind: "questionnaire",
                from_state: "BEGIN",
                to_state: "client_name",
                condition: "_",
                payload_json: Some(answer_payload("Libra")),
                recorded_at: "2026-05-21T10:00:00+00:00",
            },
        )
        .await
        .unwrap();

        let mut active: notation_event::ActiveModel = row.clone().into();
        active.to_state = ActiveValue::Set("changed".to_string());
        let update_err = active.update(&db).await.unwrap_err();
        assert!(update_err.to_string().contains("append-only"));

        let delete_err = notation_event::Entity::delete_by_id(row.id)
            .exec(&db)
            .await
            .unwrap_err();
        assert!(delete_err.to_string().contains("append-only"));
    }
}
