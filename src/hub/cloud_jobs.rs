//! Bounded background cloud transfers and cancellation.
use super::*;

impl Hub {
    pub fn start_cloud_job(
        self: &Arc<Self>,
        id: &str,
        backup_id: &str,
        download: bool,
    ) -> Result<serde_json::Value> {
        self.entry(id)?;
        crate::backups::valid_id(backup_id)?;
        anyhow::ensure!(
            !self.cloud_cancel.load(std::sync::atomic::Ordering::SeqCst),
            "O motor está a terminar"
        );
        let mut jobs = self
            .cloud_jobs
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
        anyhow::ensure!(
            !jobs.contains(id) && jobs.len() < 2,
            "Aguarda as transferências cloud em curso"
        );
        crate::cloud::load(&self.root, id)?.context("Configura primeiro o destino cloud")?;
        crate::cloud::begin(&self.root, id)?;
        jobs.insert(id.into());
        drop(jobs);
        let hub = self.clone();
        let id = id.to_string();
        let backup = backup_id.to_string();
        thread::spawn(move || {
            let result = if download {
                crate::cloud::download(&hub.root, &id, &backup, &hub.cloud_cancel).map(|_| ())
            } else {
                crate::cloud::upload(&hub.root, &id, &backup, &hub.cloud_cancel)
            };
            if let Err(error) = result {
                let _ = crate::cloud::record_error(&hub.root, &id, &format!("{error:#}"));
                if let Ok(rt) = hub.runtime(&id) {
                    rt.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .log(format!("[Nethergate] Cloud: {error:#}"));
                }
            }
            hub.cloud_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&id);
        });
        Ok(serde_json::json!({"started":true}))
    }

    pub fn start_service_cloud_job(
        self: &Arc<Self>,
        id: &str,
        backup_id: &str,
        provider: crate::cloud::accounts::Provider,
        download: bool,
    ) -> Result<serde_json::Value> {
        self.entry(id)?;
        backups::valid_id(backup_id)?;
        anyhow::ensure!(
            !self.cloud_cancel.load(std::sync::atomic::Ordering::SeqCst),
            "O motor está a terminar"
        );
        let mut jobs = self
            .cloud_jobs
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
        anyhow::ensure!(
            !jobs.contains(id) && jobs.len() < 2,
            "Aguarda as operações cloud em curso"
        );
        crate::cloud::accounts::generation(&self.root, provider)?;
        jobs.insert(id.into());
        drop(jobs);
        let hub = self.clone();
        let id = id.to_string();
        let backup = backup_id.to_string();
        thread::spawn(move || {
            let result = if download {
                crate::cloud::services::download(
                    &hub.root,
                    &id,
                    provider,
                    &backup,
                    &hub.cloud_cancel,
                )
                .map(|_| ())
            } else {
                crate::cloud::services::upload(&hub.root, &id, provider, &backup, &hub.cloud_cancel)
            };
            if let Err(error) = result {
                let _ = crate::cloud::services::record_error(
                    &hub.root,
                    &id,
                    provider,
                    &format!("{error:#}"),
                );
            }
            hub.cloud_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&id);
        });
        Ok(serde_json::json!({"started":true}))
    }
}
