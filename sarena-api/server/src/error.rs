#![allow(dead_code)]

use std::fmt::Display;

use axum::{
    Json,
    response::{IntoResponse, Response},
};
use http::StatusCode;
use sarena_services_daemon::DaemonError;
use sarena_services_endpoint::EndpointError;
use sarena_services_ipam::IpamError;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    NotFound,
    BadRequest,
    Internal,
    NotImplemented,
}

#[derive(Serialize)]
struct ErrorResponse {
    code: ErrorCode,
    message: String,
}

#[derive(Debug)]
pub enum ApiError {
    NotFound(String),
    BadRequest(String),
    Internal(String),
    NotImplemented(String),
}

impl ApiError {
    pub fn bad_request<S: Into<String>>(msg: S) -> Self {
        ApiError::BadRequest(msg.into())
    }

    pub fn not_found<S: Into<String>>(msg: S) -> Self {
        ApiError::NotFound(msg.into())
    }

    pub fn internal<S: Into<String>>(msg: S) -> Self {
        ApiError::Internal(msg.into())
    }

    pub fn not_implemented<S: Into<String>>(msg: S) -> Self {
        ApiError::NotImplemented(msg.into())
    }

    fn code(&self) -> ErrorCode {
        match self {
            ApiError::NotFound(_) => ErrorCode::NotFound,
            ApiError::BadRequest(_) => ErrorCode::BadRequest,
            ApiError::Internal(_) => ErrorCode::Internal,
            ApiError::NotImplemented(_) => ErrorCode::NotImplemented,
        }
    }

    fn message(self) -> String {
        match self {
            ApiError::NotFound(msg)
            | ApiError::BadRequest(msg)
            | ApiError::Internal(msg)
            | ApiError::NotImplemented(msg) => msg,
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::NotImplemented(_) => StatusCode::NOT_IMPLEMENTED,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let code = self.code();
        let status = self.status();
        let message = self.message();

        if matches!(code, ErrorCode::Internal) {
            tracing::error!(%message, "internal error");
        }

        if matches!(code, ErrorCode::NotImplemented) {
            tracing::warn!(%message, "not implemented");
        }

        (status, Json(ErrorResponse { code, message })).into_response()
    }
}

impl From<IpamError> for ApiError {
    fn from(err: IpamError) -> Self {
        let message = err.to_string();
        match err {
            IpamError::ExcludedError(..)
            | IpamError::PoolRequiredForAllocate(..)
            | IpamError::PoolRequiredForRelease(..)
            | IpamError::AlreadyAllocated
            | IpamError::NotInRange { .. } => ApiError::bad_request(message),

            IpamError::PoolDeterminationUnavailable(_) => ApiError::not_implemented(message),

            IpamError::Ipv4Disabled
            | IpamError::Ipv6Disabled
            | IpamError::RangeFull
            | IpamError::MismatchedNetwork => ApiError::internal(message),
        }
    }
}

impl From<DaemonError> for ApiError {
    fn from(err: DaemonError) -> Self {
        match err {}
    }
}

impl From<EndpointError> for ApiError {
    fn from(err: EndpointError) -> Self {
        match err {}
    }
}

pub trait OptionExt<T> {
    fn ok_or_not_found(self, msg: &str) -> Result<T, ApiError>;
}

impl<T> OptionExt<T> for Option<T> {
    fn ok_or_not_found(self, msg: &str) -> Result<T, ApiError> {
        self.ok_or_else(|| ApiError::not_found(msg))
    }
}

pub trait ResultExt<T> {
    fn internal(self) -> Result<T, ApiError>;
}

impl<T, E: Display> ResultExt<T> for Result<T, E> {
    fn internal(self) -> Result<T, ApiError> {
        self.map_err(|e| ApiError::internal(e.to_string()))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ApiStatus {
    Ok,
    NoContent,
}

impl IntoResponse for ApiStatus {
    fn into_response(self) -> Response {
        match self {
            ApiStatus::Ok => StatusCode::OK,
            ApiStatus::NoContent => StatusCode::NO_CONTENT,
        }
        .into_response()
    }
}

pub type ApiResult<T> = Result<Json<T>, ApiError>;
pub type Res<T> = Result<T, ApiError>;
