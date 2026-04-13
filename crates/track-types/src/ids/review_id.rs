define_path_id!(
    ReviewId,
    "Review id",
    "database review ids should be valid path components"
);

#[cfg(test)]
mod tests {
    use super::ReviewId;

    #[test]
    fn accepts_valid_review_ids() {
        let review_id = ReviewId::new("review-1").expect("review ids should validate");

        assert_eq!(review_id.as_str(), "review-1");
    }
}
