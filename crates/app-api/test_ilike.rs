use sea_orm::{entity::prelude::*, sea_query::{Expr, extension::postgres::PgExpr}};

pub fn test_ilike(pattern: &str) -> SimpleExpr {
    Expr::col("name").ilike(pattern)
}
