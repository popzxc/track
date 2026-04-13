use axum::Json;
use track_types::build_info::BuildInfo;

use crate::build_info::server_build_info;

pub(crate) async fn get_server_version() -> Json<BuildInfo> {
    Json(server_build_info())
}
