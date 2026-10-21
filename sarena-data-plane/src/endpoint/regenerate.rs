use std::{fs, os::unix::fs::PermissionsExt as _};

use crate::{
    Res,
    endpoint::{
        directory::{
            backup_directory_path, failed_directory_path, next_directory_path,
            remove_dir_if_exists, state_directory_path,
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
                self.synchronize_directories()?;

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
        let next_dir = next_directory_path(&self.state_dir, id);
        self.state.persist.write_state(&next_dir)?;

        Ok(0)
    }

    pub(crate) fn synchronize_directories(&self) -> Res<()> {
        let id = self.state.id();
        let current_dir = state_directory_path(&self.state_dir, id);
        let next_dir = next_directory_path(&self.state_dir, id);
        let backup_dir = backup_directory_path(&self.state_dir, id);

        remove_dir_if_exists(&backup_dir)?;
        match fs::rename(&current_dir, &backup_dir) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            result => result?,
        }

        if let Err(e) = fs::rename(&next_dir, &current_dir) {
            let _ = fs::rename(&backup_dir, &current_dir);
            return Err(e.into());
        }

        remove_dir_if_exists(&backup_dir)?;

        Ok(())
    }
}
