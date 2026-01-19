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
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

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
///
/// Cheaply cloneable (Rc clone) for sharing across closures.
#[derive(Debug, Clone)]
pub struct GitHubValidationCache {
    cache: Rc<RefCell<HashMap<GitHubRef, ValidationState>>>,
}

impl Default for GitHubValidationCache {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHubValidationCache {
    pub fn new() -> Self {
        Self {
            cache: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Get the validation state for a reference.
    /// Returns `None` if not in cache.
    pub fn get(&self, ref_: &GitHubRef) -> Option<ValidationState> {
        self.cache.borrow().get(ref_).copied()
    }

    /// Mark a reference as pending (fetch has been spawned).
    pub fn mark_pending(&self, ref_: GitHubRef) {
        self.cache
            .borrow_mut()
            .insert(ref_, ValidationState::Pending);
    }

    /// Set the validation result for a reference.
    pub fn set_result(&self, ref_: GitHubRef, valid: bool) {
        self.cache.borrow_mut().insert(
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
        self.cache.borrow().get(ref_) == Some(&ValidationState::Valid)
    }

    /// Clear all cached results (for manual refresh).
    pub fn clear(&self) {
        self.cache.borrow_mut().clear();
    }
}

/// Cache for autocomplete results.
/// Keys are "owner/repo" for recent issues, or "owner/repo:prefix" for search results.
#[derive(Clone, Default)]
pub struct AutocompleteCache {
    cache: Rc<RefCell<HashMap<String, Vec<Issue>>>>,
}

impl AutocompleteCache {
    pub fn new() -> Self {
        Self {
            cache: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Get cached results for a key.
    pub fn get(&self, key: &str) -> Option<Vec<Issue>> {
        self.cache.borrow().get(key).cloned()
    }

    /// Store results for a key.
    pub fn set(&self, key: String, issues: Vec<Issue>) {
        self.cache.borrow_mut().insert(key, issues);
    }

    /// Clear all cached results (for manual refresh).
    pub fn clear(&self) {
        self.cache.borrow_mut().clear();
    }
}

/// GitHub API client for validating references and autocomplete.
#[derive(Clone)]
pub struct GitHubClient {
    /// Token stored for lazy initialization of octocrab.
    /// Octocrab is built lazily on first use because its construction
    /// requires a Tokio runtime to be active (for tower's buffer service).
    token: String,
    /// Lazily initialized octocrab instance.
    octocrab: std::cell::OnceCell<Octocrab>,
    /// Cache for autocomplete results (recent issues and search results).
    autocomplete_cache: AutocompleteCache,
}

impl GitHubClient {
    /// Create a new GitHub client with the given personal access token.
    ///
    /// Note: The actual octocrab client is built lazily on first use,
    /// which must happen inside a Tokio runtime context.
    pub fn new(token: String) -> Self {
        Self {
            token,
            octocrab: std::cell::OnceCell::new(),
            autocomplete_cache: AutocompleteCache::new(),
        }
    }

    /// Clear the autocomplete cache (called on Ctrl+R refresh).
    pub fn clear_autocomplete_cache(&self) {
        self.autocomplete_cache.clear();
    }

    /// Get or initialize the octocrab client.
    /// Uses async_compat to provide a Tokio runtime context for octocrab's tower buffer.
    async fn client(&self) -> &Octocrab {
        // OnceCell doesn't have async get_or_init, so we check and init separately.
        // This is safe because we're single-threaded (Rc-based).
        if let Some(client) = self.octocrab.get() {
            return client;
        }

        // Build octocrab inside compat() to provide Tokio runtime context
        let client = async {
            Octocrab::builder()
                .personal_token(self.token.clone())
                .build()
                .expect("Failed to build GitHub client")
        }
        .compat()
        .await;

        // Store it (ignore if another call raced - shouldn't happen in single-threaded)
        let _ = self.octocrab.set(client);
        self.octocrab.get().unwrap()
    }

    /// Fetch issues for autocomplete.
    ///
    /// Algorithm:
    /// - If prefix is empty: return `limit` most recently updated issues/PRs
    /// - If prefix has digits: return exact match (if exists) + text search results
    ///   sorted by most recently updated
    pub async fn issues_matching_prefix(
        &self,
        owner: &str,
        repo: &str,
        prefix: &str,
        limit: usize,
    ) -> Vec<Issue> {
        // Case 1: Empty prefix - show most recently updated issues
        if prefix.is_empty() {
            return self.recent_issues(owner, repo, limit).await;
        }

        // Case 2: Numeric prefix - exact match + text search
        let prefix_num: u64 = match prefix.parse() {
            Ok(n) => n,
            Err(_) => {
                // Non-numeric prefix: just do text search
                return self.search_issues(owner, repo, prefix, limit).await;
            }
        };

        let mut results: Vec<Issue> = Vec::new();

        // Try to fetch the exact prefix number first (e.g., #25 for prefix "25")
        if let Some(issue) = self.get_issue(owner, repo, prefix_num).await {
            results.push(issue);
        }

        // Search for issues with the prefix in title/body/comments
        let search_results = self.search_issues(owner, repo, prefix, limit).await;

        for issue in search_results {
            if results.len() >= limit {
                break;
            }
            // Skip if already in results (e.g., exact match)
            if !results.iter().any(|i| i.number == issue.number) {
                results.push(issue);
            }
        }

        results.truncate(limit);
        results
    }

    /// Fetch the most recently updated issues/PRs (cached).
    async fn recent_issues(&self, owner: &str, repo: &str, limit: usize) -> Vec<Issue> {
        let cache_key = format!("{}/{}", owner, repo);

        // Check cache first
        if let Some(cached) = self.autocomplete_cache.get(&cache_key) {
            eprintln!("[recent] cache hit, {} results", cached.len());
            return cached.into_iter().take(limit).collect();
        }
        eprintln!("[recent] cache miss");

        let result = self
            .client()
            .await
            .issues(owner, repo)
            .list()
            .state(octocrab::params::State::All)
            .sort(octocrab::params::issues::Sort::Updated)
            .direction(octocrab::params::Direction::Descending)
            .per_page(limit as u8)
            .send()
            .compat()
            .await;

        let issues = match result {
            Ok(page) => page.items,
            Err(_) => vec![],
        };

        // Cache the results
        self.autocomplete_cache.set(cache_key, issues.clone());
        issues
    }

    /// Search issues by text query, sorted by most recently updated (cached).
    async fn search_issues(
        &self,
        owner: &str,
        repo: &str,
        query: &str,
        limit: usize,
    ) -> Vec<Issue> {
        let cache_key = format!("{}/{}:{}", owner, repo, query);

        // Check cache first
        if let Some(cached) = self.autocomplete_cache.get(&cache_key) {
            eprintln!(
                "[search] cache hit for {:?}, {} results",
                query,
                cached.len()
            );
            return cached.into_iter().take(limit).collect();
        }
        eprintln!("[search] cache miss for {:?}", query);

        // GitHub requires is:issue or is:pull-request, so we search both and merge.
        // Request half the limit from each to stay within the total limit.
        let half_limit = (limit / 2).max(1) as u8;

        let issue_query = format!("repo:{}/{} is:issue {}", owner, repo, query);
        let pr_query = format!("repo:{}/{} is:pull-request {}", owner, repo, query);

        let client = self.client().await;

        let issue_result = client
            .search()
            .issues_and_pull_requests(&issue_query)
            .sort("updated")
            .order("desc")
            .per_page(half_limit)
            .send()
            .compat()
            .await;

        let pr_result = client
            .search()
            .issues_and_pull_requests(&pr_query)
            .sort("updated")
            .order("desc")
            .per_page(half_limit)
            .send()
            .compat()
            .await;

        let mut issues: Vec<Issue> = Vec::new();

        if let Ok(page) = issue_result {
            issues.extend(page.items);
        }
        if let Ok(page) = pr_result {
            issues.extend(page.items);
        }

        // Sort by updated_at descending and take limit
        issues.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        issues.truncate(limit);

        eprintln!("[search] got {} results (issues + PRs)", issues.len());

        // Cache the results
        self.autocomplete_cache.set(cache_key, issues.clone());
        issues
    }

    /// Get a single issue by number.
    pub async fn get_issue(&self, owner: &str, repo: &str, number: u64) -> Option<Issue> {
        self.client()
            .await
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
            .client()
            .await
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
        self.client()
            .await
            .get(route, None::<&()>)
            .compat()
            .await
            .ok()
    }

    /// Get teams for an organization.
    /// Returns up to `limit` teams.
    pub async fn teams_for_org(&self, org: &str, limit: usize) -> Vec<RequestedTeam> {
        let result = self
            .client()
            .await
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
        self.client()
            .await
            .get(route, None::<&()>)
            .compat()
            .await
            .ok()
    }

    /// Get a commit by SHA.
    pub async fn get_commit(&self, owner: &str, repo: &str, sha: &str) -> Option<Commit> {
        let route = format!("/repos/{}/{}/commits/{}", owner, repo, sha);
        self.client()
            .await
            .get(route, None::<&()>)
            .compat()
            .await
            .ok()
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
            // Compare and File refs come from pasted URLs - assume valid
            GitHubRef::Compare { .. } | GitHubRef::File { .. } => true,
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
        let client = GitHubClient::new(token_from_env());

        // Empty prefix returns recent issues
        let recent = client
            .issues_matching_prefix("rust-lang", "rust", "", 5)
            .await;
        assert!(
            !recent.is_empty(),
            "Should return recent issues for empty prefix"
        );
        assert!(recent.len() <= 5, "Should respect limit");

        // Numeric prefix includes exact match attempt
        let with_prefix = client
            .issues_matching_prefix("rust-lang", "rust", "1", 5)
            .await;
        assert!(
            !with_prefix.is_empty(),
            "Should return results for numeric prefix"
        );
    }

    #[tokio::test]
    async fn test_get_issue() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        let issue = client.get_issue("rust-lang", "rust", 1).await;
        assert!(issue.is_some(), "Issue #1 should exist in rust-lang/rust");
    }

    #[tokio::test]
    async fn test_get_issue_not_found() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        let issue = client.get_issue("rust-lang", "rust", 999999999).await;
        assert!(issue.is_none(), "Non-existent issue should return None");
    }

    #[tokio::test]
    async fn test_users_matching_prefix() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());
        let users = client.users_matching_prefix("torvalds", 10).await;

        assert!(
            users.iter().any(|u| u.login == "torvalds"),
            "Should find torvalds when searching"
        );
    }

    #[tokio::test]
    async fn test_get_user() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        let user = client.get_user("torvalds").await;
        assert!(user.is_some(), "torvalds should exist");
        assert_eq!(user.unwrap().login, "torvalds");
    }

    #[tokio::test]
    async fn test_get_user_not_found() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        let user = client
            .get_user("this-user-definitely-does-not-exist-12345")
            .await;
        assert!(user.is_none(), "Non-existent user should return None");
    }

    #[tokio::test]
    async fn test_get_commit() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

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
        let client = GitHubClient::new(token_from_env());

        let commit = client.get_commit("rust-lang", "rust", "0000000").await;
        assert!(commit.is_none(), "Invalid commit should not be found");
    }

    #[test]
    fn test_cache_new_is_empty() {
        let cache = GitHubValidationCache::new();
        let ref_ = GitHubRef::Issue {
            owner: "rust-lang".to_string(),
            repo: "rust".to_string(),
            number: 123,
        };
        assert!(cache.get(&ref_).is_none());
        assert!(!cache.is_valid(&ref_));
    }

    #[test]
    fn test_cache_mark_pending() {
        let cache = GitHubValidationCache::new();
        let ref_ = GitHubRef::Issue {
            owner: "rust-lang".to_string(),
            repo: "rust".to_string(),
            number: 123,
        };

        cache.mark_pending(ref_.clone());
        assert_eq!(cache.get(&ref_), Some(ValidationState::Pending));
        assert!(!cache.is_valid(&ref_));
    }

    #[test]
    fn test_cache_set_result_valid() {
        let cache = GitHubValidationCache::new();
        let ref_ = GitHubRef::Issue {
            owner: "rust-lang".to_string(),
            repo: "rust".to_string(),
            number: 123,
        };

        cache.set_result(ref_.clone(), true);
        assert_eq!(cache.get(&ref_), Some(ValidationState::Valid));
        assert!(cache.is_valid(&ref_));
    }

    #[test]
    fn test_cache_set_result_invalid() {
        let cache = GitHubValidationCache::new();
        let ref_ = GitHubRef::Issue {
            owner: "rust-lang".to_string(),
            repo: "rust".to_string(),
            number: 123,
        };

        cache.set_result(ref_.clone(), false);
        assert_eq!(cache.get(&ref_), Some(ValidationState::Invalid));
        assert!(!cache.is_valid(&ref_));
    }

    #[test]
    fn test_cache_clear() {
        let cache = GitHubValidationCache::new();
        let ref_ = GitHubRef::Issue {
            owner: "rust-lang".to_string(),
            repo: "rust".to_string(),
            number: 123,
        };

        cache.set_result(ref_.clone(), true);
        assert!(cache.is_valid(&ref_));

        cache.clear();
        assert!(cache.get(&ref_).is_none());
        assert!(!cache.is_valid(&ref_));
    }

    #[test]
    fn test_cache_clone_shares_state() {
        let cache1 = GitHubValidationCache::new();
        let cache2 = cache1.clone();

        let ref_ = GitHubRef::User {
            username: "torvalds".to_string(),
        };

        cache1.set_result(ref_.clone(), true);

        // Both should see the same state
        assert!(cache1.is_valid(&ref_));
        assert!(cache2.is_valid(&ref_));
    }
}
