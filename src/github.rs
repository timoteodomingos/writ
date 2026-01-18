//! GitHub API client for fetching issues, users, and commits.
//!
//! Used for validating GitHub references (#123, @user, etc.) and
//! providing autocomplete suggestions.

use async_compat::CompatExt;
use octocrab::Octocrab;
use octocrab::models::Author;
use octocrab::models::commits::Commit;
use octocrab::models::issues::Issue;
use octocrab::models::teams::RequestedTeam;
use std::collections::HashMap;

use crate::inline::GitHubRef;

/// Validation state for a GitHub reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationState {
    /// Fetch has been spawned but not yet completed.
    Pending,
    /// Reference exists on GitHub.
    Valid,
    /// Reference does not exist on GitHub.
    Invalid,
}

/// Cache for GitHub reference validation results.
///
/// This is a simple cache with no automatic invalidation.
/// Use `clear()` to manually refresh (e.g., via Ctrl+R keybind).
#[derive(Debug, Default)]
pub struct GitHubValidationCache {
    cache: HashMap<GitHubRef, ValidationState>,
}

impl GitHubValidationCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the validation state for a reference.
    /// Returns `None` if not in cache.
    pub fn get(&self, ref_: &GitHubRef) -> Option<ValidationState> {
        self.cache.get(ref_).copied()
    }

    /// Mark a reference as pending (fetch has been spawned).
    pub fn mark_pending(&mut self, ref_: GitHubRef) {
        self.cache.insert(ref_, ValidationState::Pending);
    }

    /// Set the validation result for a reference.
    pub fn set_result(&mut self, ref_: GitHubRef, valid: bool) {
        self.cache.insert(
            ref_,
            if valid {
                ValidationState::Valid
            } else {
                ValidationState::Invalid
            },
        );
    }

    /// Check if a reference is validated as valid.
    pub fn is_valid(&self, ref_: &GitHubRef) -> bool {
        self.cache.get(ref_) == Some(&ValidationState::Valid)
    }

    /// Clear all cached results (for manual refresh).
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

/// GitHub API client for validating references and autocomplete.
pub struct GitHubClient {
    octocrab: Octocrab,
}

impl GitHubClient {
    /// Create a new GitHub client with the given personal access token.
    ///
    /// Note: Requires `rustls::crypto::ring::default_provider().install_default()`
    /// to be called before use (typically in main()).
    pub fn new(token: String) -> Result<Self, octocrab::Error> {
        let octocrab = Octocrab::builder().personal_token(token).build()?;
        Ok(Self { octocrab })
    }

    /// Fetch issues matching a number prefix.
    /// Returns up to `limit` issues whose number starts with `prefix`.
    pub async fn issues_matching_prefix(
        &self,
        owner: &str,
        repo: &str,
        prefix: &str,
        limit: usize,
    ) -> Vec<Issue> {
        // Validate prefix is numeric
        if prefix.parse::<u64>().is_err() {
            return vec![];
        }

        // Fetch recent issues (sorted by created desc = highest numbers first)
        let result = self
            .octocrab
            .issues(owner, repo)
            .list()
            .state(octocrab::params::State::All)
            .per_page(100)
            .send()
            .compat()
            .await;

        match result {
            Ok(page) => page
                .items
                .into_iter()
                .filter(|issue| issue.number.to_string().starts_with(prefix))
                .take(limit)
                .collect(),
            Err(_) => vec![],
        }
    }

    /// Get a single issue by number.
    pub async fn get_issue(&self, owner: &str, repo: &str, number: u64) -> Option<Issue> {
        self.octocrab
            .issues(owner, repo)
            .get(number)
            .compat()
            .await
            .ok()
    }

    /// Search users matching a username prefix.
    /// Returns up to `limit` users.
    pub async fn users_matching_prefix(&self, prefix: &str, limit: usize) -> Vec<Author> {
        if prefix.is_empty() {
            return vec![];
        }

        let result = self
            .octocrab
            .search()
            .users(prefix)
            .per_page(limit as u8)
            .send()
            .compat()
            .await;

        match result {
            Ok(page) => page.items,
            Err(_) => vec![],
        }
    }

    /// Get a single user by username.
    pub async fn get_user(&self, username: &str) -> Option<Author> {
        // Use HTTP API directly since there's no typed method
        let route = format!("/users/{}", username);
        self.octocrab.get(route, None::<&()>).compat().await.ok()
    }

    /// Get teams for an organization.
    /// Returns up to `limit` teams.
    pub async fn teams_for_org(&self, org: &str, limit: usize) -> Vec<RequestedTeam> {
        let result = self
            .octocrab
            .teams(org)
            .list()
            .per_page(limit as u8)
            .send()
            .compat()
            .await;

        match result {
            Ok(page) => page.items,
            Err(_) => vec![],
        }
    }

    /// Get a single team by org and slug.
    pub async fn get_team(&self, org: &str, team_slug: &str) -> Option<RequestedTeam> {
        let route = format!("/orgs/{}/teams/{}", org, team_slug);
        self.octocrab.get(route, None::<&()>).compat().await.ok()
    }

    /// Get a commit by SHA.
    pub async fn get_commit(&self, owner: &str, repo: &str, sha: &str) -> Option<Commit> {
        let route = format!("/repos/{}/{}/commits/{}", owner, repo, sha);
        self.octocrab.get(route, None::<&()>).compat().await.ok()
    }

    /// Validate a GitHub reference by checking if it exists.
    /// Returns `true` if the reference exists, `false` otherwise.
    pub async fn validate_ref(&self, ref_: &GitHubRef) -> bool {
        match ref_ {
            GitHubRef::Issue {
                owner,
                repo,
                number,
            } => self.get_issue(owner, repo, *number).await.is_some(),
            GitHubRef::User { username } => self.get_user(username).await.is_some(),
            GitHubRef::Team { org, team } => self.get_team(org, team).await.is_some(),
            GitHubRef::Commit { owner, repo, sha } => {
                self.get_commit(owner, repo, sha).await.is_some()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GITHUB_TOKEN_ENV;

    fn token_from_env() -> String {
        std::env::var(GITHUB_TOKEN_ENV).expect("GITHUB_TOKEN env var required for tests")
    }

    fn setup_crypto() {
        use std::sync::Once;
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });
    }

    #[tokio::test]
    async fn test_issues_matching_prefix() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env()).unwrap();
        // Use prefix "1512" which matches recent issues (e.g., 151200-151299)
        let issues = client
            .issues_matching_prefix("rust-lang", "rust", "1512", 10)
            .await;

        assert!(!issues.is_empty(), "Should find issues starting with 1512");
        assert!(
            issues
                .iter()
                .all(|i| i.number.to_string().starts_with("1512")),
            "All issues should start with prefix 1512"
        );
        assert!(issues.len() <= 10, "Should respect limit");
    }

    #[tokio::test]
    async fn test_get_issue() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env()).unwrap();

        let issue = client.get_issue("rust-lang", "rust", 1).await;
        assert!(issue.is_some(), "Issue #1 should exist in rust-lang/rust");
    }

    #[tokio::test]
    async fn test_get_issue_not_found() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env()).unwrap();

        let issue = client.get_issue("rust-lang", "rust", 999999999).await;
        assert!(issue.is_none(), "Non-existent issue should return None");
    }

    #[tokio::test]
    async fn test_users_matching_prefix() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env()).unwrap();
        let users = client.users_matching_prefix("torvalds", 10).await;

        assert!(
            users.iter().any(|u| u.login == "torvalds"),
            "Should find torvalds when searching"
        );
    }

    #[tokio::test]
    async fn test_get_user() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env()).unwrap();

        let user = client.get_user("torvalds").await;
        assert!(user.is_some(), "torvalds should exist");
        assert_eq!(user.unwrap().login, "torvalds");
    }

    #[tokio::test]
    async fn test_get_user_not_found() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env()).unwrap();

        let user = client
            .get_user("this-user-definitely-does-not-exist-12345")
            .await;
        assert!(user.is_none(), "Non-existent user should return None");
    }

    #[tokio::test]
    async fn test_get_commit() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env()).unwrap();

        // First commit in rust-lang/rust
        let commit = client.get_commit("rust-lang", "rust", "c01efc6").await;
        assert!(
            commit.is_some(),
            "First commit should exist in rust-lang/rust"
        );
    }

    #[tokio::test]
    async fn test_get_commit_not_found() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env()).unwrap();

        let commit = client.get_commit("rust-lang", "rust", "0000000").await;
        assert!(commit.is_none(), "Invalid commit should not be found");
    }
}
