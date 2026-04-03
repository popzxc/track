use track_types::errors::{ErrorCode, TrackError};
use track_types::time_utils::parse_iso_8601_seconds;

use crate::scripts::{FetchGithubApiScript, FetchGithubLoginScript, PostPullRequestCommentScript};
use crate::ssh::SshClient;
use crate::types::{
    GithubPullRequestApiResponse, GithubPullRequestMetadata, GithubPullRequestReference,
    GithubPullRequestReviewState, GithubReviewApiResponse, GithubSubmittedReview,
};
use crate::utils::{contextualize_track_error, parse_github_pull_request_reference};

fn github_pull_request_endpoint(reference: &GithubPullRequestReference) -> String {
    format!(
        "repos/{}/{}/pulls/{}",
        reference.owner, reference.repository, reference.number
    )
}

fn github_pull_request_reviews_endpoint(reference: &GithubPullRequestReference) -> String {
    format!(
        "{}/reviews?per_page=100",
        github_pull_request_endpoint(reference)
    )
}

fn github_pull_request_issue_comments_endpoint(reference: &GithubPullRequestReference) -> String {
    format!(
        "repos/{}/{}/issues/{}/comments",
        reference.owner, reference.repository, reference.number
    )
}

/// Asks the remote `gh` CLI which GitHub account it is authenticated as, which
/// validates remote GitHub access and identifies the fork namespace to use.
pub(crate) struct FetchGithubLoginAction<'a> {
    ssh_client: &'a SshClient,
}

impl<'a> FetchGithubLoginAction<'a> {
    pub(crate) fn new(ssh_client: &'a SshClient) -> Self {
        Self { ssh_client }
    }

    pub(crate) fn execute(&self) -> Result<String, TrackError> {
        let script = FetchGithubLoginScript;
        let login = self.ssh_client.run_script(&script.render(), &[])?;

        let login = login.trim().to_owned();
        if login.is_empty() {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                "Remote `gh` authentication did not return a GitHub login.",
            ));
        }

        Ok(login)
    }
}

/// Fetches the durable metadata that defines a pull request as a remote review
/// target, including repository identity, base branch, and current head commit.
pub(crate) struct FetchPullRequestMetadataAction<'a> {
    ssh_client: &'a SshClient,
    pull_request_url: &'a str,
}

impl<'a> FetchPullRequestMetadataAction<'a> {
    pub(crate) fn new(ssh_client: &'a SshClient, pull_request_url: &'a str) -> Self {
        Self {
            ssh_client,
            pull_request_url,
        }
    }

    pub(crate) fn execute(&self) -> Result<GithubPullRequestMetadata, TrackError> {
        let reference = parse_github_pull_request_reference(self.pull_request_url)?;
        let pull_request_endpoint = github_pull_request_endpoint(&reference);
        let script = FetchGithubApiScript;
        let arguments = script.arguments(&pull_request_endpoint);
        let pull_request_json = self
            .ssh_client
            .run_script(&script.render(), &arguments)
            .map_err(|error| {
                contextualize_track_error(
                    error,
                    format!(
                        "Remote `gh api` on {}@{} could not fetch PR details for {} via endpoint `{}`",
                        self.ssh_client.user(),
                        self.ssh_client.host(),
                        self.pull_request_url,
                        pull_request_endpoint
                    ),
                )
            })?;
        let pull_request =
            serde_json::from_str::<GithubPullRequestApiResponse>(&pull_request_json).map_err(
                |error| {
                    TrackError::new(
                        ErrorCode::RemoteDispatchFailed,
                        format!(
                            "GitHub PR details from endpoint `{pull_request_endpoint}` are not valid JSON: {error}"
                        ),
                    )
                },
            )?;

        if pull_request.state != "open" || pull_request.merged_at.is_some() {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "Pull request {} is not open anymore.",
                    self.pull_request_url
                ),
            ));
        }

        Ok(GithubPullRequestMetadata {
            pull_request_url: self.pull_request_url.trim().to_owned(),
            pull_request_number: reference.number,
            pull_request_title: pull_request.title,
            repository_full_name: format!("{}/{}", reference.owner, reference.repository),
            repo_url: format!(
                "https://github.com/{}/{}",
                reference.owner, reference.repository
            ),
            git_url: format!(
                "git@github.com:{}/{}.git",
                reference.owner, reference.repository
            ),
            base_branch: pull_request.base.branch_ref,
            head_oid: pull_request.head.sha,
        })
    }
}

/// Fetches the current PR head plus the latest actionable review from the
/// configured human reviewer so automatic follow-up policy can react to real
/// reviewer activity.
pub(crate) struct FetchPullRequestReviewStateAction<'a> {
    ssh_client: &'a SshClient,
    pull_request_url: &'a str,
    main_user: &'a str,
}

impl<'a> FetchPullRequestReviewStateAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        pull_request_url: &'a str,
        main_user: &'a str,
    ) -> Self {
        Self {
            ssh_client,
            pull_request_url,
            main_user,
        }
    }

    pub(crate) fn execute(&self) -> Result<GithubPullRequestReviewState, TrackError> {
        let reference = parse_github_pull_request_reference(self.pull_request_url)?;
        let pull_request_endpoint = github_pull_request_endpoint(&reference);
        let fetch_api_script = FetchGithubApiScript;
        let pull_request_arguments = fetch_api_script.arguments(&pull_request_endpoint);
        let pull_request_json = self
            .ssh_client
            .run_script(&fetch_api_script.render(), &pull_request_arguments)
            .map_err(|error| {
                contextualize_track_error(
                    error,
                    format!(
                        "Remote `gh api` on {}@{} could not fetch PR details for {} via endpoint `{}`",
                        self.ssh_client.user(),
                        self.ssh_client.host(),
                        self.pull_request_url,
                        pull_request_endpoint
                    ),
                )
            })?;
        let pull_request =
            serde_json::from_str::<GithubPullRequestApiResponse>(&pull_request_json).map_err(
                |error| {
                    TrackError::new(
                        ErrorCode::RemoteDispatchFailed,
                        format!(
                            "GitHub PR details from endpoint `{pull_request_endpoint}` are not valid JSON: {error}"
                        ),
                    )
                },
            )?;

        let reviews_endpoint = github_pull_request_reviews_endpoint(&reference);
        let review_arguments = fetch_api_script.arguments(&reviews_endpoint);
        let reviews_json = self
            .ssh_client
            .run_script(&fetch_api_script.render(), &review_arguments)
            .map_err(|error| {
                contextualize_track_error(
                    error,
                    format!(
                        "Remote `gh api` on {}@{} could not fetch PR reviews for {} via endpoint `{}`",
                        self.ssh_client.user(),
                        self.ssh_client.host(),
                        self.pull_request_url,
                        reviews_endpoint
                    ),
                )
            })?;
        let reviews = serde_json::from_str::<Vec<GithubReviewApiResponse>>(&reviews_json).map_err(
            |error| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!(
                        "GitHub PR reviews from endpoint `{reviews_endpoint}` are not valid JSON: {error}"
                    ),
                )
            },
        )?;

        let latest_eligible_review = reviews
            .into_iter()
            .filter_map(|review| {
                let reviewer = review.user?.login;
                if reviewer != self.main_user {
                    return None;
                }

                if review.state != "COMMENTED" && review.state != "CHANGES_REQUESTED" {
                    return None;
                }

                let submitted_at = review
                    .submitted_at
                    .as_deref()
                    .and_then(|value| parse_iso_8601_seconds(value).ok())?;

                Some(GithubSubmittedReview {
                    state: review.state,
                    submitted_at,
                })
            })
            .max_by_key(|review| review.submitted_at);

        Ok(GithubPullRequestReviewState {
            is_open: pull_request.state == "open" && pull_request.merged_at.is_none(),
            head_oid: pull_request.head.sha,
            latest_eligible_review,
        })
    }
}

/// Posts a comment on a pull request through the remote GitHub CLI so remote
/// automation can coordinate directly in the PR timeline.
pub(crate) struct PostPullRequestCommentAction<'a> {
    ssh_client: &'a SshClient,
    pull_request_url: &'a str,
    comment_body: &'a str,
}

impl<'a> PostPullRequestCommentAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        pull_request_url: &'a str,
        comment_body: &'a str,
    ) -> Self {
        Self {
            ssh_client,
            pull_request_url,
            comment_body,
        }
    }

    pub(crate) fn execute(&self) -> Result<(), TrackError> {
        let reference = parse_github_pull_request_reference(self.pull_request_url)?;
        let issue_comments_endpoint = github_pull_request_issue_comments_endpoint(&reference);
        let script = PostPullRequestCommentScript;
        let arguments = script.arguments(&issue_comments_endpoint, self.comment_body);
        self.ssh_client
            .run_script(&script.render(), &arguments)
            .map_err(|error| {
                contextualize_track_error(
                    error,
                    format!(
                        "Remote `gh api` on {}@{} could not post a PR comment for {} via endpoint `{}`",
                        self.ssh_client.user(),
                        self.ssh_client.host(),
                        self.pull_request_url,
                        issue_comments_endpoint
                    ),
                )
            })?;

        Ok(())
    }
}
