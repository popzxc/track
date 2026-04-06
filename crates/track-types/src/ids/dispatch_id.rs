use crate::time_utils::now_utc;

define_path_id!(
    DispatchId,
    "Dispatch id",
    "database dispatch ids should be valid path components"
);

impl DispatchId {
    pub fn unique() -> Self {
        Self::new(format!("dispatch-{}", now_utc().unix_timestamp_nanos()))
            .expect("generated dispatch ids should be valid path components")
    }
}

#[cfg(test)]
mod tests {
    use crate::errors::ErrorCode;

    use super::DispatchId;

    #[test]
    fn generated_ids_use_the_dispatch_prefix() {
        let dispatch_id = DispatchId::unique();

        assert!(dispatch_id.as_str().starts_with("dispatch-"));
    }

    #[test]
    fn rejects_invalid_path_id_shapes() {
        let error = DispatchId::new("../escape").expect_err("invalid path ids should fail");

        assert_eq!(error.code, ErrorCode::InvalidPathComponent);
    }
}
