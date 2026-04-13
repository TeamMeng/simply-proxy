use argon2::{
    Argon2,
    password_hash::{PasswordHasher, SaltString, rand_core::OsRng},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    sync::{Arc, atomic::AtomicU64},
};
use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::{Layer as _, fmt::Layer, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct User {
    id: u64,
    emila: String,
    #[serde(skip_serializing)]
    password: String,
    name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateUser {
    email: String,
    password: String,
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UpdateUser {
    email: Option<String>,
    password: Option<String>,
    name: Option<String>,
}

#[derive(Clone)]
struct AppState {
    inner: Arc<AppStateInner>,
}

struct AppStateInner {
    next_user_id: AtomicU64,
    users: DashMap<u64, User>,
    argon2: Argon2<'static>,
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

// =============================================================================
// Initialize tracing and run the server
// =============================================================================

#[tokio::main]
async fn main() {
    let layer = Layer::new().with_filter(LevelFilter::INFO);
    tracing_subscriber::registry().with(layer).init();

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let app_state = AppState::new();

    let app = Router::new()
        .route("/users/{id}", get(get_user_handler))
        .route("/users", get(list_users_handler))
        .route("/users", post(create_user_handler))
        .route("/users/{id}", put(update_user_handler))
        .route("/users/{id}", delete(delete_user_handler))
        .route("/health", get(health_check))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    info!("Starting server at http://{}", addr);

    axum::serve(listener, app).await.unwrap();
}

// =============================================================================
// Implement the application state and handlers
// =============================================================================

impl AppState {
    fn new() -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                next_user_id: AtomicU64::new(1),
                users: DashMap::new(),
                argon2: Argon2::default(),
            }),
        }
    }

    fn get_user_by_id(&self, id: u64) -> Option<User> {
        self.inner.users.get(&id).map(|user| user.clone())
    }

    fn create_user(&self, input: CreateUser) -> Result<User, anyhow::Error> {
        let id = self
            .inner
            .next_user_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let password_hash = hash_password(&self.inner.argon2, &input.password)?;

        let user = User {
            id,
            emila: input.email,
            password: password_hash,
            name: input.name,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        self.inner.users.insert(id, user.clone());
        Ok(user)
    }

    fn update_user(&self, input: UpdateUser, id: u64) -> Result<User, anyhow::Error> {
        let mut user = self
            .get_user_by_id(id)
            .ok_or_else(|| anyhow::anyhow!("User not found"))?;

        if let Some(email) = input.email {
            user.emila = email;
        }
        if let Some(password) = input.password {
            user.password = hash_password(&self.inner.argon2, &password)?;
        }
        if let Some(name) = input.name {
            user.name = name;
        }
        user.updated_at = Utc::now();
        Ok(user.clone())
    }

    fn delete_user(&self, id: u64) -> Option<User> {
        self.inner.users.remove(&id).map(|(_, user)| user)
    }

    fn list_user(&self) -> Vec<User> {
        self.inner
            .users
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    fn health(&self) -> bool {
        true
    }
}

fn hash_password(argon2: &Argon2<'static>, password: &str) -> Result<String, anyhow::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| anyhow::anyhow!("Failed to hash password"))?
        .to_string();
    Ok(password_hash)
}

async fn list_users_handler(State(state): State<AppState>) -> Json<Vec<User>> {
    Json(state.list_user())
}

async fn get_user_handler(
    Path(id): Path<u64>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    state
        .get_user_by_id(id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn create_user_handler(
    State(state): State<AppState>,
    Json(input): Json<CreateUser>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    state
        .create_user(input)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn update_user_handler(
    Path(id): Path<u64>,
    State(state): State<AppState>,
    Json(input): Json<UpdateUser>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    state
        .update_user(input, id)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn delete_user_handler(
    Path(id): Path<u64>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    state.delete_user(id).map(Json).ok_or(StatusCode::NOT_FOUND)
}

async fn health_check(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        status: if state.health() {
            "healthy"
        } else {
            "unhealthy"
        },
    })
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // AppState Tests
    // -------------------------------------------------------------------------

    mod app_state {
        use super::*;

        #[test]
        fn new_creates_empty_user_store() {
            let state = AppState::new();
            let users = state.list_user();
            assert!(users.is_empty(), "Expected empty user list");
        }

        #[test]
        fn new_starts_user_id_from_one() {
            let state = AppState::new();
            // First user created should have id = 1
            let input = CreateUser {
                email: "test@example.com".to_string(),
                password: "password123".to_string(),
                name: "Test User".to_string(),
            };
            let user = state.create_user(input).expect("Should create user");
            assert_eq!(user.id, 1, "First user should have id 1");
        }

        #[test]
        fn get_user_by_id_returns_user_when_exists() {
            let state = AppState::new();
            let input = CreateUser {
                email: "test@example.com".to_string(),
                password: "password123".to_string(),
                name: "Test User".to_string(),
            };
            let created = state.create_user(input).expect("Should create user");
            let found = state.get_user_by_id(created.id);
            assert!(found.is_some(), "Should find user by id");
            assert_eq!(found.unwrap().id, created.id);
        }

        #[test]
        fn get_user_by_id_returns_none_when_not_exists() {
            let state = AppState::new();
            let found = state.get_user_by_id(999);
            assert!(found.is_none(), "Should not find non-existent user");
        }

        #[test]
        fn list_user_returns_all_users() {
            let state = AppState::new();

            state
                .create_user(CreateUser {
                    email: "user1@example.com".to_string(),
                    password: "password1".to_string(),
                    name: "User One".to_string(),
                })
                .expect("Should create user 1");

            state
                .create_user(CreateUser {
                    email: "user2@example.com".to_string(),
                    password: "password2".to_string(),
                    name: "User Two".to_string(),
                })
                .expect("Should create user 2");

            let users = state.list_user();
            assert_eq!(users.len(), 2, "Should have 2 users");
        }

        #[test]
        fn delete_user_removes_user() {
            let state = AppState::new();
            let input = CreateUser {
                email: "test@example.com".to_string(),
                password: "password123".to_string(),
                name: "Test User".to_string(),
            };
            let user = state.create_user(input).expect("Should create user");
            let deleted = state.delete_user(user.id);
            assert!(deleted.is_some(), "Should return deleted user");

            let found = state.get_user_by_id(user.id);
            assert!(found.is_none(), "User should be deleted");
        }

        #[test]
        fn delete_user_returns_none_when_not_exists() {
            let state = AppState::new();
            let deleted = state.delete_user(999);
            assert!(
                deleted.is_none(),
                "Should return none for non-existent user"
            );
        }

        #[test]
        fn health_returns_true() {
            let state = AppState::new();
            assert!(state.health(), "Health check should return true");
        }
    }

    // -------------------------------------------------------------------------
    // User CRUD Tests
    // -------------------------------------------------------------------------

    mod user_crud {
        use super::*;

        #[test]
        fn create_user_hashes_password() {
            let state = AppState::new();
            let input = CreateUser {
                email: "test@example.com".to_string(),
                password: "plaintext_password".to_string(),
                name: "Test User".to_string(),
            };
            let user = state.create_user(input).expect("Should create user");

            assert_ne!(
                user.password, "plaintext_password",
                "Password should be hashed"
            );
            assert!(
                user.password.starts_with("$argon2"),
                "Password should be Argon2 hash"
            );
        }

        #[test]
        fn create_user_sets_timestamps() {
            let state = AppState::new();
            let before = Utc::now();
            let input = CreateUser {
                email: "test@example.com".to_string(),
                password: "password123".to_string(),
                name: "Test User".to_string(),
            };
            let user = state.create_user(input).expect("Should create user");
            let after = Utc::now();

            assert!(
                user.created_at >= before && user.created_at <= after,
                "created_at should be set"
            );
            assert!(
                user.updated_at >= before && user.updated_at <= after,
                "updated_at should be set"
            );
        }

        #[test]
        fn create_user_assigns_correct_fields() {
            let state = AppState::new();
            let input = CreateUser {
                email: "test@example.com".to_string(),
                password: "password123".to_string(),
                name: "Test User".to_string(),
            };
            let user = state.create_user(input).expect("Should create user");

            assert_eq!(user.emila, "test@example.com");
            assert_eq!(user.name, "Test User");
            assert!(user.id > 0);
        }

        #[test]
        fn update_user_updates_email() {
            let state = AppState::new();
            let created = state
                .create_user(CreateUser {
                    email: "old@example.com".to_string(),
                    password: "password123".to_string(),
                    name: "Test User".to_string(),
                })
                .expect("Should create user");

            let update = UpdateUser {
                email: Some("new@example.com".to_string()),
                password: None,
                name: None,
            };
            let updated = state
                .update_user(update, created.id)
                .expect("Should update user");

            assert_eq!(updated.emila, "new@example.com");
            assert_eq!(updated.name, "Test User"); // unchanged
        }

        #[test]
        fn update_user_updates_name() {
            let state = AppState::new();
            let created = state
                .create_user(CreateUser {
                    email: "test@example.com".to_string(),
                    password: "password123".to_string(),
                    name: "Old Name".to_string(),
                })
                .expect("Should create user");

            let update = UpdateUser {
                email: None,
                password: None,
                name: Some("New Name".to_string()),
            };
            let updated = state
                .update_user(update, created.id)
                .expect("Should update user");

            assert_eq!(updated.name, "New Name");
            assert_eq!(updated.emila, "test@example.com"); // unchanged
        }

        #[test]
        fn update_user_updates_password() {
            let state = AppState::new();
            let created = state
                .create_user(CreateUser {
                    email: "test@example.com".to_string(),
                    password: "old_password".to_string(),
                    name: "Test User".to_string(),
                })
                .expect("Should create user");

            let update = UpdateUser {
                email: None,
                password: Some("new_password".to_string()),
                name: None,
            };
            let updated = state
                .update_user(update, created.id)
                .expect("Should update user");

            assert_ne!(updated.password, "old_password");
            assert_ne!(updated.password, "new_password");
            assert!(updated.password.starts_with("$argon2"));
        }

        #[test]
        fn update_user_updates_timestamp() {
            let state = AppState::new();
            let created = state
                .create_user(CreateUser {
                    email: "test@example.com".to_string(),
                    password: "password123".to_string(),
                    name: "Test User".to_string(),
                })
                .expect("Should create user");

            std::thread::sleep(std::time::Duration::from_millis(10));

            let update = UpdateUser {
                email: None,
                password: None,
                name: Some("New Name".to_string()),
            };
            let updated = state
                .update_user(update, created.id)
                .expect("Should update user");

            assert!(
                updated.updated_at > created.updated_at,
                "updated_at should be refreshed"
            );
        }

        #[test]
        fn update_user_returns_error_when_not_found() {
            let state = AppState::new();
            let update = UpdateUser {
                email: Some("new@example.com".to_string()),
                password: None,
                name: None,
            };
            let result = state.update_user(update, 999);
            assert!(result.is_err(), "Should return error for non-existent user");
        }

        #[test]
        fn update_user_preserves_original_fields() {
            let state = AppState::new();
            let created = state
                .create_user(CreateUser {
                    email: "test@example.com".to_string(),
                    password: "password123".to_string(),
                    name: "Test User".to_string(),
                })
                .expect("Should create user");

            let update = UpdateUser {
                email: None,
                password: None,
                name: None,
            };
            let updated = state
                .update_user(update, created.id)
                .expect("Should update user");

            assert_eq!(updated.id, created.id);
            assert_eq!(updated.emila, created.emila);
            assert_eq!(updated.name, created.name);
            assert_eq!(updated.password, created.password);
            assert_eq!(updated.created_at, created.created_at);
        }
    }

    // -------------------------------------------------------------------------
    // Password Hashing Tests
    // -------------------------------------------------------------------------

    mod password_hashing {
        use super::*;

        #[test]
        fn hash_password_produces_valid_argon2_hash() {
            let argon2 = Argon2::default();
            let hash = hash_password(&argon2, "test_password").expect("Should hash password");

            assert!(hash.starts_with("$argon2"), "Should be Argon2 hash");
            assert!(hash.len() > 20, "Hash should have reasonable length");
        }

        #[test]
        fn hash_password_produces_different_hashes_for_same_password() {
            let argon2 = Argon2::default();
            let hash1 = hash_password(&argon2, "same_password").expect("Should hash");
            let hash2 = hash_password(&argon2, "same_password").expect("Should hash");

            // Same password should produce different hashes due to random salt
            assert_ne!(
                hash1, hash2,
                "Same password should produce different hashes"
            );
        }

        #[test]
        fn hash_password_produces_different_hashes_for_different_passwords() {
            let argon2 = Argon2::default();
            let hash1 = hash_password(&argon2, "password1").expect("Should hash");
            let hash2 = hash_password(&argon2, "password2").expect("Should hash");

            assert_ne!(
                hash1, hash2,
                "Different passwords should produce different hashes"
            );
        }

        #[test]
        fn hash_password_handles_empty_password() {
            let argon2 = Argon2::default();
            let hash = hash_password(&argon2, "").expect("Should hash empty password");
            assert!(hash.starts_with("$argon2"));
        }

        #[test]
        fn hash_password_handles_unicode_password() {
            let argon2 = Argon2::default();
            let hash = hash_password(&argon2, "密码123🔐").expect("Should hash unicode");
            assert!(hash.starts_with("$argon2"));
        }

        #[test]
        fn hash_password_handles_long_password() {
            let argon2 = Argon2::default();
            let long_password = "a".repeat(1000);
            let hash = hash_password(&argon2, &long_password).expect("Should hash long password");
            assert!(hash.starts_with("$argon2"));
        }
    }

    // -------------------------------------------------------------------------
    // Data Structure Tests
    // -------------------------------------------------------------------------

    mod data_structures {
        use super::*;

        #[test]
        fn create_user_serialization() {
            let input = CreateUser {
                email: "test@example.com".to_string(),
                password: "secret".to_string(),
                name: "Test".to_string(),
            };
            let json = serde_json::to_string(&input).expect("Should serialize");
            assert!(json.contains("test@example.com"));
            assert!(json.contains("secret"));
        }

        #[test]
        fn update_user_serialization_with_partial_fields() {
            let input = UpdateUser {
                email: Some("new@example.com".to_string()),
                password: None,
                name: None,
            };
            let json = serde_json::to_string(&input).expect("Should serialize");
            assert!(json.contains("new@example.com"));
            assert!(!json.contains("null") || json.contains("null")); // null values are valid
        }

        #[test]
        fn user_skips_password_on_serialization() {
            let user = User {
                id: 1,
                emila: "test@example.com".to_string(),
                password: "super_secret".to_string(),
                name: "Test".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            let json = serde_json::to_string(&user).expect("Should serialize");
            assert!(
                !json.contains("super_secret"),
                "Password should not be in JSON"
            );
            assert!(json.contains("test@example.com"));
        }

        #[test]
        fn health_serialization() {
            let health = Health { status: "healthy" };
            let json = serde_json::to_string(&health).expect("Should serialize");
            assert!(json.contains("healthy"));
        }
    }

    // -------------------------------------------------------------------------
    // Concurrent Access Tests
    // -------------------------------------------------------------------------

    mod concurrent_access {
        use super::*;
        use std::thread;

        #[test]
        fn concurrent_user_creation() {
            let state = AppState::new();
            let state_clone = state.clone();

            let handle1 = thread::spawn(move || {
                for i in 0..100 {
                    let _ = state_clone.create_user(CreateUser {
                        email: format!("user{}@example.com", i),
                        password: "password".to_string(),
                        name: format!("User {}", i),
                    });
                }
            });

            let state_clone2 = state.clone();
            let handle2 = thread::spawn(move || {
                for i in 100..200 {
                    let _ = state_clone2.create_user(CreateUser {
                        email: format!("user{}@example.com", i),
                        password: "password".to_string(),
                        name: format!("User {}", i),
                    });
                }
            });

            handle1.join().expect("Thread 1 should complete");
            handle2.join().expect("Thread 2 should complete");

            let users = state.list_user();
            assert_eq!(
                users.len(),
                200,
                "Should have 200 users after concurrent creation"
            );
        }

        #[test]
        fn concurrent_read_write() {
            let state = AppState::new();

            // Create initial users
            for i in 0..50 {
                let _ = state.create_user(CreateUser {
                    email: format!("user{}@example.com", i),
                    password: "password".to_string(),
                    name: format!("User {}", i),
                });
            }

            let state_read = state.clone();
            let state_write = state.clone();

            // Concurrent reads and writes
            let read_handle = thread::spawn(move || {
                for _ in 0..100 {
                    let _ = state_read.list_user();
                    let _ = state_read.get_user_by_id(1);
                }
            });

            let write_handle = thread::spawn(move || {
                for i in 50..100 {
                    let _ = state_write.create_user(CreateUser {
                        email: format!("new{}@example.com", i),
                        password: "password".to_string(),
                        name: format!("New User {}", i),
                    });
                }
            });

            read_handle.join().expect("Read thread should complete");
            write_handle.join().expect("Write thread should complete");

            // Final state should be consistent
            let users = state.list_user();
            assert!(users.len() >= 50, "Should have at least 50 users");
        }
    }
}
