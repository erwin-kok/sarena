use rscni_plugin::types::Args;

use crate::{Res, args::ArgsSpec};

pub(crate) async fn status(_args: Args, _cni_args: ArgsSpec) -> Res<()> {
    Ok(())
}
