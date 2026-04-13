use serde::Serialize;

/// Keeps the rest of the crate on a tiny, dependency-shaped interface instead
/// of letting `tera` leak through every prompt and script module.
pub(crate) fn render_template<T>(template_source: &str, context: &T) -> String
where
    T: Serialize,
{
    let context =
        tera::Context::from_serialize(context).expect("template context should serialize");
    tera::Tera::one_off(template_source, &context, false)
        .expect("template rendering should succeed")
}
