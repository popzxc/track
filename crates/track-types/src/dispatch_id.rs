use crate::ids::DispatchId;
use crate::time_utils::now_utc;

impl DispatchId {
    pub fn unique() -> Self {
        Self::new(format!("dispatch-{}", now_utc().unix_timestamp_nanos()))
            .expect("generated dispatch ids should be valid path components")
    }
}

#[cfg(test)]
mod tests {
    use crate::ids::DispatchId;

    #[test]
    fn generated_ids_use_the_dispatch_prefix() {
        let dispatch_id = DispatchId::unique();

        assert!(dispatch_id.as_str().starts_with("dispatch-"));
    }
}
