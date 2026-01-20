//! GitHub API client using GraphQL.
//!
//! Uses GitHub's GraphQL API for search/autocomplete and validation.

use async_compat::CompatExt;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::inline::GitHubRef;

const GITHUB_GRAPHQL_URL: &str = "https://api.github.com/graphql";

// ============================================================================
// GraphQL request/response types
// ============================================================================

#[derive(Serialize)]
struct GraphQLRequest<'a> {
    query: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    variables: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct GraphQLResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphQLError>,
}

#[derive(Debug, Deserialize)]
struct GraphQLError {
    message: String,
}

// Issue/PR search response types
#[derive(Debug, Deserialize)]
struct IssueSearchData {
    search: SearchNodes,
    #[serde(default)]
    repository: Option<RepoIssueData>,
}

#[derive(Debug, Deserialize)]
struct SearchNodes {
    nodes: Vec<IssueOrPr>,
}

#[derive(Debug, Deserialize)]
struct RepoIssueData {
    #[serde(rename = "issueOrPullRequest")]
    issue_or_pull_request: Option<IssueOrPr>,
}

/// Issue/PR data from GraphQL.
#[derive(Debug, Clone, Deserialize)]
pub struct IssueOrPr {
    #[serde(rename = "__typename")]
    pub typename: String,
    pub number: u64,
    pub title: String,
    pub state: String,
}

impl IssueOrPr {
    /// Returns true if this is a pull request (vs an issue).
    pub fn is_pr(&self) -> bool {
        self.typename == "PullRequest"
    }
}

// Mentionable users response types
#[derive(Debug, Deserialize)]
struct MentionableUsersData {
    repository: Option<RepoMentionableUsers>,
}

#[derive(Debug, Deserialize)]
struct RepoMentionableUsers {
    #[serde(rename = "mentionableUsers")]
    mentionable_users: UserNodes,
}

#[derive(Debug, Deserialize)]
struct UserNodes {
    nodes: Vec<MentionableUser>,
}

/// User data from GraphQL mentionableUsers.
#[derive(Debug, Clone, Deserialize)]
pub struct MentionableUser {
    pub login: String,
    pub name: Option<String>,
}

// Validation lookup response types
#[derive(Debug, Deserialize)]
struct ValidationData {
    repository: Option<ValidationRepoData>,
    user: Option<ValidationUser>,
}

#[derive(Debug, Deserialize)]
struct ValidationRepoData {
    #[serde(rename = "issueOrPullRequest")]
    issue_or_pull_request: Option<ValidationIssue>,
    #[serde(rename = "object")]
    commit: Option<ValidationCommit>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ValidationIssue {
    number: u64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ValidationCommit {
    oid: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ValidationUser {
    login: String,
}

// Team validation
#[derive(Debug, Deserialize)]
struct TeamValidationData {
    organization: Option<TeamOrg>,
}

#[derive(Debug, Deserialize)]
struct TeamOrg {
    team: Option<ValidationTeam>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ValidationTeam {
    slug: String,
}

// ============================================================================
// Validation cache
// ============================================================================

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

    pub fn get(&self, ref_: &GitHubRef) -> Option<ValidationState> {
        self.cache.borrow().get(ref_).copied()
    }

    pub fn mark_pending(&self, ref_: GitHubRef) {
        self.cache
            .borrow_mut()
            .insert(ref_, ValidationState::Pending);
    }

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

    pub fn is_valid(&self, ref_: &GitHubRef) -> bool {
        self.cache.borrow().get(ref_) == Some(&ValidationState::Valid)
    }

    pub fn clear(&self) {
        self.cache.borrow_mut().clear();
    }
}

// ============================================================================
// Autocomplete caches
// ============================================================================

/// Cache for issue/PR autocomplete results.
#[derive(Clone, Default)]
pub struct IssueCache {
    cache: Rc<RefCell<HashMap<String, Vec<IssueOrPr>>>>,
}

impl IssueCache {
    pub fn new() -> Self {
        Self {
            cache: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn get(&self, key: &str) -> Option<Vec<IssueOrPr>> {
        self.cache.borrow().get(key).cloned()
    }

    pub fn set(&self, key: String, issues: Vec<IssueOrPr>) {
        self.cache.borrow_mut().insert(key, issues);
    }

    pub fn clear(&self) {
        self.cache.borrow_mut().clear();
    }
}

/// Cache for user autocomplete results.
#[derive(Clone, Default)]
pub struct UserCache {
    cache: Rc<RefCell<HashMap<String, Vec<MentionableUser>>>>,
}

impl UserCache {
    pub fn new() -> Self {
        Self {
            cache: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn get(&self, key: &str) -> Option<Vec<MentionableUser>> {
        self.cache.borrow().get(key).cloned()
    }

    pub fn set(&self, key: String, users: Vec<MentionableUser>) {
        self.cache.borrow_mut().insert(key, users);
    }

    pub fn clear(&self) {
        self.cache.borrow_mut().clear();
    }
}

// ============================================================================
// GitHub client
// ============================================================================

/// GitHub API client using GraphQL.
#[derive(Clone)]
pub struct GitHubClient {
    token: String,
    client: reqwest::Client,
    issue_cache: IssueCache,
    user_cache: UserCache,
}

impl GitHubClient {
    pub fn new(token: String) -> Self {
        let client = reqwest::Client::new();
        Self {
            token,
            client,
            issue_cache: IssueCache::new(),
            user_cache: UserCache::new(),
        }
    }

    pub fn clear_autocomplete_cache(&self) {
        self.issue_cache.clear();
    }

    pub fn clear_user_cache(&self) {
        self.user_cache.clear();
    }

    /// Execute a GraphQL query.
    async fn graphql<T: for<'de> Deserialize<'de>>(
        &self,
        query: &str,
        variables: Option<serde_json::Value>,
    ) -> Option<T> {
        let request = GraphQLRequest { query, variables };

        let response = self
            .client
            .post(GITHUB_GRAPHQL_URL)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "writ")
            .json(&request)
            .send()
            .compat()
            .await
            .ok()?;

        let result: GraphQLResponse<T> = response.json().compat().await.ok()?;

        if !result.errors.is_empty() {
            eprintln!(
                "[graphql] errors: {:?}",
                result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
            );
        }

        result.data
    }

    // ========================================================================
    // Issue/PR autocomplete
    // ========================================================================

    /// Fetch issues/PRs for autocomplete.
    ///
    /// - Empty prefix: returns most recently updated issues/PRs
    /// - Numeric prefix: returns exact match (if exists) + text search
    /// - Text prefix: returns text search results
    pub async fn issues_matching_prefix(
        &self,
        owner: &str,
        repo: &str,
        prefix: &str,
        limit: usize,
    ) -> Vec<IssueOrPr> {
        let cache_key = format!("{}/{}:{}", owner, repo, prefix);

        if let Some(cached) = self.issue_cache.get(&cache_key) {
            return cached.into_iter().take(limit).collect();
        }

        let results = if prefix.is_empty() {
            self.search_issues(owner, repo, None, limit).await
        } else if let Ok(number) = prefix.parse::<u64>() {
            // Numeric prefix: get exact match + search
            self.search_issues_with_exact(owner, repo, number, prefix, limit)
                .await
        } else {
            // Text prefix: just search
            self.search_issues(owner, repo, Some(prefix), limit).await
        };

        self.issue_cache.set(cache_key, results.clone());
        results
    }

    /// Search issues/PRs, optionally with a text query.
    async fn search_issues(
        &self,
        owner: &str,
        repo: &str,
        query: Option<&str>,
        limit: usize,
    ) -> Vec<IssueOrPr> {
        let search_query = match query {
            Some(q) => format!(
                "repo:{}/{} type:issue type:pr {} sort:updated",
                owner, repo, q
            ),
            None => format!("repo:{}/{} type:issue type:pr sort:updated", owner, repo),
        };

        let graphql_query = r#"
            query($query: String!, $limit: Int!) {
                search(query: $query, type: ISSUE, first: $limit) {
                    nodes {
                        __typename
                        ... on Issue { number title state }
                        ... on PullRequest { number title state }
                    }
                }
            }
        "#;

        let variables = serde_json::json!({
            "query": search_query,
            "limit": limit
        });

        let data: Option<IssueSearchData> = self.graphql(graphql_query, Some(variables)).await;

        data.map(|d| d.search.nodes).unwrap_or_default()
    }

    /// Search issues/PRs with an exact number lookup in one query.
    async fn search_issues_with_exact(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        search_text: &str,
        limit: usize,
    ) -> Vec<IssueOrPr> {
        let search_query = format!(
            "repo:{}/{} type:issue type:pr {} sort:updated",
            owner, repo, search_text
        );

        let graphql_query = r#"
            query($owner: String!, $repo: String!, $number: Int!, $query: String!, $limit: Int!) {
                repository(owner: $owner, name: $repo) {
                    issueOrPullRequest(number: $number) {
                        __typename
                        ... on Issue { number title state }
                        ... on PullRequest { number title state }
                    }
                }
                search(query: $query, type: ISSUE, first: $limit) {
                    nodes {
                        __typename
                        ... on Issue { number title state }
                        ... on PullRequest { number title state }
                    }
                }
            }
        "#;

        let variables = serde_json::json!({
            "owner": owner,
            "repo": repo,
            "number": number,
            "query": search_query,
            "limit": limit
        });

        let data: Option<IssueSearchData> = self.graphql(graphql_query, Some(variables)).await;

        let Some(data) = data else {
            return vec![];
        };

        let mut results = Vec::new();

        // Add exact match first if it exists
        if let Some(repo_data) = data.repository {
            if let Some(issue) = repo_data.issue_or_pull_request {
                results.push(issue);
            }
        }

        // Add search results, deduplicating
        for issue in data.search.nodes {
            if !results.iter().any(|i| i.number == issue.number) {
                results.push(issue);
            }
            if results.len() >= limit {
                break;
            }
        }

        results.truncate(limit);
        results
    }

    // ========================================================================
    // User autocomplete (mentionableUsers)
    // ========================================================================

    /// Fetch mentionable users for autocomplete.
    /// Uses server-side search against both login and display name.
    pub async fn users_matching_prefix(
        &self,
        owner: &str,
        repo: &str,
        prefix: &str,
        limit: usize,
    ) -> Vec<MentionableUser> {
        let cache_key = format!("{}/{}:{}", owner, repo, prefix);

        if let Some(cached) = self.user_cache.get(&cache_key) {
            return cached.into_iter().take(limit).collect();
        }

        let graphql_query = r#"
            query($owner: String!, $repo: String!, $query: String!, $limit: Int!) {
                repository(owner: $owner, name: $repo) {
                    mentionableUsers(query: $query, first: $limit) {
                        nodes {
                            login
                            name
                        }
                    }
                }
            }
        "#;

        let variables = serde_json::json!({
            "owner": owner,
            "repo": repo,
            "query": prefix,
            "limit": limit
        });

        let data: Option<MentionableUsersData> = self.graphql(graphql_query, Some(variables)).await;

        let results = data
            .and_then(|d| d.repository)
            .map(|r| r.mentionable_users.nodes)
            .unwrap_or_default();

        self.user_cache.set(cache_key, results.clone());
        results
    }

    // ========================================================================
    // Validation (for GitHubRef validation)
    // ========================================================================

    /// Validate a GitHub reference by checking if it exists.
    pub async fn validate_ref(&self, ref_: &GitHubRef) -> bool {
        match ref_ {
            GitHubRef::Issue {
                owner,
                repo,
                number,
            } => self.validate_issue(owner, repo, *number).await,
            GitHubRef::User { username } => self.validate_user(username).await,
            GitHubRef::Team { org, team } => self.validate_team(org, team).await,
            GitHubRef::Commit { owner, repo, sha } => self.validate_commit(owner, repo, sha).await,
            // Compare and File refs come from pasted URLs - assume valid
            GitHubRef::Compare { .. } | GitHubRef::File { .. } => true,
        }
    }

    async fn validate_issue(&self, owner: &str, repo: &str, number: u64) -> bool {
        let query = r#"
            query($owner: String!, $repo: String!, $number: Int!) {
                repository(owner: $owner, name: $repo) {
                    issueOrPullRequest(number: $number) {
                        ... on Issue { number }
                        ... on PullRequest { number }
                    }
                }
            }
        "#;

        let variables = serde_json::json!({
            "owner": owner,
            "repo": repo,
            "number": number
        });

        let data: Option<ValidationData> = self.graphql(query, Some(variables)).await;

        data.and_then(|d| d.repository)
            .and_then(|r| r.issue_or_pull_request)
            .is_some()
    }

    async fn validate_user(&self, username: &str) -> bool {
        let query = r#"
            query($login: String!) {
                user(login: $login) {
                    login
                }
            }
        "#;

        let variables = serde_json::json!({
            "login": username
        });

        let data: Option<ValidationData> = self.graphql(query, Some(variables)).await;

        data.and_then(|d| d.user).is_some()
    }

    async fn validate_team(&self, org: &str, team_slug: &str) -> bool {
        let query = r#"
            query($org: String!, $slug: String!) {
                organization(login: $org) {
                    team(slug: $slug) {
                        slug
                    }
                }
            }
        "#;

        let variables = serde_json::json!({
            "org": org,
            "slug": team_slug
        });

        let data: Option<TeamValidationData> = self.graphql(query, Some(variables)).await;

        data.and_then(|d| d.organization)
            .and_then(|o| o.team)
            .is_some()
    }

    async fn validate_commit(&self, owner: &str, repo: &str, sha: &str) -> bool {
        let query = r#"
            query($owner: String!, $repo: String!, $oid: GitObjectID!) {
                repository(owner: $owner, name: $repo) {
                    object(oid: $oid) {
                        oid
                    }
                }
            }
        "#;

        let variables = serde_json::json!({
            "owner": owner,
            "repo": repo,
            "oid": sha
        });

        let data: Option<ValidationData> = self.graphql(query, Some(variables)).await;

        data.and_then(|d| d.repository)
            .and_then(|r| r.commit)
            .is_some()
    }
}

// ============================================================================
// Tests
// ============================================================================

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
    async fn test_issues_matching_prefix_empty() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        let recent = client
            .issues_matching_prefix("rust-lang", "rust", "", 5)
            .await;
        assert!(
            !recent.is_empty(),
            "Should return recent issues for empty prefix"
        );
        assert!(recent.len() <= 5, "Should respect limit");
    }

    #[tokio::test]
    async fn test_issues_matching_prefix_numeric() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        let results = client
            .issues_matching_prefix("rust-lang", "rust", "1", 5)
            .await;
        assert!(
            !results.is_empty(),
            "Should return results for numeric prefix"
        );
        // First result should be issue #1 (exact match)
        assert_eq!(results[0].number, 1, "First result should be exact match");
    }

    #[tokio::test]
    async fn test_issues_matching_prefix_text() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        let results = client
            .issues_matching_prefix("rust-lang", "rust", "ICE", 5)
            .await;
        assert!(!results.is_empty(), "Should return results for text prefix");
    }

    #[tokio::test]
    async fn test_users_matching_prefix_empty() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        let users = client
            .users_matching_prefix("rust-lang", "rust", "", 5)
            .await;
        assert!(!users.is_empty(), "Should return mentionable users");
    }

    #[tokio::test]
    async fn test_users_matching_prefix_with_query() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        let users = client
            .users_matching_prefix("rust-lang", "rust", "mat", 10)
            .await;
        assert!(!users.is_empty(), "Should return matching users");
        // Should match users with 'mat' in login or name
    }

    #[tokio::test]
    async fn test_users_have_names() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        let users = client
            .users_matching_prefix("rust-lang", "rust", "", 20)
            .await;

        // At least some users should have display names
        let with_names = users.iter().filter(|u| u.name.is_some()).count();
        assert!(with_names > 0, "Some users should have display names");
    }

    #[tokio::test]
    async fn test_validate_issue_exists() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        let exists = client.validate_issue("rust-lang", "rust", 1).await;
        assert!(exists, "Issue #1 should exist in rust-lang/rust");
    }

    #[tokio::test]
    async fn test_validate_issue_not_found() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        let exists = client.validate_issue("rust-lang", "rust", 999999999).await;
        assert!(!exists, "Non-existent issue should not be found");
    }

    #[tokio::test]
    async fn test_validate_user_exists() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        let exists = client.validate_user("torvalds").await;
        assert!(exists, "torvalds should exist");
    }

    #[tokio::test]
    async fn test_validate_user_not_found() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        let exists = client
            .validate_user("this-user-definitely-does-not-exist-12345")
            .await;
        assert!(!exists, "Non-existent user should not be found");
    }

    #[tokio::test]
    async fn test_validate_commit_exists() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        // First commit in rust-lang/rust (full SHA required for GraphQL)
        let exists = client
            .validate_commit(
                "rust-lang",
                "rust",
                "c01efc669f09508b55eced32d3c88702578a7c3e",
            )
            .await;
        assert!(exists, "First commit should exist in rust-lang/rust");
    }

    #[tokio::test]
    async fn test_validate_commit_not_found() {
        setup_crypto();
        let client = GitHubClient::new(token_from_env());

        let exists = client
            .validate_commit(
                "rust-lang",
                "rust",
                "0000000000000000000000000000000000000000",
            )
            .await;
        assert!(!exists, "Invalid commit should not be found");
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
