//! The glue between an Exchange engine's own async methods (Graph, EWS;
//! each with its own error type) and the contract's engine traits
//! (`nail_api::exchange::engine`): the trait signatures are written once
//! here, the engines name the method that answers each call.

#[doc(hidden)]
pub mod __reexport {
    pub use nail_api::domain::Flags;
    pub use nail_api::engine::{BoxFuture, DeltaOutcome, EngineError, RemoteWrite, WriteOutcome};
    pub use nail_api::exchange::InferenceClassification;
    pub use nail_api::exchange::engine::{
        ExchangeMailEngine, ExchangePimEngine, ExchangePimWrite, ExchangePimWriteOutcome,
        HierarchyOutcome, PimDeltaOutcome,
    };
    pub use nail_api::exchange::pim::ExchangePimFolder;
    pub use nail_api::scheduling::InvitationResponse;
}

/// One delegating trait method (`$name` answered by `Self::$target`).
#[doc(hidden)]
#[macro_export]
macro_rules! __engine_call {
    ($name:ident => $target:ident($($arg:ident: $argty:ty),*) -> $ret:ty) => {
        fn $name<'a>(
            &'a self,
            $($arg: $argty),*
        ) -> $crate::engine_glue::__reexport::BoxFuture<
            'a,
            Result<$ret, $crate::engine_glue::__reexport::EngineError>,
        > {
            Box::pin(async move { Ok(Self::$target(self, $($arg),*).await?) })
        }
    };
}

/// Implements `ExchangeMailEngine` for `$ty`: each listed call is answered
/// by the engine's own async method of that name (same arguments, any
/// error type with `Into<EngineError>`); `server_info` and
/// `wait_for_changes` (which differ per engine) follow as `$extra` items.
#[macro_export]
macro_rules! exchange_mail_engine {
    (
        $ty:ty {
            folder_hierarchy: $hierarchy:ident,
            folder_items: $items:ident,
            fetch_mime: $mime:ident,
            write: $write:ident,
            classify: $classify:ident,
            fetch_headers: $headers:ident,
            move_to_mailbox: $move_to:ident $(,)?
        }
        $($extra:tt)*
    ) => {
        impl $crate::engine_glue::__reexport::ExchangeMailEngine for $ty {
            $crate::__engine_call!(folder_hierarchy => $hierarchy(
                mailbox: Option<&'a str>,
                cursor: Option<&'a str>
            ) -> $crate::engine_glue::__reexport::HierarchyOutcome);
            $crate::__engine_call!(folder_items => $items(
                mailbox: Option<&'a str>,
                folder: &'a str,
                cursor: Option<&'a str>,
                since: Option<i64>
            ) -> $crate::engine_glue::__reexport::DeltaOutcome);
            $crate::__engine_call!(fetch_mime => $mime(
                mailbox: Option<&'a str>,
                remote_id: &'a str
            ) -> Vec<u8>);
            $crate::__engine_call!(write => $write(
                mailbox: Option<&'a str>,
                write: &'a $crate::engine_glue::__reexport::RemoteWrite
            ) -> $crate::engine_glue::__reexport::WriteOutcome);
            $crate::__engine_call!(classify => $classify(
                mailbox: Option<&'a str>,
                remote_ids: &'a [String],
                classification: $crate::engine_glue::__reexport::InferenceClassification,
                always: bool
            ) -> ());
            $crate::__engine_call!(fetch_headers => $headers(
                mailbox: Option<&'a str>,
                remote_id: &'a str
            ) -> Vec<u8>);
            $crate::__engine_call!(move_to_mailbox => $move_to(
                from_mailbox: Option<&'a str>,
                remote_id: &'a str,
                to_mailbox: Option<&'a str>,
                to_folder: &'a str,
                flags: $crate::engine_glue::__reexport::Flags
            ) -> String);

            $($extra)*
        }
    };
}

/// Implements `ExchangePimEngine` for `$ty`: each listed call is answered
/// by the engine's own async method of that name (same arguments, any
/// error type with `Into<EngineError>`).
#[macro_export]
macro_rules! exchange_pim_engine {
    (
        $ty:ty {
            pim_folders: $folders:ident,
            pim_changes: $changes:ident,
            write_pim: $write:ident,
            respond_to_meeting: $respond:ident $(,)?
        }
        $($extra:tt)*
    ) => {
        impl $crate::engine_glue::__reexport::ExchangePimEngine for $ty {
            $crate::__engine_call!(pim_folders => $folders(
                mailbox: Option<&'a str>
            ) -> Vec<$crate::engine_glue::__reexport::ExchangePimFolder>);
            $crate::__engine_call!(pim_changes => $changes(
                mailbox: Option<&'a str>,
                folder: &'a $crate::engine_glue::__reexport::ExchangePimFolder,
                cursor: Option<&'a str>
            ) -> $crate::engine_glue::__reexport::PimDeltaOutcome);
            $crate::__engine_call!(write_pim => $write(
                mailbox: Option<&'a str>,
                write: &'a $crate::engine_glue::__reexport::ExchangePimWrite
            ) -> $crate::engine_glue::__reexport::ExchangePimWriteOutcome);
            $crate::__engine_call!(respond_to_meeting => $respond(
                mailbox: Option<&'a str>,
                event_remote_id: &'a str,
                response: $crate::engine_glue::__reexport::InvitationResponse,
                comment: Option<&'a str>,
                send: bool
            ) -> ());

            $($extra)*
        }
    };
}
