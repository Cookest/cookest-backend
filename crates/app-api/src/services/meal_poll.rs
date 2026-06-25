//! Meal poll service — vote on what to cook, shareable by token so even people
//! without the app can help choose.
//!
//! Flow: an owner creates a poll with 2–4 candidate dishes and gets a token. The
//! token resolves to a public page (no auth) where anyone can vote once
//! (deduplicated by a client-supplied `voter_key`). The owner sees live results.

use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::{meal_poll, meal_poll_option, meal_poll_vote};
use cookest_shared::errors::AppError;

#[derive(Debug, Deserialize)]
pub struct PollOptionInput {
    pub recipe_id: Option<i64>,
    pub label: String,
    pub image_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePollRequest {
    pub title: String,
    pub slot_id: Option<i64>,
    pub options: Vec<PollOptionInput>,
}

#[derive(Debug, Deserialize)]
pub struct VoteRequest {
    pub option_id: i64,
    pub voter_key: String,
    pub voter_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PollOptionView {
    pub id: i64,
    pub label: String,
    pub image_url: Option<String>,
    pub recipe_id: Option<i64>,
    pub votes: i64,
}

#[derive(Debug, Serialize)]
pub struct PollView {
    pub id: Uuid,
    pub token: String,
    pub title: String,
    pub status: String,
    pub options: Vec<PollOptionView>,
    pub total_votes: i64,
}

pub struct MealPollService {
    db: DatabaseConnection,
}

impl MealPollService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn create(
        &self,
        owner_id: Uuid,
        req: CreatePollRequest,
    ) -> Result<PollView, AppError> {
        if req.options.len() < 2 {
            return Err(AppError::Internal("A poll needs at least 2 options".into()));
        }

        let id = Uuid::new_v4();
        let token = Uuid::new_v4().simple().to_string();

        meal_poll::ActiveModel {
            id: Set(id),
            owner_id: Set(owner_id),
            slot_id: Set(req.slot_id),
            token: Set(token.clone()),
            title: Set(req.title),
            status: Set("open".to_string()),
            closes_at: Set(None),
            created_at: Set(Utc::now().fixed_offset()),
        }
        .insert(&self.db)
        .await?;

        for opt in req.options {
            meal_poll_option::ActiveModel {
                poll_id: Set(id),
                recipe_id: Set(opt.recipe_id),
                label: Set(opt.label),
                image_url: Set(opt.image_url),
                ..Default::default()
            }
            .insert(&self.db)
            .await?;
        }

        self.view_by_token(&token).await
    }

    /// Public poll view by token — used by the app and the no-auth web page.
    pub async fn view_by_token(&self, token: &str) -> Result<PollView, AppError> {
        let poll = meal_poll::Entity::find()
            .filter(meal_poll::Column::Token.eq(token))
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Poll".to_string()))?;

        let options = meal_poll_option::Entity::find()
            .filter(meal_poll_option::Column::PollId.eq(poll.id))
            .all(&self.db)
            .await?;

        let votes = meal_poll_vote::Entity::find()
            .filter(meal_poll_vote::Column::PollId.eq(poll.id))
            .all(&self.db)
            .await?;

        let mut counts: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
        for v in &votes {
            *counts.entry(v.option_id).or_insert(0) += 1;
        }

        Ok(PollView {
            id: poll.id,
            token: poll.token,
            title: poll.title,
            status: poll.status,
            total_votes: votes.len() as i64,
            options: options
                .into_iter()
                .map(|o| PollOptionView {
                    votes: counts.get(&o.id).copied().unwrap_or(0),
                    id: o.id,
                    label: o.label,
                    image_url: o.image_url,
                    recipe_id: o.recipe_id,
                })
                .collect(),
        })
    }

    /// Record a vote. One vote per `voter_key`; voting again changes the choice.
    pub async fn vote(&self, token: &str, req: VoteRequest) -> Result<PollView, AppError> {
        let poll = meal_poll::Entity::find()
            .filter(meal_poll::Column::Token.eq(token))
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Poll".to_string()))?;

        if poll.status != "open" {
            return Err(AppError::Forbidden);
        }

        // Validate the option belongs to this poll.
        let option = meal_poll_option::Entity::find_by_id(req.option_id)
            .one(&self.db)
            .await?
            .filter(|o| o.poll_id == poll.id)
            .ok_or_else(|| AppError::NotFound("Option".to_string()))?;

        let existing = meal_poll_vote::Entity::find()
            .filter(meal_poll_vote::Column::PollId.eq(poll.id))
            .filter(meal_poll_vote::Column::VoterKey.eq(&req.voter_key))
            .one(&self.db)
            .await?;

        match existing {
            Some(v) => {
                let mut active: meal_poll_vote::ActiveModel = v.into();
                active.option_id = Set(option.id);
                active.voter_name = Set(req.voter_name);
                active.update(&self.db).await?;
            }
            None => {
                meal_poll_vote::ActiveModel {
                    poll_id: Set(poll.id),
                    option_id: Set(option.id),
                    voter_key: Set(req.voter_key),
                    voter_name: Set(req.voter_name),
                    created_at: Set(Utc::now().fixed_offset()),
                    ..Default::default()
                }
                .insert(&self.db)
                .await?;
            }
        }

        self.view_by_token(token).await
    }
}
