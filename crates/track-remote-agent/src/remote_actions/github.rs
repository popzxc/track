use track_types::errors::{ErrorCode, TrackError};
use track_types::time_utils::parse_iso_8601_seconds;
use track_types::urls::Url;

use crate::helper::{
    EmptyRequest, EmptyResponse, GithubApiRequest, GithubApiResponse, GithubLoginResponse,
    PostPullRequestCommentRequest,
};
use crate::ssh::SshClient;
use crate::types::{
    GithubPullRequestApiResponse, GithubPullRequestMetadata, GithubPullRequestReference,
    GithubPullRequestReviewState, GithubReviewApiResponse, GithubSubmittedReview,
};
use crate::utils::contextualize_track_error;

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
        let login = self
            .ssh_client
            .run_helper_json::<_, GithubLoginResponse>("github-login", &EmptyRequest {})?
            .login;

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
    pull_request_url: &'a Url,
}

impl<'a> FetchPullRequestMetadataAction<'a> {
    pub(crate) fn new(ssh_client: &'a SshClient, pull_request_url: &'a Url) -> Self {
        Self {
            ssh_client,
            pull_request_url,
        }
    }

    pub(crate) fn execute(&self) -> Result<GithubPullRequestMetadata, TrackError> {
        let reference = GithubPullRequestReference::parse(self.pull_request_url)?;
        let pull_request_endpoint = reference.pull_request_endpoint();
        let pull_request_json = self
            .ssh_client
            .run_helper_json::<_, GithubApiResponse>(
                "fetch-gh-api",
                &GithubApiRequest {
                    endpoint: &pull_request_endpoint,
                },
            )
            .map(|response| response.output)
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

        GithubPullRequestMetadata::from_api_response(
            &reference,
            self.pull_request_url,
            pull_request,
        )
    }
}

/// Fetches the current PR head plus the latest actionable review from the
/// configured human reviewer so automatic follow-up policy can react to real
/// reviewer activity.
pub(crate) struct FetchPullRequestReviewStateAction<'a> {
    ssh_client: &'a SshClient,
    pull_request_url: &'a Url,
    main_user: &'a str,
}

impl<'a> FetchPullRequestReviewStateAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        pull_request_url: &'a Url,
        main_user: &'a str,
    ) -> Self {
        Self {
            ssh_client,
            pull_request_url,
            main_user,
        }
    }

    pub(crate) fn execute(&self) -> Result<GithubPullRequestReviewState, TrackError> {
        let reference = GithubPullRequestReference::parse(self.pull_request_url)?;
        let pull_request_endpoint = reference.pull_request_endpoint();
        let pull_request_json = self
            .ssh_client
            .run_helper_json::<_, GithubApiResponse>(
                "fetch-gh-api",
                &GithubApiRequest {
                    endpoint: &pull_request_endpoint,
                },
            )
            .map(|response| response.output)
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

        let reviews_endpoint = reference.reviews_endpoint();
        let reviews_json = self
            .ssh_client
            .run_helper_json::<_, GithubApiResponse>(
                "fetch-gh-api",
                &GithubApiRequest {
                    endpoint: &reviews_endpoint,
                },
            )
            .map(|response| response.output)
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

                Some(GithubSubmittedReview::new(review.state, submitted_at))
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
    pull_request_url: &'a Url,
    comment_body: &'a str,
}

impl<'a> PostPullRequestCommentAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        pull_request_url: &'a Url,
        comment_body: &'a str,
    ) -> Self {
        Self {
            ssh_client,
            pull_request_url,
            comment_body,
        }
    }

    pub(crate) fn execute(&self) -> Result<(), TrackError> {
        let reference = GithubPullRequestReference::parse(self.pull_request_url)?;
        let issue_comments_endpoint = reference.issue_comments_endpoint();
        self.ssh_client
            .run_helper_json::<_, EmptyResponse>(
                "post-pr-comment",
                &PostPullRequestCommentRequest {
                    endpoint: &issue_comments_endpoint,
                    body: self.comment_body,
                },
            )
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
