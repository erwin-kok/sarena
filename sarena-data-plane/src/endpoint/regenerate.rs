use std::{fs, os::unix::fs::PermissionsExt as _};

use crate::{
    LoaderEndpoint, Res,
    endpoint::{
        directory::{
            failed_directory_path, next_directory_path, remove_dir_if_exists,
            synchronize_directories,
        },
        worker::{EndpointPhase, EndpointWorker},
    },
};

impl EndpointWorker {
    pub(crate) async fn regenerate(&mut self) -> Res<()> {
        let id = self.state.id();
        tracing::info!("regenerating endpoint {id}");

        let next_dir = next_directory_path(&self.state_dir, id);

        remove_dir_if_exists(&next_dir)?;
        fs::create_dir_all(&next_dir)?;
        fs::set_permissions(&next_dir, fs::Permissions::from_mode(0o777))?;

        let result = self.regenerate_bpf().await;
        match result {
            Ok(_revision) => {
                synchronize_directories(&self.state_dir, id)?;

                // TODO

                self.state.phase = EndpointPhase::Ready;
                self.state.generation += 1;
            }
            Err(e) => {
                self.state.phase = EndpointPhase::Failed(e.to_string());

                tracing::warn!("regeneration failed: {e}");

                let fail_dir = failed_directory_path(&self.state_dir, id);
                remove_dir_if_exists(&fail_dir)?;
                fs::rename(&next_dir, &fail_dir)?;
            }
        }

        remove_dir_if_exists(&next_dir)?;

        self.publish();

        Ok(())
    }

    pub(crate) async fn regenerate_bpf(&mut self) -> Res<u64> {
        let id = self.state.id();
        let persist = self.state.persist();
        let next_dir = next_directory_path(&self.state_dir, id);

        persist.write_state(&next_dir)?;

        self.loader
            .add_loader_endpoint(&LoaderEndpoint::Container(persist))
            .await?;

        Ok(0)
    }
}
