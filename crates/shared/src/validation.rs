use validator::Validate;

use crate::errors::AppError;

/// Validate a request body and return AppError on failure
pub fn validate_request<T: Validate>(body: &T) -> Result<(), AppError> {
    body.validate().map_err(AppError::from)
}
