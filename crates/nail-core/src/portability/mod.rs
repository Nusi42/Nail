//! M4c data portability (ADRs 0094, 0096, 0099): imports, exports,
//! backups, restores, the diagnostics bundle and the desktop's file
//! dialog.
//!
//! `nail-portability` does the file work behind traits the core
//! implements here: [`mail`] (the import sink: Local Folders, or an
//! account's existing folders through its operation queue; the export
//! source), [`pim`] (calendars and contacts through the core's PIM import
//! path), [`backup`] (what a `.nailbackup` holds, where a restore writes)
//! and [`diagnostics`] (what the bundle holds). Jobs run on the blocking
//! pool ([`jobs`]) and report through `PortabilityJobProgress` /
//! `PortabilityJobFinished`.

mod backup;
mod diagnostics;
mod jobs;
mod mail;
mod pim;

use std::path::PathBuf;

use nail_api::backup::{BackupContents, passphrase_issues};
use nail_api::portability::{
    ExportFormat, ExportSource, ImportSource, IssueSeverity, ItemIssue, ItemRef, JobKind,
    JobReport, PortabilityIssueCode,
};
use nail_api::{ApiError, Event, LocalizedMessage, Request, Response};
use nail_portability::backup::{BackupArchive, BackupOptions, KdfParams};
use nail_portability::{export, import, thunderbird};

use crate::error::{CoreError, Result};
use crate::service::Service;

pub(crate) use diagnostics::DiagnosticsCache;
use jobs::After;
pub(crate) use jobs::PortabilityCtl;
use mail::{ImportTarget, StoreMailSource};
use pim::{PimBuffer, PimMode, StorePimSource};

impl From<nail_portability::PortabilityError> for CoreError {
    fn from(err: nail_portability::PortabilityError) -> Self {
        err.to_api_error().into()
    }
}

fn invalid(message: &str, id: &str) -> CoreError {
    ApiError::invalid_localized(message.to_owned(), id).into()
}

/// An absolute path from a request.
fn absolute(path: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path.trim());
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err(invalid(
            "the path must be absolute",
            "portability-error-path-not-absolute",
        ));
    }
    Ok(path)
}

/// An absolute path that exists.
fn existing(path: &str) -> Result<PathBuf> {
    let path = absolute(path)?;
    if !path.exists() {
        return Err(
            ApiError::not_found(format!("{} does not exist", path.display()))
                .with_localized(LocalizedMessage::new("portability-error-missing-path"))
                .into(),
        );
    }
    Ok(path)
}

pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::PickPath { pick } => {
            let host = service.inner.desktop.clone().ok_or_else(|| {
                CoreError::from(
                    ApiError::unsupported("only the desktop app has file dialogs")
                        .with_localized(LocalizedMessage::new("desktop-unavailable")),
                )
            })?;
            let path = host.pick_path(&pick).await?;
            Ok(Response::PathPicked { path })
        }
        Request::FindImportSources => {
            let sources = tokio::task::spawn_blocking(|| {
                thunderbird::default_data_dir()
                    .map(|dir| thunderbird::find_profiles(&dir))
                    .unwrap_or_default()
            })
            .await?;
            Ok(Response::ImportSources { sources })
        }
        Request::PreviewImport { source } => {
            existing(&source.path)?;
            let preview = service
                .blocking(move |s| {
                    let mut preview = import::preview(&source).map_err(CoreError::from)?;
                    thunderbird::mark_duplicates(
                        &mut preview.accounts,
                        &s.inner.accounts.config().accounts,
                    );
                    Ok(preview)
                })
                .await?;
            Ok(Response::ImportPreview {
                preview: Box::new(preview),
            })
        }
        Request::StartImport { source, options } => {
            existing(&source.path)?;
            let job = start_import(service, source, options).await?;
            Ok(Response::PortabilityJob { job: Box::new(job) })
        }
        Request::StartExport {
            source,
            format,
            path,
            options,
        } => {
            let job = start_export(service, source, format, &path, options).await?;
            Ok(Response::PortabilityJob { job: Box::new(job) })
        }
        Request::CreateBackup {
            path,
            contents,
            passphrase,
            overwrite,
        } => {
            let job = start_backup(service, &path, contents, passphrase, overwrite)?;
            Ok(Response::PortabilityJob { job: Box::new(job) })
        }
        Request::InspectBackup { path, passphrase } => {
            let path = existing(&path)?;
            let manifest = tokio::task::spawn_blocking(move || {
                nail_portability::backup::restore::inspect(&path, &passphrase)
            })
            .await?
            .map_err(CoreError::from)?;
            Ok(Response::BackupManifest {
                manifest: Box::new(manifest),
            })
        }
        Request::VerifyBackup { path, passphrase } => {
            let file = existing(&path)?;
            let work: jobs::JobWork = Box::new(move |_service, ctx| {
                let result =
                    BackupArchive::open(&file, &passphrase).and_then(|mut a| a.verify(ctx));
                (result, None)
            });
            let job = service.run_portability_job(JobKind::VerifyBackup, &path, work);
            Ok(Response::PortabilityJob { job: Box::new(job) })
        }
        Request::PreviewRestore {
            path,
            passphrase,
            contents,
        } => {
            let file = existing(&path)?;
            let plan = service
                .blocking(move |s| {
                    let mut archive =
                        BackupArchive::open(&file, &passphrase).map_err(CoreError::from)?;
                    let contents = contents.unwrap_or_else(|| archive.manifest().contents.clone());
                    let mut target = backup::CoreRestoreTarget::new(s, true);
                    archive
                        .plan(&contents, &mut target)
                        .map_err(CoreError::from)
                })
                .await?;
            Ok(Response::RestorePlan {
                plan: Box::new(plan),
            })
        }
        Request::StartRestore {
            path,
            passphrase,
            contents,
        } => {
            let job = start_restore(service, &path, passphrase, contents)?;
            Ok(Response::PortabilityJob { job: Box::new(job) })
        }
        Request::ListPortabilityJobs => Ok(Response::PortabilityJobs {
            jobs: service.inner.portability.list(),
        }),
        Request::GetPortabilityJob { job_id } => Ok(Response::PortabilityJob {
            job: Box::new(service.inner.portability.get(&job_id)?),
        }),
        Request::CancelPortabilityJob { job_id } => Ok(Response::PortabilityJob {
            job: Box::new(service.inner.portability.cancel(&job_id)?),
        }),
        Request::PreviewDiagnostics { options } => {
            let manifest = service
                .blocking(move |s| {
                    let (manifest, entries) = diagnostics::preview(s, &options)?;
                    s.inner.diagnostics.remember(&options, &manifest, &entries);
                    Ok(manifest)
                })
                .await?;
            Ok(Response::DiagnosticsPreview {
                manifest: Box::new(manifest),
            })
        }
        Request::CreateDiagnosticsBundle {
            options,
            path,
            overwrite,
        } => {
            let file = absolute(&path)?;
            let bundle = service
                .blocking(move |s| {
                    let (manifest, entries) = match s.inner.diagnostics.take(&options) {
                        Some(previewed) => previewed,
                        None => diagnostics::preview(s, &options)?,
                    };
                    nail_portability::diagnostics::write_bundle(
                        &file, &manifest, &entries, overwrite,
                    )
                    .map_err(CoreError::from)
                })
                .await?;
            Ok(Response::DiagnosticsBundle {
                bundle: Box::new(bundle),
            })
        }
        other => Err(ApiError::internal(format!(
            "{} routed to the portability module",
            other.wire_type()
        ))
        .into()),
    }
}

/// Announces what a mail target wrote and adds its facts to the report.
fn finish_mail(service: &Service, target: ImportTarget, report: &mut JobReport) {
    for (source, into) in &target.merged {
        report.push_issue(ItemIssue::new(
            PortabilityIssueCode::FolderMerged,
            IssueSeverity::Warning,
            ItemRef {
                folder: Some(source.clone()),
                label: Some(into.clone()),
                ..ItemRef::default()
            },
            format!("folder {source:?} filed into {into:?}"),
        ));
    }
    report
        .created_folders
        .extend(target.created.iter().cloned());
    service.announce_enqueued(&target.queued);
    if target.account_created || !target.created.is_empty() {
        service.broadcast(Event::AccountsChanged);
    }
    if !target.changed.is_empty() {
        let mut folder_ids: Vec<_> = target.changed.into_iter().collect();
        folder_ids.sort();
        service.broadcast(Event::MailChanged { folder_ids });
    }
}

async fn start_import(
    service: &Service,
    source: ImportSource,
    options: nail_api::portability::ImportOptions,
) -> Result<nail_api::portability::JobStatus> {
    let target = {
        let mail = options.mail.clone();
        service
            .blocking(move |s| ImportTarget::resolve(s, &mail))
            .await?
    };
    let path = source.path.clone();
    let work: jobs::JobWork = Box::new(move |service, ctx| {
        let mut target = target;
        let mut buffer = PimBuffer::new(
            service,
            PimMode::Import,
            options.calendar_id.clone(),
            options.address_book_id.clone(),
        );
        let mut result = import::run(&source, &options, &mut target, &mut buffer, ctx);
        if let Err(err) = target.flush() {
            result = result.and(Err(err.into()));
        }
        let after: After = Box::new(move |service: Service, mut report: JobReport| {
            Box::pin(async move {
                finish_mail(&service, target, &mut report);
                if !buffer.is_empty() {
                    pim::apply(&service, buffer, &mut report).await;
                }
                report
            })
        });
        (result, Some(after))
    });
    Ok(service.run_portability_job(JobKind::Import, &path, work))
}

/// The export's source, built before the job so a bad request is an error
/// answer, and whether the target is one file.
enum Exported {
    Mail(StoreMailSource, bool),
    Pim(StorePimSource, bool),
}

async fn start_export(
    service: &Service,
    source: ExportSource,
    format: ExportFormat,
    path: &str,
    options: nail_api::portability::ExportOptions,
) -> Result<nail_api::portability::JobStatus> {
    let target = absolute(path)?;
    if source.is_empty() {
        return Err(invalid(
            "nothing to export",
            "portability-error-nothing-chosen",
        ));
    }
    if !format.accepts(&source) {
        return Err(invalid(
            "this format cannot export this source",
            "portability-error-format-mismatch",
        ));
    }
    let fetch = !options.allow_headers_only;
    let locale = service.inner.locale.clone();
    let exported = service
        .blocking(move |s| {
            Ok(match source {
                ExportSource::Folders {
                    folder_ids,
                    include_subfolders,
                } => {
                    let mail = StoreMailSource::folders(s, &folder_ids, include_subfolders, fetch)?;
                    let single = format == ExportFormat::Mbox && mail.folder_count() == 1;
                    Exported::Mail(mail, single)
                }
                ExportSource::Messages { message_ids } => {
                    let name = crate::strings::message(
                        locale.as_deref(),
                        "portability-export-messages-folder",
                    );
                    let mail = StoreMailSource::messages(s, message_ids, &name, fetch);
                    Exported::Mail(mail, format == ExportFormat::Mbox)
                }
                ExportSource::Calendars { calendar_ids } => {
                    let pim = StorePimSource::calendars(s, &calendar_ids)?;
                    let single = pim.collection_count() == 1;
                    Exported::Pim(pim, single)
                }
                ExportSource::AddressBooks { address_book_ids } => {
                    let pim = StorePimSource::address_books(s, &address_book_ids)?;
                    let single = pim.collection_count() == 1;
                    Exported::Pim(pim, single)
                }
            })
        })
        .await?;
    let single = match &exported {
        Exported::Mail(_, single) | Exported::Pim(_, single) => *single,
    };
    export::check_target(&target, !single, options.overwrite).map_err(CoreError::from)?;
    let work: jobs::JobWork = Box::new(move |_service, ctx| {
        let now = chrono::Utc::now();
        let result = match exported {
            Exported::Mail(mut mail, _) => {
                export::run_mail(format, &target, &options, &mut mail, ctx, now)
            }
            Exported::Pim(mut pim, _) => export::run_pim(format, &target, &options, &mut pim, ctx),
        };
        (result, None)
    });
    Ok(service.run_portability_job(JobKind::Export, path, work))
}

fn start_backup(
    service: &Service,
    path: &str,
    contents: BackupContents,
    passphrase: String,
    overwrite: bool,
) -> Result<nail_api::portability::JobStatus> {
    let file = absolute(path)?;
    if let Some(issue) = contents.issues().first() {
        return Err(invalid(
            &format!("backup contents: {}", issue.as_str()),
            &issue.message_id(),
        ));
    }
    if let Some(issue) = passphrase_issues(&passphrase).first() {
        return Err(invalid(
            &format!("passphrase: {}", issue.as_str()),
            &issue.message_id(),
        ));
    }
    // An existing archive is replaced only when the user confirmed it
    // (`overwrite`, the Save dialog); the job then replaces it atomically.
    nail_portability::files::check_target(&file, overwrite).map_err(CoreError::from)?;
    let work: jobs::JobWork = Box::new(move |service, ctx| {
        let mut source = backup::CoreBackupSource {
            service: service.clone(),
        };
        let options = BackupOptions {
            contents,
            passphrase,
            kdf: KdfParams::STANDARD,
            overwrite,
            now: chrono::Utc::now(),
        };
        let result = nail_portability::backup::write_backup(&file, &options, &mut source, ctx);
        (result.map(|_| ()), None)
    });
    Ok(service.run_portability_job(JobKind::Backup, path, work))
}

fn start_restore(
    service: &Service,
    path: &str,
    passphrase: String,
    contents: Option<BackupContents>,
) -> Result<nail_api::portability::JobStatus> {
    let file = existing(path)?;
    let work: jobs::JobWork = Box::new(move |service, ctx| {
        let mut target = backup::CoreRestoreTarget::new(service, false);
        let result = (|| {
            let mut archive = BackupArchive::open(&file, &passphrase)?;
            let contents = contents.unwrap_or_else(|| archive.manifest().contents.clone());
            archive.restore(&contents, &mut target, ctx)?;
            target
                .finish()
                .map_err(nail_portability::PortabilityError::from)
        })();
        let after: After = Box::new(move |service: Service, mut report: JobReport| {
            Box::pin(async move {
                finish_restore(&service, target, &mut report).await;
                report
            })
        });
        (result, Some(after))
    });
    Ok(service.run_portability_job(JobKind::Restore, path, work))
}

async fn finish_restore(
    service: &Service,
    mut target: backup::CoreRestoreTarget,
    report: &mut JobReport,
) {
    service.broadcast_config(&std::mem::take(&mut target.updates));
    for file in std::mem::take(&mut target.reload) {
        match file {
            nail_api::customize::ConfigFile::Accounts => service.reload_accounts_file(),
            nail_api::customize::ConfigFile::Rules => service.reload_rules_file(),
            _ => {}
        }
    }
    for done in std::mem::take(&mut target.done) {
        finish_mail(service, done, report);
    }
    if target.plugins_installed {
        service.broadcast(Event::PluginsChanged);
    }
    let buffer = std::mem::replace(
        &mut target.pim,
        PimBuffer::new(service, PimMode::Restore, None, None),
    );
    if !buffer.is_empty() {
        pim::apply(service, buffer, report).await;
    }
}
