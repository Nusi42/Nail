//! M4c printing (ADR 0094): print jobs in memory (what to print and how,
//! read back by the UI's print view) and PDF files through the desktop
//! shell's [`nail_api::DesktopHost::print_to_pdf`].

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use nail_api::print::{PRINT_JOB_TTL_SECS, PdfFile, PrintJob, print_view_route};
use nail_api::{ApiError, LocalizedMessage, PrintJobId, Request, Response};

use crate::error::Result;
use crate::service::Service;

/// The print jobs of this run.
#[derive(Debug, Default)]
pub(crate) struct PrintCtl {
    jobs: Mutex<HashMap<PrintJobId, (PrintJob, Instant)>>,
    next: Mutex<u64>,
}

impl PrintCtl {
    fn sweep(jobs: &mut HashMap<PrintJobId, (PrintJob, Instant)>) {
        let ttl = Duration::from_secs(PRINT_JOB_TTL_SECS);
        jobs.retain(|_, (_, made)| made.elapsed() < ttl);
    }

    fn insert(&self, mut job: PrintJob) -> PrintJob {
        let id = {
            let mut next = self
                .next
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *next += 1;
            PrintJobId::new(format!("print-{}-{}", std::process::id(), *next))
        };
        job.view = print_view_route(&id);
        job.job_id = id.clone();
        let mut jobs = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::sweep(&mut jobs);
        jobs.insert(id, (job.clone(), Instant::now()));
        job
    }

    fn get(&self, id: &PrintJobId) -> Option<PrintJob> {
        let mut jobs = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::sweep(&mut jobs);
        jobs.get(id).map(|(job, _)| job.clone())
    }

    fn mark_ready(&self, id: &PrintJobId) -> bool {
        let mut jobs = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match jobs.get_mut(id) {
            Some((job, _)) => {
                job.ready = true;
                true
            }
            None => false,
        }
    }
}

fn expired(id: &PrintJobId) -> crate::error::CoreError {
    ApiError::not_found(format!("print job {id} is gone"))
        .with_localized(LocalizedMessage::new("print-job-expired"))
        .into()
}

pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    let ctl = &service.inner.print;
    match request {
        Request::CreatePrintJob { target, options } => {
            if let Some(issue) = target.issues().first() {
                return Err(ApiError::invalid_localized(
                    format!("print target: {}", issue.as_str()),
                    issue.message_id(),
                )
                .into());
            }
            let job = ctl.insert(PrintJob {
                job_id: PrintJobId::new(String::new()),
                target,
                options,
                created_at: crate::rules::rfc3339(crate::engine::unix_now()),
                view: String::new(),
                ready: false,
            });
            Ok(Response::PrintJob { job: Box::new(job) })
        }
        Request::GetPrintJob { job_id } => match ctl.get(&job_id) {
            Some(job) => Ok(Response::PrintJob { job: Box::new(job) }),
            None => Err(expired(&job_id)),
        },
        Request::PrintViewReady { job_id } => {
            if !ctl.mark_ready(&job_id) {
                return Err(expired(&job_id));
            }
            if let Some(host) = &service.inner.desktop {
                host.print_view_ready(&job_id);
            }
            Ok(Response::Done)
        }
        Request::SavePrintPdf {
            job_id,
            path,
            overwrite,
        } => {
            let job = ctl.get(&job_id).ok_or_else(|| expired(&job_id))?;
            let host = service.inner.desktop.clone().ok_or_else(|| {
                crate::error::CoreError::from(
                    ApiError::unsupported("only the desktop app writes PDF files")
                        .with_localized(LocalizedMessage::new("desktop-unavailable")),
                )
            })?;
            // Post-M4c: an existing file is replaced only when the user
            // confirmed it (`overwrite`); the shell writes the PDF under a
            // temporary name next to it, which is then renamed into place.
            let target = std::path::PathBuf::from(&path);
            let portability = |e: nail_portability::PortabilityError| {
                crate::error::CoreError::from(e.to_api_error())
            };
            nail_portability::files::check_target(&target, overwrite).map_err(portability)?;
            let temp = nail_portability::files::temp_path(&target);
            drop(std::fs::remove_file(&temp));
            let written = host.print_to_pdf(&job, &temp.display().to_string()).await;
            let size_bytes = match written {
                Ok(size) => size,
                Err(err) => {
                    drop(std::fs::remove_file(&temp));
                    return Err(err.into());
                }
            };
            nail_portability::files::persist(&temp, &target, overwrite).map_err(portability)?;
            Ok(Response::PdfSaved {
                file: PdfFile { path, size_bytes },
            })
        }
        other => {
            Err(ApiError::internal(format!("not a print request: {}", other.wire_type())).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::print::{PrintOptions, PrintTarget};

    fn job() -> PrintJob {
        PrintJob {
            job_id: PrintJobId::new(String::new()),
            target: PrintTarget::Message {
                message_id: "m1".into(),
            },
            options: PrintOptions::default(),
            created_at: "t".into(),
            view: String::new(),
            ready: false,
        }
    }

    #[test]
    fn jobs_get_ids_routes_and_ready_flags() {
        let ctl = PrintCtl::default();
        let a = ctl.insert(job());
        let b = ctl.insert(job());
        assert_ne!(a.job_id, b.job_id);
        assert_eq!(a.view, format!("#/print/{}", a.job_id));
        assert!(!ctl.get(&a.job_id).unwrap().ready);
        assert!(ctl.mark_ready(&a.job_id));
        assert!(ctl.get(&a.job_id).unwrap().ready);
        assert!(!ctl.mark_ready(&"nope".into()));
        assert!(ctl.get(&"nope".into()).is_none());
    }
}
