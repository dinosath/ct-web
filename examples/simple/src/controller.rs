use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use sea_orm::{ActiveValue::Set, DbErr};

use crate::{entity, service::NewUser, user_service::UserService};

pub fn router(users: UserService) -> Router {
    Router::new()
        .route("/users", get(list_users).post(create_user))
        .route(
            "/users/{id}",
            get(get_user).put(update_user).delete(delete_user),
        )
        .with_state(users)
}

#[app::route(method = "GET", path = "/users")]
async fn list_users(
    State(users): State<UserService>,
) -> Result<Json<Vec<entity::Model>>, ApiError> {
    users.list().await.map(Json).map_err(internal_error)
}

#[app::route(method = "POST", path = "/users")]
async fn create_user(
    State(users): State<UserService>,
    Json(input): Json<NewUser>,
) -> Result<(StatusCode, Json<entity::Model>), ApiError> {
    let user: entity::ActiveModel = input.into();
    users
        .create(user)
        .await
        .map(|user| (StatusCode::CREATED, Json(user)))
        .map_err(internal_error)
}

#[app::route(method = "GET", path = "/users/{id}")]
async fn get_user(
    State(users): State<UserService>,
    Path(id): Path<i32>,
) -> Result<Json<entity::Model>, ApiError> {
    users
        .get(id)
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or_else(not_found)
}

#[app::route(method = "PUT", path = "/users/{id}")]
async fn update_user(
    State(users): State<UserService>,
    Path(id): Path<i32>,
    Json(input): Json<NewUser>,
) -> Result<Json<entity::Model>, ApiError> {
    if users.get(id).await.map_err(internal_error)?.is_none() {
        return Err(not_found());
    }
    let mut user: entity::ActiveModel = input.into();
    user.id = Set(id);
    users.update(user).await.map(Json).map_err(internal_error)
}

#[app::route(method = "DELETE", path = "/users/{id}")]
async fn delete_user(
    State(users): State<UserService>,
    Path(id): Path<i32>,
) -> Result<StatusCode, ApiError> {
    if users.delete(id).await.map_err(internal_error)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found())
    }
}

type ApiError = (StatusCode, &'static str);

fn internal_error(_: DbErr) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
}

fn not_found() -> ApiError {
    (StatusCode::NOT_FOUND, "user not found")
}
