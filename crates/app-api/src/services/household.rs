//! Household service — family groups that share meal planning.
//!
//! An owner creates a household, generates a shareable invite token, and other
//! Cookest users join with it. Members can then be polled on what to cook
//! (see [`crate::services::meal_poll`]).

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::{household, household_invite, household_member, user};
use cookest_shared::errors::AppError;

#[derive(Debug, Deserialize)]
pub struct CreateHouseholdRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct JoinRequest {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct MemberView {
    pub user_id: Uuid,
    pub name: Option<String>,
    pub role: String,
}

#[derive(Debug, Serialize)]
pub struct HouseholdView {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub members: Vec<MemberView>,
}

pub struct HouseholdService {
    db: DatabaseConnection,
}

impl HouseholdService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Create a household and add the creator as its owner member.
    pub async fn create(&self, owner_id: Uuid, name: String) -> Result<HouseholdView, AppError> {
        let now = Utc::now().fixed_offset();
        let id = Uuid::new_v4();

        household::ActiveModel {
            id: Set(id),
            owner_id: Set(owner_id),
            name: Set(name),
            created_at: Set(now),
        }
        .insert(&self.db)
        .await?;

        household_member::ActiveModel {
            household_id: Set(id),
            user_id: Set(owner_id),
            role: Set("owner".to_string()),
            joined_at: Set(now),
            ..Default::default()
        }
        .insert(&self.db)
        .await?;

        self.view(id).await
    }

    /// The household the user belongs to (owner or member), if any.
    pub async fn my_household(&self, user_id: Uuid) -> Result<Option<HouseholdView>, AppError> {
        let membership = household_member::Entity::find()
            .filter(household_member::Column::UserId.eq(user_id))
            .one(&self.db)
            .await?;
        match membership {
            Some(m) => Ok(Some(self.view(m.household_id).await?)),
            None => Ok(None),
        }
    }

    /// Create a shareable invite token. Only the household owner may invite.
    pub async fn create_invite(
        &self,
        user_id: Uuid,
        household_id: Uuid,
    ) -> Result<String, AppError> {
        let hh = household::Entity::find_by_id(household_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Household".to_string()))?;
        if hh.owner_id != user_id {
            return Err(AppError::Forbidden);
        }

        let token = Uuid::new_v4().simple().to_string();
        household_invite::ActiveModel {
            id: Set(Uuid::new_v4()),
            household_id: Set(household_id),
            token: Set(token.clone()),
            expires_at: Set(None),
            created_at: Set(Utc::now().fixed_offset()),
        }
        .insert(&self.db)
        .await?;

        Ok(token)
    }

    /// Join a household with an invite token.
    pub async fn join(&self, user_id: Uuid, token: &str) -> Result<HouseholdView, AppError> {
        let invite = household_invite::Entity::find()
            .filter(household_invite::Column::Token.eq(token))
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Invite".to_string()))?;

        if let Some(exp) = invite.expires_at {
            if exp < Utc::now().fixed_offset() {
                return Err(AppError::NotFound("Invite".to_string()));
            }
        }

        // Idempotent: only add membership if not already present.
        let existing = household_member::Entity::find()
            .filter(household_member::Column::HouseholdId.eq(invite.household_id))
            .filter(household_member::Column::UserId.eq(user_id))
            .one(&self.db)
            .await?;
        if existing.is_none() {
            household_member::ActiveModel {
                household_id: Set(invite.household_id),
                user_id: Set(user_id),
                role: Set("member".to_string()),
                joined_at: Set(Utc::now().fixed_offset()),
                ..Default::default()
            }
            .insert(&self.db)
            .await?;
        }

        self.view(invite.household_id).await
    }

    /// Build a household view with members and their display names.
    async fn view(&self, household_id: Uuid) -> Result<HouseholdView, AppError> {
        let hh = household::Entity::find_by_id(household_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Household".to_string()))?;

        let members = household_member::Entity::find()
            .filter(household_member::Column::HouseholdId.eq(household_id))
            .all(&self.db)
            .await?;

        let user_ids: Vec<Uuid> = members.iter().map(|m| m.user_id).collect();
        let names: std::collections::HashMap<Uuid, Option<String>> = user::Entity::find()
            .filter(user::Column::Id.is_in(user_ids))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|u| (u.id, u.name))
            .collect();

        Ok(HouseholdView {
            id: hh.id,
            name: hh.name,
            owner_id: hh.owner_id,
            members: members
                .into_iter()
                .map(|m| MemberView {
                    name: names.get(&m.user_id).cloned().flatten(),
                    user_id: m.user_id,
                    role: m.role,
                })
                .collect(),
        })
    }
}
