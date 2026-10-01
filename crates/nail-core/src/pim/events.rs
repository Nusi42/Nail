//! Events (series, edit scopes, instances, free time) and tasks.

use nail_api::pim::calendar::EventFields;
use nail_api::pim::{
    CalendarEvent, EditScope, FreeSlot, PimIssue, PimItemRef, PimItemSync, PimTime, Recurrence,
    Task, TaskFields, TaskStatus,
};
use nail_api::{ApiError, CalendarId, EventId, Request, Response, TaskId};
use nail_ical::edit::{self, EditError};
use nail_ical::write::WriteOptions;
use nail_store::pim::{PimItem, PimWrite};

use super::{
    Expander, blob_bytes, event_issues, forbidden, invalid, new_id, new_uuid, not_found, op_ids,
    parse_instant, request_zone, task_issues,
};
use crate::error::Result;
use crate::service::Service;

/// Longest range of `ListEventInstances` / `FindFreeTime`.
const MAX_RANGE_DAYS: i64 = 400;

pub(super) async fn handle(service: &Service, request: Request) -> Result<Response> {
    Ok(match request {
        Request::ListEventInstances {
            from,
            to,
            calendar_ids,
            tz,
        } => {
            let instances = service
                .event_instances(&from, &to, calendar_ids, &tz)
                .await?;
            Response::EventInstances {
                from,
                to,
                instances,
            }
        }
        Request::GetEvent { event_id } => Response::PimEvent {
            event: Box::new(service.event(&event_id).await?),
        },
        Request::CreateEvent {
            calendar_id,
            fields,
            recurrence,
        } => {
            let (event, ops) = service
                .create_event(calendar_id, *fields, recurrence)
                .await?;
            Response::EventSaved {
                event: Box::new(event),
                split: None,
                op_ids: ops,
            }
        }
        Request::UpdateEvent {
            event_id,
            scope,
            recurrence_id,
            fields,
            recurrence,
        } => {
            let (event, split, ops) = service
                .update_event(event_id, scope, recurrence_id, *fields, recurrence)
                .await?;
            Response::EventSaved {
                event: Box::new(event),
                split: split.map(Box::new),
                op_ids: ops,
            }
        }
        Request::DeleteEvent {
            event_id,
            scope,
            recurrence_id,
        } => Response::PimDeleted {
            op_ids: service.delete_event(event_id, scope, recurrence_id).await?,
        },
        Request::MoveEvent {
            event_id,
            calendar_id,
        } => {
            let (event, ops) = service.move_event(event_id, calendar_id).await?;
            Response::EventSaved {
                event: Box::new(event),
                split: None,
                op_ids: ops,
            }
        }
        Request::FindFreeTime {
            from,
            to,
            duration_minutes,
            calendar_ids,
            tz,
        } => Response::FreeTime {
            slots: service
                .free_time(&from, &to, duration_minutes, calendar_ids, &tz)
                .await?,
        },
        Request::ListTasks {
            calendar_ids,
            include_completed,
        } => Response::Tasks {
            tasks: service
                .inner
                .store
                .call(move |s| s.list_tasks(calendar_ids.as_deref(), include_completed))
                .await?,
        },
        Request::GetTask { task_id } => Response::PimTask {
            task: Box::new(service.task(&task_id).await?),
        },
        Request::CreateTask {
            calendar_id,
            fields,
            recurrence,
        } => {
            let (task, ops) = service
                .create_task(calendar_id, *fields, recurrence)
                .await?;
            Response::TaskSaved {
                task: Box::new(task),
                op_ids: ops,
            }
        }
        Request::UpdateTask {
            task_id,
            fields,
            recurrence,
        } => {
            let (task, ops) = service.update_task(task_id, *fields, recurrence).await?;
            Response::TaskSaved {
                task: Box::new(task),
                op_ids: ops,
            }
        }
        Request::CompleteTask { task_id, completed } => {
            let (task, ops) = service.complete_task(task_id, completed).await?;
            Response::TaskSaved {
                task: Box::new(task),
                op_ids: ops,
            }
        }
        Request::MoveTask {
            task_id,
            calendar_id,
        } => {
            let (task, ops) = service.move_task(task_id, calendar_id).await?;
            Response::TaskSaved {
                task: Box::new(task),
                op_ids: ops,
            }
        }
        Request::DeleteTask {
            task_id,
            with_subtasks,
        } => Response::PimDeleted {
            op_ids: service.delete_task(task_id, with_subtasks).await?,
        },
        other => {
            return Err(ApiError::internal(format!(
                "{} routed to PIM events",
                crate::service::request_name(&other)
            ))
            .into());
        }
    })
}

fn scope_error(err: EditError) -> crate::error::CoreError {
    invalid(&[err.issue()])
}

/// A range of RFC 3339 instants, at most [`MAX_RANGE_DAYS`] long.
fn range(from: &str, to: &str) -> Result<(i64, i64)> {
    let (from, to) = (parse_instant(from)?, parse_instant(to)?);
    if to < from || to - from > MAX_RANGE_DAYS * 86_400 {
        return Err(ApiError::invalid_request(format!(
            "the range must be at most {MAX_RANGE_DAYS} days"
        ))
        .into());
    }
    Ok((from, to))
}

impl Service {
    /// Writes an event with `nail-ical` (patching `original`).
    pub(crate) fn event_text(
        &self,
        event: &CalendarEvent,
        original: Option<&str>,
        related_to: Option<&str>,
    ) -> String {
        let blobs = self.blobs().clone();
        let lookup = move |sha: &str| blob_bytes(&blobs, sha);
        let options = WriteOptions {
            now: self.pim_utc_now(),
            blobs: Some(&lookup),
            related_to,
        };
        nail_ical::write::write_event(event, original, &options)
    }

    /// Writes a task with `nail-ical` (patching `original`).
    pub(crate) fn task_text(
        &self,
        task: &Task,
        parent_uid: Option<&str>,
        original: Option<&str>,
    ) -> String {
        let blobs = self.blobs().clone();
        let lookup = move |sha: &str| blob_bytes(&blobs, sha);
        let options = WriteOptions {
            now: self.pim_utc_now(),
            blobs: Some(&lookup),
            related_to: None,
        };
        nail_ical::write::write_task(task, parent_uid, original, &options)
    }

    pub(crate) async fn event(&self, id: &EventId) -> Result<CalendarEvent> {
        let key = id.clone();
        self.inner
            .store
            .call(move |s| s.event(&key))
            .await?
            .ok_or_else(|| not_found(format!("event {id}")))
    }

    pub(crate) async fn task(&self, id: &TaskId) -> Result<Task> {
        let key = id.clone();
        self.inner
            .store
            .call(move |s| s.task(&key))
            .await?
            .ok_or_else(|| not_found(format!("task {id}")))
    }

    /// The stored text of an item.
    pub(crate) async fn item_raw(&self, item: &PimItemRef) -> Result<Option<String>> {
        let key = item.clone();
        Ok(self
            .inner
            .store
            .call(move |s| s.pim_object(&key))
            .await?
            .map(|o| o.raw))
    }

    /// Saves a user's item and reports its queued operations.
    pub(crate) async fn save_item(&self, item: PimItem, raw: String) -> Result<PimWrite> {
        let now = self.pim_now();
        let collection = item.collection_id().to_owned();
        let write = self
            .inner
            .store
            .call(move |s| s.save_pim_item(&item, &raw, now))
            .await?;
        let source = self.collection_source(&collection).await?;
        self.report_queued(&source, &write.op_ids);
        self.after_item_change(&write.item, &collection).await;
        Ok(write)
    }

    /// Deletes a user's item and reports its queued operations.
    pub(crate) async fn delete_item(&self, item: PimItemRef, collection: &str) -> Result<PimWrite> {
        let now = self.pim_now();
        let key = item.clone();
        let write = self
            .inner
            .store
            .call(move |s| s.delete_pim_item(&key, now))
            .await?;
        let source = self.collection_source(collection).await?;
        self.report_queued(&source, &write.op_ids);
        self.pim_index_apply(&[], std::slice::from_ref(&item)).await;
        self.forget_item_reminders(&item);
        self.after_item_change(&item, collection).await;
        Ok(write)
    }

    /// A writable calendar holding `component`.
    async fn writable_calendar(
        &self,
        id: &CalendarId,
        component: nail_api::pim::CalendarComponent,
    ) -> Result<nail_api::pim::Calendar> {
        let calendar = self.calendar(id).await?;
        if calendar.read_only {
            return Err(forbidden(format!("calendar {id} is read-only")));
        }
        if !calendar.components.contains(&component) {
            return Err(ApiError::invalid_request(format!(
                "calendar {id} does not hold this kind of item"
            ))
            .into());
        }
        Ok(calendar)
    }

    async fn event_instances(
        &self,
        from: &str,
        to: &str,
        calendars: Option<Vec<CalendarId>>,
        tz: &str,
    ) -> Result<Vec<nail_api::pim::EventInstance>> {
        let (from, to) = range(from, to)?;
        request_zone(tz)?;
        let tz = tz.to_owned();
        let mut instances = self
            .inner
            .store
            .call(move |s| s.list_event_instances(from, to, calendars.as_deref(), &tz, &Expander))
            .await?;
        instances.retain(|i| i.status != nail_api::pim::calendar::EventStatus::Cancelled);
        Ok(instances)
    }

    async fn free_time(
        &self,
        from: &str,
        to: &str,
        minutes: u32,
        calendars: Option<Vec<CalendarId>>,
        tz: &str,
    ) -> Result<Vec<FreeSlot>> {
        let (from, to) = range(from, to)?;
        request_zone(tz)?;
        let tz = tz.to_owned();
        let busy = self
            .inner
            .store
            .call(move |s| s.busy_intervals(from, to, calendars.as_deref(), &tz, &Expander))
            .await?;
        Ok(
            nail_store::pim::free_slots(&busy, from, to, i64::from(minutes.max(1)) * 60)
                .into_iter()
                .map(|(s, e)| FreeSlot {
                    start: nail_store::pim::time::utc_string(s),
                    end: nail_store::pim::time::utc_string(e),
                })
                .collect(),
        )
    }

    pub(crate) async fn create_event(
        &self,
        calendar_id: CalendarId,
        fields: EventFields,
        recurrence: Option<Recurrence>,
    ) -> Result<(CalendarEvent, Vec<nail_api::OpId>)> {
        let recurrence = recurrence.filter(|r| !r.is_empty());
        let issues = event_issues(&fields, recurrence.as_ref());
        if !issues.is_empty() {
            return Err(invalid(&issues));
        }
        self.writable_calendar(&calendar_id, nail_api::pim::CalendarComponent::Event)
            .await?;
        let now = nail_store::pim::time::utc_string(self.pim_now());
        let event = CalendarEvent {
            id: EventId::new(new_id("ev")),
            calendar_id,
            uid: format!("{}@nail", new_uuid()),
            fields,
            recurrence,
            overrides: Vec::new(),
            sequence: 0,
            created_at: Some(now.clone()),
            updated_at: Some(now),
            read_only: false,
            sync: PimItemSync::Pending,
        };
        let raw = self.event_text(&event, None, None);
        let id = event.id.clone();
        let write = self.save_item(PimItem::Event(event), raw).await?;
        Ok((self.event(&id).await?, op_ids(&write.op_ids)))
    }

    async fn update_event(
        &self,
        id: EventId,
        scope: EditScope,
        recurrence_id: Option<PimTime>,
        fields: EventFields,
        recurrence: Option<Recurrence>,
    ) -> Result<(CalendarEvent, Option<CalendarEvent>, Vec<nail_api::OpId>)> {
        let recurrence = recurrence.filter(|r| !r.is_empty());
        let mut event = self.event(&id).await?;
        if event.read_only {
            return Err(forbidden(format!("event {id} is read-only")));
        }
        let scope = if event.is_recurring() {
            scope
        } else {
            EditScope::All
        };
        let issues = event_issues(
            &fields,
            if scope == EditScope::This {
                None
            } else {
                recurrence.as_ref()
            },
        );
        if !issues.is_empty() {
            return Err(invalid(&issues));
        }
        if scope.needs_occurrence() && recurrence_id.is_none() {
            return Err(invalid(&[PimIssue::InvalidScope]));
        }
        let original = self
            .item_raw(&PimItemRef::Event {
                event_id: id.clone(),
            })
            .await?;
        let now = nail_store::pim::time::utc_string(self.pim_now());
        let mut split = None;
        match scope {
            EditScope::This => {
                let rid = recurrence_id.as_ref().expect("checked above");
                edit::override_occurrence(&mut event, rid, fields).map_err(scope_error)?;
            }
            EditScope::ThisAndFollowing => {
                let rid = recurrence_id.as_ref().expect("checked above");
                edit::check_scope(&event, scope, Some(rid)).map_err(scope_error)?;
                // The editor sends the series' recurrence back unchanged
                // unless the user changed it: then the split's own (counts,
                // dates and exceptions shared out) stays.
                let recurrence = recurrence.filter(|r| Some(r) != event.recurrence.as_ref());
                match edit::split_series(
                    &mut event,
                    rid,
                    EventId::new(new_id("ev")),
                    format!("{}@nail", new_uuid()),
                ) {
                    Ok(mut new) => {
                        if let Some(r) = recurrence {
                            new.recurrence = Some(r);
                        }
                        if new.fields.start != fields.start {
                            let _ = edit::move_series(
                                &mut new,
                                fields.start.clone(),
                                fields.end.clone(),
                            );
                        }
                        new.fields = fields;
                        new.created_at = Some(now.clone());
                        new.updated_at = Some(now.clone());
                        new.sync = PimItemSync::Pending;
                        split = Some(new);
                    }
                    Err(EditError::FirstOccurrence) => {
                        let recurrence = recurrence.or_else(|| event.recurrence.clone());
                        apply_all(&mut event, fields, recurrence);
                    }
                    Err(e) => return Err(scope_error(e)),
                }
            }
            EditScope::All => apply_all(&mut event, fields, recurrence),
        }
        event.sequence += 1;
        event.updated_at = Some(now);
        let raw = self.event_text(&event, original.as_deref(), None);
        let mut ops = self
            .save_item(PimItem::Event(event.clone()), raw)
            .await?
            .op_ids;
        let split = match split {
            Some(new) => {
                let raw = self.event_text(&new, None, Some(&event.uid));
                let new_id = new.id.clone();
                ops.extend(self.save_item(PimItem::Event(new), raw).await?.op_ids);
                Some(self.event(&new_id).await?)
            }
            None => None,
        };
        Ok((self.event(&id).await?, split, op_ids(&ops)))
    }

    async fn delete_event(
        &self,
        id: EventId,
        scope: EditScope,
        recurrence_id: Option<PimTime>,
    ) -> Result<Vec<nail_api::OpId>> {
        let mut event = self.event(&id).await?;
        if event.read_only {
            return Err(forbidden(format!("event {id} is read-only")));
        }
        let scope = if event.is_recurring() {
            scope
        } else {
            EditScope::All
        };
        let item = PimItemRef::Event {
            event_id: id.clone(),
        };
        let partial = match (scope, recurrence_id.as_ref()) {
            (EditScope::All, _) => false,
            (_, None) => return Err(invalid(&[PimIssue::InvalidScope])),
            (EditScope::This, Some(rid)) => {
                edit::delete_occurrence(&mut event, rid).map_err(scope_error)?;
                true
            }
            (EditScope::ThisAndFollowing, Some(rid)) => match edit::end_before(&mut event, rid) {
                Ok(_) => true,
                Err(EditError::FirstOccurrence) => false,
                Err(e) => return Err(scope_error(e)),
            },
        };
        if !partial {
            let write = self.delete_item(item, event.calendar_id.as_str()).await?;
            return Ok(op_ids(&write.op_ids));
        }
        let original = self.item_raw(&item).await?;
        event.sequence += 1;
        event.updated_at = Some(nail_store::pim::time::utc_string(self.pim_now()));
        let raw = self.event_text(&event, original.as_deref(), None);
        let write = self.save_item(PimItem::Event(event), raw).await?;
        Ok(op_ids(&write.op_ids))
    }

    async fn move_event(
        &self,
        id: EventId,
        calendar_id: CalendarId,
    ) -> Result<(CalendarEvent, Vec<nail_api::OpId>)> {
        let event = self.event(&id).await?;
        if event.read_only {
            return Err(forbidden(format!("event {id} is read-only")));
        }
        self.writable_calendar(&calendar_id, nail_api::pim::CalendarComponent::Event)
            .await?;
        let ops = self
            .move_item(
                PimItemRef::Event {
                    event_id: id.clone(),
                },
                event.calendar_id.as_str(),
                calendar_id.as_str(),
            )
            .await?;
        Ok((self.event(&id).await?, ops))
    }

    /// Moves an item between collections (queued: upload there, delete here).
    pub(crate) async fn move_item(
        &self,
        item: PimItemRef,
        from: &str,
        to: &str,
    ) -> Result<Vec<nail_api::OpId>> {
        let now = self.pim_now();
        let key = item.clone();
        let target = to.to_owned();
        let write = self
            .inner
            .store
            .call(move |s| s.move_pim_item(&key, &target, now))
            .await?;
        let (from_source, to_source) = (
            self.collection_source(from).await?,
            self.collection_source(to).await?,
        );
        self.report_queued(&from_source, &write.op_ids);
        if to_source != from_source {
            self.wake_pim_source(&to_source);
        }
        self.after_item_change(&item, from).await;
        self.after_item_change(&item, to).await;
        Ok(op_ids(&write.op_ids))
    }

    // ------------------------------------------------------------------ tasks

    /// Checks a task's parent: in the same list, no cycle. Returns the
    /// parent's `UID`.
    async fn check_parent(
        &self,
        task_id: Option<&TaskId>,
        calendar: &CalendarId,
        parent: Option<&TaskId>,
    ) -> Result<Option<String>> {
        let Some(parent) = parent else {
            return Ok(None);
        };
        let mut current = self.task(parent).await?;
        if &current.calendar_id != calendar {
            return Err(ApiError::invalid_request("a subtask is in its parent's list").into());
        }
        let uid = current.uid.clone();
        let mut steps = 0;
        loop {
            if task_id == Some(&current.id) {
                return Err(invalid(&[PimIssue::TaskCycle]));
            }
            let Some(next) = current.fields.parent_id.clone() else {
                break;
            };
            steps += 1;
            if steps > 1000 {
                return Err(invalid(&[PimIssue::TaskCycle]));
            }
            current = self.task(&next).await?;
        }
        Ok(Some(uid))
    }

    pub(crate) async fn create_task(
        &self,
        calendar_id: CalendarId,
        fields: TaskFields,
        recurrence: Option<Recurrence>,
    ) -> Result<(Task, Vec<nail_api::OpId>)> {
        let recurrence = recurrence.filter(|r| !r.is_empty());
        let issues = task_issues(&fields, recurrence.as_ref());
        if !issues.is_empty() {
            return Err(invalid(&issues));
        }
        self.writable_calendar(&calendar_id, nail_api::pim::CalendarComponent::Task)
            .await?;
        let parent_uid = self
            .check_parent(None, &calendar_id, fields.parent_id.as_ref())
            .await?;
        let now = nail_store::pim::time::utc_string(self.pim_now());
        let task = Task {
            id: TaskId::new(new_id("task")),
            calendar_id,
            uid: format!("{}@nail", new_uuid()),
            fields,
            recurrence,
            subtask_ids: Vec::new(),
            created_at: Some(now.clone()),
            updated_at: Some(now),
            read_only: false,
            sync: PimItemSync::Pending,
        };
        let raw = self.task_text(&task, parent_uid.as_deref(), None);
        let id = task.id.clone();
        let write = self
            .save_item(PimItem::Task { task, parent_uid }, raw)
            .await?;
        Ok((self.task(&id).await?, op_ids(&write.op_ids)))
    }

    /// Saves a changed task (the parent's `UID` resolved again).
    async fn save_task(&self, task: Task) -> Result<PimWrite> {
        let parent_uid = match &task.fields.parent_id {
            Some(parent) => Some(self.task(parent).await?.uid),
            None => None,
        };
        let original = self
            .item_raw(&PimItemRef::Task {
                task_id: task.id.clone(),
            })
            .await?;
        let raw = self.task_text(&task, parent_uid.as_deref(), original.as_deref());
        self.save_item(PimItem::Task { task, parent_uid }, raw)
            .await
    }

    async fn update_task(
        &self,
        id: TaskId,
        fields: TaskFields,
        recurrence: Option<Recurrence>,
    ) -> Result<(Task, Vec<nail_api::OpId>)> {
        let recurrence = recurrence.filter(|r| !r.is_empty());
        let issues = task_issues(&fields, recurrence.as_ref());
        if !issues.is_empty() {
            return Err(invalid(&issues));
        }
        let mut task = self.task(&id).await?;
        if task.read_only {
            return Err(forbidden(format!("task {id} is read-only")));
        }
        self.check_parent(Some(&id), &task.calendar_id, fields.parent_id.as_ref())
            .await?;
        task.fields = fields;
        task.recurrence = recurrence;
        task.updated_at = Some(nail_store::pim::time::utc_string(self.pim_now()));
        let write = self.save_task(task).await?;
        Ok((self.task(&id).await?, op_ids(&write.op_ids)))
    }

    async fn complete_task(
        &self,
        id: TaskId,
        completed: bool,
    ) -> Result<(Task, Vec<nail_api::OpId>)> {
        let mut task = self.task(&id).await?;
        if task.read_only {
            return Err(forbidden(format!("task {id} is read-only")));
        }
        let now = nail_store::pim::time::utc_string(self.pim_now());
        let next = match (&task.recurrence, completed) {
            (Some(recurrence), true) => edit::advance_task(&task.fields, recurrence),
            _ => None,
        };
        match next {
            Some((fields, recurrence)) => {
                // A recurring task moves on to its next occurrence.
                let parent = task.fields.parent_id.clone();
                task.fields = fields;
                task.fields.parent_id = parent;
                task.recurrence = Some(recurrence);
            }
            None if completed => {
                task.fields.status = TaskStatus::Completed;
                task.fields.percent_complete = Some(100);
                task.fields.completed_at = Some(now.clone());
            }
            None => {
                task.fields.status = TaskStatus::NeedsAction;
                task.fields.completed_at = None;
                if task.fields.percent_complete == Some(100) {
                    task.fields.percent_complete = None;
                }
            }
        }
        task.updated_at = Some(now);
        let write = self.save_task(task).await?;
        Ok((self.task(&id).await?, op_ids(&write.op_ids)))
    }

    /// A task and its subtasks (depth first).
    async fn task_tree(&self, id: &TaskId) -> Result<Vec<Task>> {
        let mut out = Vec::new();
        let mut stack = vec![id.clone()];
        while let Some(next) = stack.pop() {
            if out.iter().any(|t: &Task| t.id == next) || out.len() > 10_000 {
                continue;
            }
            let task = self.task(&next).await?;
            stack.extend(task.subtask_ids.iter().cloned());
            out.push(task);
        }
        Ok(out)
    }

    async fn move_task(
        &self,
        id: TaskId,
        calendar_id: CalendarId,
    ) -> Result<(Task, Vec<nail_api::OpId>)> {
        let task = self.task(&id).await?;
        if task.read_only {
            return Err(forbidden(format!("task {id} is read-only")));
        }
        self.writable_calendar(&calendar_id, nail_api::pim::CalendarComponent::Task)
            .await?;
        let mut ops = Vec::new();
        if task.fields.parent_id.is_some() {
            // A moved subtask becomes a top-level task of the new list.
            let mut top = task.clone();
            top.fields.parent_id = None;
            let write = self.save_task(top).await?;
            ops.extend(op_ids(&write.op_ids));
        }
        for t in self.task_tree(&id).await? {
            ops.extend(
                self.move_item(
                    PimItemRef::Task { task_id: t.id },
                    t.calendar_id.as_str(),
                    calendar_id.as_str(),
                )
                .await?,
            );
        }
        Ok((self.task(&id).await?, ops))
    }

    async fn delete_task(&self, id: TaskId, with_subtasks: bool) -> Result<Vec<nail_api::OpId>> {
        let task = self.task(&id).await?;
        if task.read_only {
            return Err(forbidden(format!("task {id} is read-only")));
        }
        let mut ops = Vec::new();
        if with_subtasks {
            let mut tree = self.task_tree(&id).await?;
            tree.reverse();
            for t in tree {
                let write = self
                    .delete_item(PimItemRef::Task { task_id: t.id }, t.calendar_id.as_str())
                    .await?;
                ops.extend(op_ids(&write.op_ids));
            }
            return Ok(ops);
        }
        for sub in &task.subtask_ids {
            let mut child = self.task(sub).await?;
            child.fields.parent_id = task.fields.parent_id.clone();
            let write = self.save_task(child).await?;
            ops.extend(op_ids(&write.op_ids));
        }
        let write = self
            .delete_item(PimItemRef::Task { task_id: id }, task.calendar_id.as_str())
            .await?;
        ops.extend(op_ids(&write.op_ids));
        Ok(ops)
    }
}

/// Scope `all`: the fields and recurrence are replaced; a new start moves
/// the whole series (recurrence ids, `RDATE`s and `EXDATE`s follow, as
/// the UI's mock does).
fn apply_all(event: &mut CalendarEvent, fields: EventFields, recurrence: Option<Recurrence>) {
    match recurrence {
        Some(r) => {
            event.recurrence = Some(r);
            if event.fields.start != fields.start {
                let _ = edit::move_series(event, fields.start.clone(), fields.end.clone());
            }
        }
        None => {
            event.recurrence = None;
            event.overrides.clear();
        }
    }
    event.fields = fields;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_checked() {
        assert!(range("2026-10-01T00:00:00Z", "2026-10-08T00:00:00Z").is_ok());
        assert!(range("2026-10-08T00:00:00Z", "2026-10-01T00:00:00Z").is_err());
        assert!(range("2026-01-01T00:00:00Z", "2027-03-01T00:00:00Z").is_err());
        assert!(range("yesterday", "2026-10-08T00:00:00Z").is_err());
    }

    #[test]
    fn apply_all_shifts_recurring_series() {
        let fields = EventFields::new(
            "Standup",
            PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin"),
            PimTime::zoned("2026-10-05T09:15:00", "Europe/Berlin"),
        );
        let mut recurrence = Recurrence::rule(nail_api::pim::RecurrenceRule::new(
            nail_api::pim::calendar::Frequency::Daily,
        ));
        recurrence
            .exceptions
            .push(PimTime::zoned("2026-10-07T09:00:00", "Europe/Berlin"));
        let mut event = CalendarEvent {
            id: "e".into(),
            calendar_id: "c".into(),
            uid: "u".into(),
            fields: fields.clone(),
            recurrence: Some(recurrence.clone()),
            overrides: Vec::new(),
            sequence: 0,
            created_at: None,
            updated_at: None,
            read_only: false,
            sync: PimItemSync::Synced,
        };
        let mut moved = fields.clone();
        moved.start = PimTime::zoned("2026-10-05T10:00:00", "Europe/Berlin");
        moved.end = PimTime::zoned("2026-10-05T10:15:00", "Europe/Berlin");
        apply_all(&mut event, moved.clone(), Some(recurrence.clone()));
        assert_eq!(event.fields, moved);
        // Exceptions move with the start.
        assert_eq!(
            event.recurrence.as_ref().unwrap().exceptions,
            [PimTime::zoned("2026-10-07T10:00:00", "Europe/Berlin")]
        );
        apply_all(&mut event, fields.clone(), None);
        assert!(event.recurrence.is_none() && event.overrides.is_empty());
    }
}
