use std::{collections::HashMap, sync::Arc};

use actix_web::{
    error::InternalError, http::StatusCode, web, HttpRequest, HttpResponse, Responder, Result,
};
use serde::Deserialize;
use shared::ok;

use crate::{
    repo::{secret_repo::SecretData, user_repo::RoleType},
    server::auth_handler::must_auth,
};

use super::ServerData;

pub async fn handle_add_secret(
    sv: web::Data<Arc<ServerData>>,
    body: web::Json<AddSecretBody>,
    req: HttpRequest,
) -> Result<impl Responder> {
    must_auth(&req, vec![RoleType::FullAccess, RoleType::SuperUser])?;
    let secret_list = SecretData::list_db(&sv.repo.pool).await.map_err(|_| {
        InternalError::new(
            "Failed to get secret list",
            StatusCode::from_u16(500).unwrap(),
        )
    })?;
    if secret_list.iter().any(|s| s.key == body.key) {
        return Err(InternalError::new(
            "Secret already exists, delete it first or use another key",
            StatusCode::from_u16(409).unwrap(),
        )
        .into());
    }
    SecretData::new(body.key.to_owned(), body.value.to_owned())
        .insert_db(&sv.repo.pool)
        .await
        .map_err(|_| {
            InternalError::new(
                "Failed to insert secret",
                StatusCode::from_u16(500).unwrap(),
            )
        })?;
    ok!(HttpResponse::Ok().body("OK"))
}

#[derive(Deserialize)]
pub struct DeleteSecretBody {
    key: String,
}

pub async fn handle_delete_secret(
    sv: web::Data<Arc<ServerData>>,
    body: web::Json<DeleteSecretBody>,
    req: HttpRequest,
) -> Result<impl Responder> {
    must_auth(&req, vec![RoleType::SuperUser, RoleType::FullAccess])?;
    SecretData::delete_db(body.key.to_owned(), &sv.repo.pool)
        .await
        .map_err(|_| {
            InternalError::new(
                "Failed to delete secret",
                StatusCode::from_u16(500).unwrap(),
            )
        })?;
    ok!(HttpResponse::Ok().body("OK"))
}

pub async fn handle_show_secret(
    sv: web::Data<Arc<ServerData>>,
    body: web::Json<DeleteSecretBody>,
    req: HttpRequest,
) -> Result<impl Responder> {
    must_auth(&req, vec![RoleType::SuperUser, RoleType::FullAccess])?;
    let secret = SecretData::show_db(body.key.to_owned(), &sv.repo.pool)
        .await
        .map_err(|_| {
            InternalError::new(
                "Failed to delete secret",
                StatusCode::from_u16(500).unwrap(),
            )
        })?;
    ok!(HttpResponse::Ok().body(secret.value))
}

pub async fn handle_update_secret(
    sv: web::Data<Arc<ServerData>>,
    body: web::Json<AddSecretBody>,
    req: HttpRequest,
) -> Result<impl Responder> {
    must_auth(&req, vec![RoleType::SuperUser, RoleType::FullAccess])?;
    SecretData::update_db(body.key.to_owned(), body.value.to_owned(), &sv.repo.pool)
        .await
        .map_err(|_| {
            InternalError::new(
                "Failed to update secret",
                StatusCode::from_u16(500).unwrap(),
            )
        })?;
    ok!(HttpResponse::Ok().body("OK"))
}

pub async fn handle_list_secrets(
    sv: web::Data<Arc<ServerData>>,
    req: HttpRequest,
) -> Result<impl Responder> {
    must_auth(
        &req,
        vec![
            RoleType::SuperUser,
            RoleType::FullAccess,
            RoleType::ReadOnly,
            RoleType::UpdateOnly,
        ],
    )?;
    let secret_list: Vec<HashMap<String, String>> = SecretData::list_db(&sv.repo.pool)
        .await
        .map_err(|_| {
            InternalError::new(
                "Failed to get secret list",
                StatusCode::from_u16(500).unwrap(),
            )
        })?
        .into_iter()
        .map(|s| {
            HashMap::from([
                ("key".to_string(), s.key),
                ("created_at".to_string(), s.created_at),
            ])
        })
        .collect();
    ok!(web::Json(secret_list))
}

#[derive(Deserialize, Debug)]
pub struct AddSecretBody {
    pub key: String,
    pub value: String,
}
