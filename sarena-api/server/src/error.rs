use axum::Json;
use hyper::StatusCode;

pub type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;
