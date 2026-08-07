use rscni_plugin::types::{Args, CNIResult};
use tracing::instrument;

use crate::{Res, args::ArgsSpec};

#[instrument(skip_all, err)]
pub async fn add(_args: Args, _cni_args: ArgsSpec) -> Res<CNIResult> {
    Ok(CNIResult::default())
}
