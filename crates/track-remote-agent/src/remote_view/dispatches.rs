use track_dal::database::DatabaseContext;
use track_types::errors::TrackError;
use track_types::ids::ProjectId;
use track_types::types::{ReviewRecord, ReviewRunRecord, TaskDispatchRecord};

// =============================================================================
// Database-Backed Read Composition
// =============================================================================
//
// The remote workspace view still relies on SQLite for canonical task and
// review metadata. These helpers gather the per-project slices that the facade
// needs without introducing another repository layer.
pub(super) async fn list_task_dispatches_for_project(
    database: &DatabaseContext,
    project_id: &ProjectId,
) -> Result<Vec<TaskDispatchRecord>, TrackError> {
    let tasks = database
        .task_repository()
        .list_tasks(true, Some(project_id))
        .await?;

    let mut dispatches = Vec::new();
    for task in tasks {
        dispatches.extend(
            database
                .dispatch_repository()
                .dispatches_for_task(&task.id)
                .await?,
        );
    }

    dispatches.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(dispatches)
}

pub(super) async fn list_reviews_for_project(
    database: &DatabaseContext,
    project_id: &ProjectId,
) -> Result<Vec<ReviewRecord>, TrackError> {
    let mut reviews = database
        .review_repository()
        .list_reviews()
        .await?
        .into_iter()
        .filter(|review| review.project.as_ref() == Some(project_id))
        .collect::<Vec<_>>();
    reviews.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));

    Ok(reviews)
}

pub(super) async fn list_review_runs_for_project(
    database: &DatabaseContext,
    project_id: &ProjectId,
) -> Result<Vec<ReviewRunRecord>, TrackError> {
    let reviews = list_reviews_for_project(database, project_id).await?;
    let mut review_runs = Vec::new();

    for review in reviews {
        review_runs.extend(
            database
                .review_dispatch_repository()
                .dispatches_for_review(&review.id)
                .await?,
        );
    }

    review_runs.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(review_runs)
}
