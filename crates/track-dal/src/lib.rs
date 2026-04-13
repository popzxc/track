// The purpose of this crate is to encapsulate the data access layer
// No DAL-specific things should leak, e.g. we should not expose
// raw `sqlx` interfaces or row types -- these are internal details
// of representation. Public types should be public first and not
// declared in this crate, so that e.g. no crate is required to depend
// on DAL just to get access to a certain type.

pub mod database;
pub mod dispatch_repository;
pub mod project_repository;
pub mod review_dispatch_repository;
pub mod review_repository;
pub mod settings_repository;
pub mod task_repository;

#[cfg(test)]
mod test_support;
