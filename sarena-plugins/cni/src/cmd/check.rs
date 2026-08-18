use rscni_plugin::types::{Args, CNIResult};

use crate::{Res, args::ArgsSpec};

pub(crate) async fn check(_args: Args, _cni_args: ArgsSpec) -> Res<CNIResult> {
    Ok(CNIResult::default())
}
