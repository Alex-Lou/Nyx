use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: Option<i64>,
    pub url: String,
    pub title: String,
    /// Dossier d'appartenance ("" = racine) — miroir du modèle UI.
    pub folder: String,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
}

impl Bookmark {
    pub fn new(url: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: None,
            url: url.into(),
            title: title.into(),
            folder: String::new(),
            tags: vec![],
            created_at: Utc::now(),
        }
    }

    pub fn in_folder(mut self, folder: impl Into<String>) -> Self {
        self.folder = folder.into();
        self
    }
}

#[derive(Debug, Clone)]
pub struct Password {
    pub id: Option<i64>,
    pub domain: String,
    pub username: String,
    pub password: String,
    pub created_at: DateTime<Utc>,
}

impl Password {
    pub fn new(
        domain: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            id: None,
            domain: domain.into(),
            username: username.into(),
            password: password.into(),
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: Option<i64>,
    pub url: String,
    pub title: String,
    pub visited_at: DateTime<Utc>,
}
