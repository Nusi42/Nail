//! Contacts, groups, duplicates, merging and photos.

use std::collections::HashSet;

use base64::Engine as _;
use nail_api::pim::contact::{ContactPhoto, MAX_CONTACT_PHOTO_BYTES, VCardVersion};
use nail_api::pim::{
    Contact, ContactFields, ContactGroup, ContactMatch, PimIssue, PimItemRef, PimItemSync,
    PimSourceKind,
};
use nail_api::{AddressBookId, ApiError, ContactGroupId, ContactId, ErrorCode, Request, Response};
use nail_ical::vcard::{CardWriteOptions, GroupCard, GroupStyle, uid_key};
use nail_store::pim::PimItem;

use super::{blob_bytes, forbidden, invalid, new_id, new_uuid, not_found, op_ids};
use crate::error::Result;
use crate::service::Service;

pub(super) async fn handle(service: &Service, request: Request) -> Result<Response> {
    Ok(match request {
        Request::ListContacts {
            address_book_ids,
            group_id,
            offset,
            limit,
        } => {
            let limit = limit.clamp(1, crate::MAX_PAGE_SIZE);
            let (total, contacts) = service
                .inner
                .store
                .call(move |s| {
                    s.list_contacts(
                        address_book_ids.as_deref(),
                        group_id.as_ref(),
                        offset,
                        limit,
                    )
                })
                .await?;
            Response::Contacts {
                total,
                offset,
                contacts,
            }
        }
        Request::GetContact { contact_id } => Response::PimContact {
            contact: Box::new(service.contact(&contact_id).await?),
        },
        Request::CreateContact {
            address_book_id,
            fields,
        } => {
            let (contact, ops) = service.create_contact(address_book_id, *fields).await?;
            Response::ContactSaved {
                contact: Box::new(contact),
                op_ids: ops,
            }
        }
        Request::UpdateContact { contact_id, fields } => {
            let (contact, ops) = service.update_contact(contact_id, *fields).await?;
            Response::ContactSaved {
                contact: Box::new(contact),
                op_ids: ops,
            }
        }
        Request::DeleteContacts { contact_ids } => {
            let mut ops = Vec::new();
            for id in contact_ids {
                let contact = service.contact(&id).await?;
                if contact.read_only {
                    return Err(forbidden(format!("contact {id} is read-only")));
                }
                let write = service
                    .delete_item(
                        PimItemRef::Contact { contact_id: id },
                        contact.address_book_id.as_str(),
                    )
                    .await?;
                ops.extend(op_ids(&write.op_ids));
            }
            Response::PimDeleted { op_ids: ops }
        }
        Request::MoveContact {
            contact_id,
            address_book_id,
        } => {
            let contact = service.contact(&contact_id).await?;
            if contact.read_only {
                return Err(forbidden(format!("contact {contact_id} is read-only")));
            }
            service.writable_book(&address_book_id).await?;
            let ops = service
                .move_item(
                    PimItemRef::Contact {
                        contact_id: contact_id.clone(),
                    },
                    contact.address_book_id.as_str(),
                    address_book_id.as_str(),
                )
                .await?;
            Response::ContactSaved {
                contact: Box::new(service.contact(&contact_id).await?),
                op_ids: ops,
            }
        }
        Request::SearchContacts {
            query,
            address_book_ids,
            limit,
        } => Response::ContactMatches {
            matches: service
                .search_contacts(query, address_book_ids, limit)
                .await?,
        },
        Request::FindDuplicateContacts { address_book_ids } => Response::ContactDuplicates {
            groups: service
                .inner
                .store
                .call(move |s| s.find_duplicate_contacts(address_book_ids.as_deref()))
                .await?,
        },
        Request::MergeContacts {
            primary_id,
            contact_ids,
            fields,
        } => {
            let (contact, ops) = service
                .merge_contacts(primary_id, contact_ids, *fields)
                .await?;
            Response::ContactSaved {
                contact: Box::new(contact),
                op_ids: ops,
            }
        }
        Request::UnlinkContact { contact_id } => {
            let contact = service.contact(&contact_id).await?;
            service
                .inner
                .store
                .call(move |s| s.unlink_contact(&contact_id))
                .await?;
            service.broadcast(nail_api::Event::ContactsChanged {
                address_book_ids: vec![contact.address_book_id],
            });
            Response::Done
        }
        Request::GetContactPhoto { contact_id } => {
            let contact = service.contact(&contact_id).await?;
            let photo = contact
                .fields
                .photo
                .as_ref()
                .ok_or_else(|| not_found(format!("photo of {contact_id}")))?;
            let data = match (&photo.data, &photo.sha256) {
                (Some(data), _) => data.clone(),
                (None, Some(sha)) => {
                    let blobs = service.blobs().clone();
                    let sha = sha.clone();
                    let bytes = tokio::task::spawn_blocking(move || blob_bytes(&blobs, &sha))
                        .await?
                        .ok_or_else(|| not_found(format!("photo of {contact_id}")))?;
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                }
                (None, None) => return Err(not_found(format!("photo of {contact_id}"))),
            };
            Response::ContactPhoto {
                contact_id,
                mime_type: photo
                    .mime_type
                    .clone()
                    .unwrap_or_else(|| "image/jpeg".to_owned()),
                data,
            }
        }
        Request::ListContactGroups { address_book_ids } => {
            let mut groups = service
                .inner
                .store
                .call(move |s| s.contact_groups(address_book_ids.as_deref()))
                .await?;
            groups.sort_by_key(|g| g.name.to_lowercase());
            Response::ContactGroups { groups }
        }
        Request::CreateContactGroup {
            address_book_id,
            name,
            member_ids,
            member_emails,
        } => {
            let (group, ops) = service
                .save_group(None, address_book_id, name, member_ids, member_emails)
                .await?;
            Response::ContactGroupSaved {
                group: Box::new(group),
                op_ids: ops,
            }
        }
        Request::UpdateContactGroup {
            group_id,
            name,
            member_ids,
            member_emails,
        } => {
            let group = service.group(&group_id).await?;
            let (group, ops) = service
                .save_group(
                    Some(group),
                    AddressBookId::new(""),
                    name,
                    member_ids,
                    member_emails,
                )
                .await?;
            Response::ContactGroupSaved {
                group: Box::new(group),
                op_ids: ops,
            }
        }
        Request::DeleteContactGroup { group_id } => {
            let group = service.group(&group_id).await?;
            if group.read_only {
                return Err(forbidden(format!("group {group_id} is read-only")));
            }
            let write = service
                .delete_item(
                    PimItemRef::ContactGroup { group_id },
                    group.address_book_id.as_str(),
                )
                .await?;
            Response::PimDeleted {
                op_ids: op_ids(&write.op_ids),
            }
        }
        other => {
            return Err(ApiError::internal(format!(
                "{} routed to PIM contacts",
                crate::service::request_name(&other)
            ))
            .into());
        }
    })
}

impl Service {
    pub(crate) async fn contact(&self, id: &ContactId) -> Result<Contact> {
        let key = id.clone();
        self.inner
            .store
            .call(move |s| s.contact(&key))
            .await?
            .ok_or_else(|| not_found(format!("contact {id}")))
    }

    async fn group(&self, id: &ContactGroupId) -> Result<ContactGroup> {
        let key = id.clone();
        self.inner
            .store
            .call(move |s| s.contact_group(&key))
            .await?
            .ok_or_else(|| not_found(format!("group {id}")))
    }

    /// A writable address book.
    async fn writable_book(&self, id: &AddressBookId) -> Result<nail_api::pim::AddressBook> {
        let book = self.address_book(id).await?;
        if book.read_only {
            return Err(forbidden(format!("address book {id} is read-only")));
        }
        Ok(book)
    }

    /// Writes a contact with `nail-ical` (patching `original`).
    pub(crate) fn contact_text(&self, contact: &Contact, original: Option<&str>) -> String {
        let blobs = self.blobs().clone();
        let lookup = move |sha: &str| blob_bytes(&blobs, sha);
        let options = CardWriteOptions {
            now: self.pim_utc_now(),
            blobs: Some(&lookup),
        };
        nail_ical::vcard::write_contact(contact, original, &options)
    }

    pub(crate) fn group_text(&self, group: &GroupCard, original: Option<&str>) -> String {
        let options = CardWriteOptions::at(self.pim_utc_now());
        nail_ical::vcard::write_group(group, original, &options)
    }

    /// Moves an uploaded photo (`data`) into the blob store.
    async fn store_photo(&self, fields: &mut ContactFields) -> Result<()> {
        let Some(photo) = fields.photo.as_mut() else {
            return Ok(());
        };
        let Some(data) = photo.data.take() else {
            return Ok(());
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.trim())
            .map_err(|_| ApiError::invalid_request("the photo is not base64"))?;
        if bytes.len() > MAX_CONTACT_PHOTO_BYTES {
            return Err(ApiError::new(ErrorCode::TooLarge, "the photo is too large").into());
        }
        let blobs = self.blobs().clone();
        let hash = tokio::task::spawn_blocking(move || blobs.put(&bytes)).await??;
        *photo = ContactPhoto {
            mime_type: photo.mime_type.clone(),
            sha256: Some(hash.as_str().to_owned()),
            url: None,
            data: None,
        };
        Ok(())
    }

    async fn create_contact(
        &self,
        book_id: AddressBookId,
        mut fields: ContactFields,
    ) -> Result<(Contact, Vec<nail_api::OpId>)> {
        let issues = fields.issues();
        if !issues.is_empty() {
            return Err(invalid(&issues));
        }
        let book = self.writable_book(&book_id).await?;
        self.store_photo(&mut fields).await?;
        let kind = self
            .inner
            .accounts
            .config()
            .pim_source(&book.source_id)
            .map(nail_api::pim::PimSourceConfig::kind);
        // vCard 3.0 is what every CardDAV server takes; local cards are 4.0.
        let version = if kind == Some(PimSourceKind::Carddav) {
            VCardVersion::V3
        } else {
            VCardVersion::V4
        };
        let uuid = new_uuid();
        let contact = Contact {
            id: ContactId::new(new_id("ct")),
            address_book_id: book_id,
            uid: match version {
                VCardVersion::V4 => format!("urn:uuid:{uuid}"),
                VCardVersion::V3 => uuid,
            },
            fields,
            version,
            group_ids: Vec::new(),
            linked_ids: Vec::new(),
            updated_at: Some(nail_store::pim::time::utc_string(self.pim_now())),
            read_only: false,
            sync: PimItemSync::Pending,
        };
        let raw = self.contact_text(&contact, None);
        let id = contact.id.clone();
        let write = self.save_item(PimItem::Contact(contact), raw).await?;
        Ok((self.contact(&id).await?, op_ids(&write.op_ids)))
    }

    async fn update_contact(
        &self,
        id: ContactId,
        mut fields: ContactFields,
    ) -> Result<(Contact, Vec<nail_api::OpId>)> {
        let issues = fields.issues();
        if !issues.is_empty() {
            return Err(invalid(&issues));
        }
        let mut contact = self.contact(&id).await?;
        if contact.read_only {
            return Err(forbidden(format!("contact {id} is read-only")));
        }
        self.store_photo(&mut fields).await?;
        let original = self
            .item_raw(&PimItemRef::Contact {
                contact_id: id.clone(),
            })
            .await?;
        contact.fields = fields;
        contact.updated_at = Some(nail_store::pim::time::utc_string(self.pim_now()));
        let raw = self.contact_text(&contact, original.as_deref());
        let write = self.save_item(PimItem::Contact(contact), raw).await?;
        Ok((self.contact(&id).await?, op_ids(&write.op_ids)))
    }

    async fn search_contacts(
        &self,
        query: String,
        books: Option<Vec<AddressBookId>>,
        limit: u32,
    ) -> Result<Vec<ContactMatch>> {
        let limit = limit.clamp(1, 200) as usize;
        let index = self.pim_index().await?;
        let found = {
            let query = query.clone();
            tokio::task::spawn_blocking(move || {
                index.search_contacts(&query, books.as_deref(), limit)
            })
            .await??
        };
        let ids: Vec<ContactId> = found.into_iter().map(|(id, _)| id).collect();
        let contacts = self
            .inner
            .store
            .call(move |s| s.contacts_by_ids(&ids))
            .await?;
        Ok(contacts
            .iter()
            .map(|c| {
                let (field, value) = nail_search::pim::contact_match(c, &query);
                ContactMatch {
                    contact: nail_store::pim::contact_summary(c),
                    field,
                    value,
                }
            })
            .collect())
    }

    async fn merge_contacts(
        &self,
        primary_id: ContactId,
        others: Vec<ContactId>,
        mut fields: ContactFields,
    ) -> Result<(Contact, Vec<nail_api::OpId>)> {
        let issues = fields.issues();
        if !issues.is_empty() {
            return Err(invalid(&issues));
        }
        if others.is_empty() || others.contains(&primary_id) {
            return Err(
                ApiError::invalid_request("merge needs other contacts than the primary").into(),
            );
        }
        let mut primary = self.contact(&primary_id).await?;
        if primary.read_only {
            return Err(forbidden(format!("contact {primary_id} is read-only")));
        }
        self.store_photo(&mut fields).await?;
        let mut folded_keys = HashSet::new();
        for id in &others {
            let other = self.contact(id).await?;
            if other.address_book_id == primary.address_book_id {
                folded_keys.insert(uid_key(&other.uid));
            }
        }
        let original = self
            .item_raw(&PimItemRef::Contact {
                contact_id: primary_id.clone(),
            })
            .await?;
        primary.fields = fields;
        primary.updated_at = Some(nail_store::pim::time::utc_string(self.pim_now()));
        let raw = self.contact_text(&primary, original.as_deref());
        let now = self.pim_now();
        let merged = primary.clone();
        let outcome = self
            .inner
            .store
            .call(move |s| s.merge_contacts(&merged, &raw, &others, now))
            .await?;
        let source = self
            .collection_source(primary.address_book_id.as_str())
            .await?;
        let mut ops = outcome.op_ids.clone();
        // Groups that listed a folded contact now list the primary.
        let primary_key = uid_key(&primary.uid);
        for group_id in &outcome.groups_to_update {
            let group = self.group(group_id).await?;
            let item = PimItemRef::ContactGroup {
                group_id: group_id.clone(),
            };
            let Some(raw) = self.item_raw(&item).await? else {
                continue;
            };
            let Some(parsed) = nail_ical::vcard::parse_cards(&raw)
                .groups
                .into_iter()
                .next()
            else {
                continue;
            };
            let mut card = parsed.group;
            let mut members: Vec<String> = Vec::new();
            for key in card.member_uids.drain(..) {
                let key = if folded_keys.contains(&key) {
                    primary_key.clone()
                } else {
                    key
                };
                if !members.contains(&key) {
                    members.push(key);
                }
            }
            card.member_uids = members.clone();
            let text = self.group_text(&card, Some(&raw));
            let all_members = members
                .iter()
                .map(|k| format!("urn:uuid:{k}"))
                .chain(card.member_emails.iter().map(|e| format!("mailto:{e}")))
                .collect();
            let write = self
                .save_item(
                    PimItem::Group {
                        group,
                        members: all_members,
                    },
                    text,
                )
                .await?;
            ops.extend(write.op_ids);
        }
        self.report_queued(&source, &outcome.op_ids);
        for id in outcome.folded.iter().chain(&outcome.linked) {
            self.pim_index_apply(
                &[],
                &[PimItemRef::Contact {
                    contact_id: id.clone(),
                }],
            )
            .await;
        }
        for id in &outcome.linked {
            self.pim_index_update(&PimItemRef::Contact {
                contact_id: id.clone(),
            })
            .await;
        }
        self.after_item_change(
            &PimItemRef::Contact {
                contact_id: primary_id.clone(),
            },
            primary.address_book_id.as_str(),
        )
        .await;
        Ok((self.contact(&primary_id).await?, op_ids(&ops)))
    }

    /// `CreateContactGroup` (`existing` `None`) / `UpdateContactGroup`.
    async fn save_group(
        &self,
        existing: Option<ContactGroup>,
        book_id: AddressBookId,
        name: String,
        member_ids: Vec<ContactId>,
        mut member_emails: Vec<String>,
    ) -> Result<(ContactGroup, Vec<nail_api::OpId>)> {
        if name.trim().is_empty() {
            return Err(invalid(&[PimIssue::MissingName]));
        }
        if member_emails
            .iter()
            .any(|e| !nail_api::pim::is_plausible_email(e))
        {
            return Err(invalid(&[PimIssue::InvalidEmail]));
        }
        member_emails.dedup();
        let book_id = existing
            .as_ref()
            .map_or(book_id, |g| g.address_book_id.clone());
        if existing.as_ref().is_some_and(|g| g.read_only) {
            return Err(forbidden("the group is read-only"));
        }
        self.writable_book(&book_id).await?;
        let ids = member_ids.clone();
        let members = self
            .inner
            .store
            .call(move |s| s.contacts_by_ids(&ids))
            .await?;
        if members.len() != member_ids.len() || members.iter().any(|c| c.address_book_id != book_id)
        {
            return Err(ApiError::invalid_request(
                "group members must be contacts of the group's address book",
            )
            .into());
        }
        let (group, original) = match existing {
            Some(group) => {
                let original = self
                    .item_raw(&PimItemRef::ContactGroup {
                        group_id: group.id.clone(),
                    })
                    .await?;
                (group, original)
            }
            None => (
                ContactGroup {
                    id: ContactGroupId::new(new_id("grp")),
                    address_book_id: book_id.clone(),
                    uid: format!("urn:uuid:{}", new_uuid()),
                    name: String::new(),
                    member_ids: Vec::new(),
                    member_emails: Vec::new(),
                    read_only: false,
                    sync: PimItemSync::Pending,
                },
                None,
            ),
        };
        let style = original
            .as_deref()
            .and_then(|raw| nail_ical::vcard::parse_cards(raw).groups.into_iter().next())
            .map(|g| (g.group.style, g.group.version));
        let card = GroupCard {
            uid: group.uid.clone(),
            name: name.trim().to_owned(),
            member_uids: members.iter().map(|c| uid_key(&c.uid)).collect(),
            member_emails: member_emails.clone(),
            version: style.map_or(VCardVersion::V4, |(_, v)| v),
            style: style.map_or(GroupStyle::Kind, |(s, _)| s),
        };
        let raw = self.group_text(&card, original.as_deref());
        let group = ContactGroup {
            name: card.name.clone(),
            member_ids,
            member_emails,
            ..group
        };
        let id = group.id.clone();
        let write = self.save_item(PimItem::group(group), raw).await?;
        Ok((self.group(&id).await?, op_ids(&write.op_ids)))
    }
}
