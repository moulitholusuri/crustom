//! Local username-only account access.
//!
//! Crustom is a local-first desktop workspace. A password is intentionally not
//! required: the account name identifies the local workspace and the database
//! remains protected by the operating-system account and filesystem permissions.

use crate::db::{DbError, Storage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Setup,
    Login,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("Enter a username.")]
    MissingFields,
    #[error("No local account with that username.")]
    UnknownAccount,
    #[error(
        "Username must be 3–32 characters and use only letters, numbers, dots, dashes or underscores."
    )]
    InvalidUsername,
    #[error("A local account already exists. Sign in instead.")]
    AccountExists,
    #[error("{0}")]
    Storage(String),
}

fn validate_username(username: &str) -> Result<(), AuthError> {
    let length = username.chars().count();
    if !(3..=32).contains(&length) {
        return Err(AuthError::InvalidUsername);
    }
    if !username
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(AuthError::InvalidUsername);
    }
    Ok(())
}

fn storage_error(error: DbError) -> AuthError {
    AuthError::Storage(error.to_string())
}

/// Create the single local account. Only a username is required.
pub fn create_account(storage: &Storage, username: &str) -> Result<String, AuthError> {
    let username = username.trim();
    validate_username(username)?;
    if storage.user_count().map_err(storage_error)? > 0 {
        return Err(AuthError::AccountExists);
    }

    storage.create_user(username).map_err(storage_error)?;
    let _ = storage.set_setting("last_username", username);
    Ok(username.to_owned())
}

/// Open the local workspace for an existing username.
pub fn sign_in(storage: &Storage, username: &str) -> Result<String, AuthError> {
    let username = username.trim();
    if username.is_empty() {
        return Err(AuthError::MissingFields);
    }

    let Some(username) = storage.find_username(username).map_err(storage_error)? else {
        return Err(AuthError::UnknownAccount);
    };

    let _ = storage.set_setting("last_username", &username);
    Ok(username)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_signs_in_with_only_a_username() {
        let storage = Storage::open_in_memory().expect("database opens");
        let created = create_account(&storage, "Ravi").expect("account is created");
        assert_eq!(created, "Ravi");
        assert_eq!(storage.user_count().unwrap(), 1);

        let username = sign_in(&storage, "ravi").expect("sign-in works");
        assert_eq!(username, "Ravi");
        assert_eq!(
            storage.get_setting("last_username").unwrap().as_deref(),
            Some("Ravi")
        );
    }

    #[test]
    fn rejects_duplicate_accounts() {
        let storage = Storage::open_in_memory().expect("database opens");
        create_account(&storage, "Ravi").expect("account is created");
        assert_eq!(
            create_account(&storage, "someone").unwrap_err(),
            AuthError::AccountExists
        );
    }

    #[test]
    fn rejects_empty_or_unknown_usernames() {
        let storage = Storage::open_in_memory().expect("database opens");
        create_account(&storage, "Ravi").expect("account is created");

        assert_eq!(
            sign_in(&storage, "  ").unwrap_err(),
            AuthError::MissingFields
        );
        assert_eq!(
            sign_in(&storage, "nobody").unwrap_err(),
            AuthError::UnknownAccount
        );
    }

    #[test]
    fn rejects_invalid_usernames() {
        let storage = Storage::open_in_memory().expect("database opens");
        assert_eq!(
            create_account(&storage, "ab").unwrap_err(),
            AuthError::InvalidUsername
        );
        assert_eq!(
            create_account(&storage, "bad name").unwrap_err(),
            AuthError::InvalidUsername
        );
    }
}
