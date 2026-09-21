use axum::Json;
use ipnet::IpNet;
use kube::core::{
    DynamicObject,
    admission::{AdmissionRequest, AdmissionResponse, AdmissionReview},
};
use tracing::warn;

use crate::crd::address_pool::AddressPool;

pub async fn validate(
    Json(review): Json<AdmissionReview<AddressPool>>,
) -> Json<AdmissionReview<DynamicObject>> {
    let request: AdmissionRequest<AddressPool> = match review.try_into() {
        Ok(request) => request,
        Err(_) => {
            return Json(
                AdmissionResponse::invalid("malformed AdmissionReview request").into_review(),
            );
        }
    };

    let response = AdmissionResponse::from(&request);

    let response = match &request.object {
        Some(pool) => match pool.spec.cidr.parse::<IpNet>() {
            Ok(_) => response,
            Err(err) => {
                warn!(cidr = %pool.spec.cidr, %err, "rejecting AddressPool with invalid CIDR");
                response.deny(format!(
                    "spec.cidr {:?} is not a valid CIDR: {err}",
                    pool.spec.cidr
                ))
            }
        },
        None => response,
    };

    Json(response.into_review())
}
