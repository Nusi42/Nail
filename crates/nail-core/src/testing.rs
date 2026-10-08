//! Test support (feature `testing`; the crate's own tests use it too, and
//! so do the server's): services on temporary directories and the answers
//! tests look at (wave Q2, ADR 0171).

use std::borrow::Borrow;
use std::path::Path;

use nail_api::{ErrorCode, Response};
use tempfile::TempDir;

use crate::{Service, ServiceOptions};

/// The options of a quiet test service on `data` / `config`: a seeded run
/// ([`crate::demo::demo_options`]: secrets in memory, a light OS, offline
/// key lookups) without config watching and without engines.
pub fn quiet_options(data: &Path, config: &Path) -> ServiceOptions {
    quiet_options_with(data, config, |_| {})
}

/// [`quiet_options`] with `configure` applied before the seeded-run
/// defaults fill in what it left unset.
pub fn quiet_options_with(
    data: &Path,
    config: &Path,
    configure: impl FnOnce(&mut ServiceOptions),
) -> ServiceOptions {
    let mut options = ServiceOptions::new(data, config);
    options.watch_config = false;
    options.start_engines = false;
    configure(&mut options);
    crate::demo::demo_options(options)
}

/// The options of a service that is no seeded run (the real defaults:
/// engines on, the OS keychain unless `secrets` is set) without config
/// watching.
pub fn plain_options(data: &Path, config: &Path) -> ServiceOptions {
    let mut options = ServiceOptions::new(data, config);
    options.watch_config = false;
    options
}

/// A service on fresh temporary data and config directories, which live
/// as long as it.
#[derive(Debug)]
pub struct TempService {
    /// The service.
    pub service: Service,
    /// Its data directory.
    pub data: TempDir,
    /// Its config directory.
    pub config: TempDir,
}

impl TempService {
    /// A quiet service ([`quiet_options`]).
    ///
    /// # Panics
    ///
    /// When the directories or the service cannot be created.
    pub fn quiet() -> Self {
        Self::quiet_with(&[], |_| {})
    }

    /// A quiet service with the demo corpus ([`crate::demo::seed_demo`]).
    ///
    /// # Panics
    ///
    /// When the directories, the service or the corpus cannot be created.
    #[expect(
        clippy::expect_used,
        reason = "test support: a failed seed ends the test"
    )]
    pub fn seeded() -> Self {
        let this = Self::quiet();
        crate::demo::seed_demo(this.service.store(), this.service.blobs()).expect("demo seeded");
        this
    }

    /// A quiet service whose config directory holds `files` (name,
    /// content; e.g. `accounts.kdl`), with `configure` applied to the
    /// options ([`quiet_options_with`]).
    ///
    /// # Panics
    ///
    /// When the directories, the files or the service cannot be created.
    pub fn quiet_with(files: &[(&str, &str)], configure: impl FnOnce(&mut ServiceOptions)) -> Self {
        Self::open_with(files, |data, config| {
            quiet_options_with(data, config, configure)
        })
    }

    /// A service whose config directory holds `files`, opened with the
    /// options `options` builds for the data and config directories.
    ///
    /// # Panics
    ///
    /// When the directories, the files or the service cannot be created.
    #[expect(
        clippy::expect_used,
        reason = "test support: a failed setup ends the test"
    )]
    pub fn open_with(
        files: &[(&str, &str)],
        options: impl FnOnce(&Path, &Path) -> ServiceOptions,
    ) -> Self {
        let data = tempfile::tempdir().expect("data directory");
        let config = tempfile::tempdir().expect("config directory");
        for (name, content) in files {
            std::fs::write(config.path().join(name), content).expect("config file written");
        }
        let service = Service::open(options(data.path(), config.path())).expect("service opened");
        Self {
            service,
            data,
            config,
        }
    }

    /// Opens another service on the same directories (after this one was
    /// dropped or shut down: a restart).
    ///
    /// # Panics
    ///
    /// When the service cannot be opened.
    #[expect(
        clippy::expect_used,
        reason = "test support: a failed restart ends the test"
    )]
    pub fn reopen(&self) -> Service {
        Service::open(quiet_options(self.data.path(), self.config.path()))
            .expect("service reopened")
    }

    /// The service and its directories (data, config), for tests that
    /// keep them apart.
    pub fn into_parts(self) -> (Service, TempDir, TempDir) {
        (self.service, self.data, self.config)
    }
}

/// The code and the Fluent id of an error answer.
///
/// # Panics
///
/// When the answer is not an error.
pub fn error_of(response: impl Borrow<Response>) -> (ErrorCode, Option<String>) {
    match response.borrow() {
        Response::Error {
            code, localized, ..
        } => (*code, localized.as_ref().map(|l| l.id.clone())),
        other => panic!("error expected, got {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_quiet_service_answers_and_errors_read_back() {
        let t = TempService::quiet_with(&[("accounts.kdl", "")], |o| o.watch_config = true);
        assert!(t.config.path().join("accounts.kdl").is_file());
        assert!(matches!(
            t.service.handle(nail_api::Request::Ping).await,
            Response::Pong { .. }
        ));
        let refused = t
            .service
            .handle(nail_api::Request::GetMessage { id: "nope".into() })
            .await;
        assert_eq!(error_of(&refused).0, ErrorCode::NotFound);
    }
}
