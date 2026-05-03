//! Authentication service with secure password handling
//! 
//! Security features:
//! - Argon2id with OWASP-recommended parameters
//! - Timing-safe password verification
//! - Account lockout after failed attempts
//! - Refresh token rotation with SHA-256 hashing (not DefaultHasher)
//! - Subscription tier always read from DB when issuing access tokens

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2, Algorithm, Params, Version,
};
use chrono::{Duration, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::entity::user::{self, ActiveModel, Entity as User, Model as UserModel, UserResponse};
use cookest_shared::errors::AppError;
use crate::services::token::{SubscriptionTier, TokenPair, TokenService};
use crate::validation::{normalize_email, LoginRequest, RegisterRequest};

/// Maximum failed login attempts before lockout
const MAX_FAILED_ATTEMPTS: i32 = 5;
/// Lockout duration in minutes
const LOCKOUT_DURATION_MINUTES: i64 = 15;

pub struct AuthService {
    db: DatabaseConnection,
    token_service: TokenService,
    argon2: Argon2<'static>,
}

impl AuthService {
    pub fn new(db: DatabaseConnection, token_service: TokenService) -> Self {
        // Configure Argon2id with OWASP-recommended parameters
        // Memory: 19 MiB, Iterations: 2, Parallelism: 1
        let params = Params::new(
            19 * 1024, // 19 MiB memory
            2,         // 2 iterations
            1,         // 1 degree of parallelism
            None,      // Default output length
        )
        .expect("Invalid Argon2 parameters");

        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        Self {
            db,
            token_service,
            argon2,
        }
    }

    /// Register a new user
    pub async fn register(&self, request: RegisterRequest) -> Result<UserResponse, AppError> {
        let email = normalize_email(&request.email);

        // Check if user already exists
        let existing = User::find()
            .filter(user::Column::Email.eq(&email))
            .one(&self.db)
            .await?;

        if existing.is_some() {
            // Return generic error to prevent email enumeration
            return Err(AppError::UserAlreadyExists);
        }

        // Hash password with Argon2id
        let password_hash = self.hash_password(&request.password)?;

        let now = Utc::now().fixed_offset();
        let user_id = Uuid::new_v4();

        let user = ActiveModel {
            id: Set(user_id),
            email: Set(email),
            password_hash: Set(password_hash),
            refresh_token_hash: Set(None),
            name: Set(None),
            household_size: Set(1),
            dietary_restrictions: Set(None),
            allergies: Set(None),
            avatar_url: Set(None),
            is_email_verified: Set(false),
            two_factor_enabled: Set(false),
            totp_secret: Set(None),
            failed_login_attempts: Set(0),
            locked_until: Set(None),
            subscription_tier: Set("free".to_string()),
            subscription_valid_until: Set(None),
            stripe_customer_id: Set(None),
            cooking_skill_level: Set(None),
            preferred_cuisines: Set(None),
            health_goals: Set(None),
            weekly_budget: Set(None),
            preferred_time_per_meal_min: Set(None),
            onboarding_completed: Set(false),
            is_admin: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
        };

        let user = user.insert(&self.db).await?;

        tracing::info!("User registered: {}", user.id);

        Ok(UserResponse::from(user))
    }

    /// Authenticate user and return tokens
    pub async fn login(&self, request: LoginRequest) -> Result<(TokenPair, String, UserModel), AppError> {
        let email = normalize_email(&request.email);

        // Find user
        let user = User::find()
            .filter(user::Column::Email.eq(&email))
            .one(&self.db)
            .await?
            .ok_or(AppError::AuthenticationFailed)?;

        // Check if account is locked
        if let Some(locked_until) = user.locked_until {
            if Utc::now() < locked_until {
                tracing::warn!("Login attempt on locked account: {}", user.id);
                return Err(AppError::AuthenticationFailed);
            }
        }

        // Verify password (timing-safe comparison via Argon2)
        if !self.verify_password(&request.password, &user.password_hash)? {
            // Increment failed attempts
            self.increment_failed_attempts(&user).await?;
            return Err(AppError::AuthenticationFailed);
        }

        // Read tier and admin status from DB (authoritative source)
        let tier = SubscriptionTier::from_str(&user.subscription_tier);
        let is_admin = user.is_admin;

        // Generate tokens — tier embedded in access token only
        let access_token = self.token_service.generate_access_token(user.id, &user.email, tier, is_admin)?;
        let refresh_token = self.token_service.generate_refresh_token(user.id, &user.email)?;

        // Hash and store refresh token for rotation tracking (SHA-256)
        let refresh_token_hash = hash_token_sha256(&refresh_token);

        // Reset failed attempts and store refresh token hash
        let mut active_user: ActiveModel = user.clone().into();
        active_user.failed_login_attempts = Set(0);
        active_user.locked_until = Set(None);
        active_user.refresh_token_hash = Set(Some(refresh_token_hash));
        active_user.updated_at = Set(Utc::now().fixed_offset());
        active_user.update(&self.db).await?;

        tracing::info!("User logged in: {}", user.id);

        let token_pair = TokenPair {
            access_token,
            token_type: "Bearer".to_string(),
            expires_in: self.token_service.access_expiry_seconds(),
        };

        Ok((token_pair, refresh_token, user))
    }

    /// Refresh access token using refresh token — always reads tier from DB
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<(TokenPair, String, UserModel), AppError> {
        // Validate refresh token structure
        let claims = self.token_service.validate_refresh_token(refresh_token)?;
        
        let user_id = Uuid::parse_str(&claims.sub)
            .map_err(|_| AppError::InvalidToken)?;

        // Find user — tier is always read fresh from DB here
        let user = User::find_by_id(user_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::InvalidToken)?;

        // Verify refresh token hash matches (token rotation via SHA-256)
        let current_hash = hash_token_sha256(refresh_token);
        match &user.refresh_token_hash {
            Some(stored_hash) if stored_hash == &current_hash => {}
            _ => {
                // Token reuse detected or invalid — invalidate all sessions
                tracing::warn!("Refresh token reuse or invalid token for user: {}", user.id);
                let mut active_user: ActiveModel = user.clone().into();
                active_user.refresh_token_hash = Set(None);
                active_user.update(&self.db).await?;
                return Err(AppError::InvalidToken);
            }
        }

        // Read current tier from DB (subscription may have changed since last login)
        let tier = SubscriptionTier::from_str(&user.subscription_tier);
        let is_admin = user.is_admin;

        // Generate new token pair (token rotation)
        let new_access_token = self.token_service.generate_access_token(user.id, &user.email, tier, is_admin)?;
        let new_refresh_token = self.token_service.generate_refresh_token(user.id, &user.email)?;

        // Store new refresh token hash
        let new_refresh_hash = hash_token_sha256(&new_refresh_token);
        let mut active_user: ActiveModel = user.clone().into();
        active_user.refresh_token_hash = Set(Some(new_refresh_hash));
        active_user.updated_at = Set(Utc::now().fixed_offset());
        active_user.update(&self.db).await?;

        let token_pair = TokenPair {
            access_token: new_access_token,
            token_type: "Bearer".to_string(),
            expires_in: self.token_service.access_expiry_seconds(),
        };

        Ok((token_pair, new_refresh_token, user))
    }

    /// Logout user by invalidating refresh token
    pub async fn logout(&self, user_id: Uuid) -> Result<(), AppError> {
        let user = User::find_by_id(user_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::InvalidToken)?;

        let mut active_user: ActiveModel = user.into();
        active_user.refresh_token_hash = Set(None);
        active_user.updated_at = Set(Utc::now().fixed_offset());
        active_user.update(&self.db).await?;

        tracing::info!("User logged out: {}", user_id);

        Ok(())
    }

    /// Change user password — invalidates all sessions
    pub async fn change_password(
        &self,
        user_id: Uuid,
        current_password: &str,
        new_password: &str,
    ) -> Result<(), AppError> {
        let user = User::find_by_id(user_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::AuthenticationFailed)?;

        if !self.verify_password(current_password, &user.password_hash)? {
            return Err(AppError::AuthenticationFailed);
        }

        // Validate new password strength (same rules as registration)
        crate::validation::validate_password_strength(new_password).map_err(|e| {
            let mut errors = validator::ValidationErrors::new();
            errors.add("new_password", e);
            AppError::Validation(errors)
        })?;

        let new_hash = self.hash_password(new_password)?;

        let mut active_user: ActiveModel = user.into();
        active_user.password_hash = Set(new_hash);
        active_user.refresh_token_hash = Set(None); // invalidate all sessions
        active_user.updated_at = Set(Utc::now().fixed_offset());
        active_user.update(&self.db).await?;

        tracing::info!("Password changed for user: {}", user_id);
        Ok(())
    }

    /// Delete account — verifies password before deletion
    pub async fn delete_account(&self, user_id: Uuid, password: &str) -> Result<(), AppError> {
        let user = User::find_by_id(user_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::AuthenticationFailed)?;

        if !self.verify_password(password, &user.password_hash)? {
            return Err(AppError::AuthenticationFailed);
        }

        User::delete_by_id(user_id).exec(&self.db).await?;
        tracing::info!("Account deleted for user: {}", user_id);
        Ok(())
    }

    /// Hash password using Argon2id
    fn hash_password(&self, password: &str) -> Result<String, AppError> {
        let salt = SaltString::generate(&mut OsRng);
        
        self.argon2
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|e| AppError::Internal(format!("Password hashing failed: {}", e)))
    }

    /// Verify password against hash (timing-safe)
    fn verify_password(&self, password: &str, hash: &str) -> Result<bool, AppError> {
        let parsed_hash = PasswordHash::new(hash)
            .map_err(|e| AppError::Internal(format!("Invalid password hash: {}", e)))?;

        Ok(self.argon2.verify_password(password.as_bytes(), &parsed_hash).is_ok())
    }

    /// Increment failed login attempts and lock if necessary
    async fn increment_failed_attempts(&self, user: &UserModel) -> Result<(), AppError> {
        let new_attempts = user.failed_login_attempts + 1;
        
        let mut active_user: ActiveModel = user.clone().into();
        active_user.failed_login_attempts = Set(new_attempts);
        
        if new_attempts >= MAX_FAILED_ATTEMPTS {
            let lockout_until = Utc::now() + Duration::minutes(LOCKOUT_DURATION_MINUTES);
            active_user.locked_until = Set(Some(lockout_until.fixed_offset()));
            tracing::warn!("Account locked due to failed attempts: {}", user.id);
        }
        
        active_user.updated_at = Set(Utc::now().fixed_offset());
        active_user.update(&self.db).await?;
        
        Ok(())
    }
}

/// Hash a token using SHA-256 (cryptographically secure, replaces DefaultHasher)
pub fn hash_token_sha256(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}

