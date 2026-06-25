//! Household service — family groups that share meal planning.
//!
//! An owner creates a household, generates a shareable invite token, and other
//! Cookest users join with it. Members can then be polled on what to cook
//! (see [`crate::services::meal_poll`]).

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Set,
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

    /// Remove a member from a household (either leave or kick).
    /// If the owner leaves:
    /// - If they are the last member, the household is deleted (disbanded).
    /// - Otherwise, ownership is transferred to the next joined member.
    pub async fn remove_member(
        &self,
        user_id: Uuid,
        member_id: Uuid,
    ) -> Result<Option<HouseholdView>, AppError> {
        let membership = household_member::Entity::find()
            .filter(household_member::Column::UserId.eq(user_id))
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Household membership".to_string()))?;

        let household_id = membership.household_id;
        let hh = household::Entity::find_by_id(household_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Household".to_string()))?;

        if user_id == member_id {
            // Caller is leaving
            if hh.owner_id == user_id {
                // Owner is leaving
                // Check if there are other members
                let other_members = household_member::Entity::find()
                    .filter(household_member::Column::HouseholdId.eq(household_id))
                    .filter(household_member::Column::UserId.ne(user_id))
                    .order_by_asc(household_member::Column::JoinedAt)
                    .all(&self.db)
                    .await?;

                if other_members.is_empty() {
                    // Disband household
                    household::Entity::delete_by_id(household_id)
                        .exec(&self.db)
                        .await?;
                    return Ok(None);
                } else {
                    // Promote the next member who joined
                    let next_owner = &other_members[0];

                    // Update household owner_id
                    let mut hh_active: household::ActiveModel = hh.into();
                    hh_active.owner_id = Set(next_owner.user_id);
                    hh_active.update(&self.db).await?;

                    // Update next owner's role to "owner"
                    let mut member_active: household_member::ActiveModel =
                        next_owner.clone().into();
                    member_active.role = Set("owner".to_string());
                    member_active.update(&self.db).await?;

                    // Delete old owner's membership
                    household_member::Entity::delete_many()
                        .filter(household_member::Column::HouseholdId.eq(household_id))
                        .filter(household_member::Column::UserId.eq(user_id))
                        .exec(&self.db)
                        .await?;
                }
            } else {
                // Non-owner is leaving
                household_member::Entity::delete_many()
                    .filter(household_member::Column::HouseholdId.eq(household_id))
                    .filter(household_member::Column::UserId.eq(user_id))
                    .exec(&self.db)
                    .await?;
            }
        } else {
            // Kicking another member
            // Verify caller is owner
            if hh.owner_id != user_id {
                return Err(AppError::Forbidden);
            }

            // Verify member to be removed is in the same household
            let member_to_kick = household_member::Entity::find()
                .filter(household_member::Column::HouseholdId.eq(household_id))
                .filter(household_member::Column::UserId.eq(member_id))
                .one(&self.db)
                .await?;

            if member_to_kick.is_none() {
                return Err(AppError::NotFound("Member".to_string()));
            }

            household_member::Entity::delete_many()
                .filter(household_member::Column::HouseholdId.eq(household_id))
                .filter(household_member::Column::UserId.eq(member_id))
                .exec(&self.db)
                .await?;
        }

        // Return updated household view if still exists
        let hh_exists = household::Entity::find_by_id(household_id)
            .one(&self.db)
            .await?;

        match hh_exists {
            Some(_) => Ok(Some(self.view(household_id).await?)),
            None => Ok(None),
        }
    }

    /// Transfer ownership of the household to another member.
    pub async fn transfer_ownership(
        &self,
        user_id: Uuid,
        new_owner_id: Uuid,
    ) -> Result<HouseholdView, AppError> {
        let membership = household_member::Entity::find()
            .filter(household_member::Column::UserId.eq(user_id))
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Household membership".to_string()))?;

        let household_id = membership.household_id;
        let hh = household::Entity::find_by_id(household_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Household".to_string()))?;

        // Verify caller is owner
        if hh.owner_id != user_id {
            return Err(AppError::Forbidden);
        }

        // Verify new owner is a member of the same household
        let new_owner_membership = household_member::Entity::find()
            .filter(household_member::Column::HouseholdId.eq(household_id))
            .filter(household_member::Column::UserId.eq(new_owner_id))
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Recipient member".to_string()))?;

        // 1. Update household owner_id
        let mut hh_active: household::ActiveModel = hh.into();
        hh_active.owner_id = Set(new_owner_id);
        hh_active.update(&self.db).await?;

        // 2. Demote old owner's role to "member"
        let mut old_owner_active: household_member::ActiveModel = membership.into();
        old_owner_active.role = Set("member".to_string());
        old_owner_active.update(&self.db).await?;

        // 3. Promote new owner's role to "owner"
        let mut new_owner_active: household_member::ActiveModel = new_owner_membership.into();
        new_owner_active.role = Set("owner".to_string());
        new_owner_active.update(&self.db).await?;

        self.view(household_id).await
    }
}
