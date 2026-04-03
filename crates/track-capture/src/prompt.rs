use serde_json::json;
use track_projects::project_catalog::ProjectCatalog;

pub struct LlamaCppPrompt {
    pub system_prompt: String,
    pub user_prompt: String,
}

const TASK_PARSER_SYSTEM_PROMPT: &str =
    "You convert short CLI issue notes into structured task data. Return only fields supported by the schema. Write a concise task title and a readable Markdown body that preserves concrete context.";

const TASK_PARSER_DEVELOPER_PROMPT: &str =
    "Choose priority from high, medium, or low. Default priority to medium when the text does not clearly set one. Choose project only from the provided project names and aliases. Return the chosen project name or alias exactly as selected. If the project is ambiguous or missing, output project as null and confidence as low. If you are uncertain about project selection, set confidence to low. Write `title` as one concise actionable line. Write `bodyMarkdown` as optional supporting Markdown that preserves important links, file paths, commands, flags, and other concrete context. Do not repeat the title inside `bodyMarkdown`. Use an empty string when no extra body is needed. Respond with strict JSON that matches the provided schema.";

pub fn build_task_parser_payload(
    raw_text: &str,
    project_catalog: &ProjectCatalog,
) -> serde_json::Value {
    // The model gets a constrained project lookup table so it can infer from
    // real local repositories instead of inventing names that do not exist.
    json!({
        "rawText": raw_text,
        "allowedProjects": project_catalog
            .projects()
            .iter()
            .map(|project| {
                json!({
                    "canonicalName": project.canonical_name,
                    "aliases": project.aliases,
                })
            })
            .collect::<Vec<_>>(),
        "expectedJsonShape": {
            "project": "project-name-or-alias-or-null",
            "priority": "high|medium|low",
            "title": "Concise actionable sentence",
            "bodyMarkdown": "Optional supporting markdown, without repeating the title",
            "confidence": "high|low",
            "reason": "Optional short explanation",
        }
    })
}

pub fn build_task_parser_json_schema(project_catalog: &ProjectCatalog) -> serde_json::Value {
    let mut allowed_projects = project_catalog
        .projects()
        .iter()
        .flat_map(|project| {
            std::iter::once(project.canonical_name.clone()).chain(project.aliases.iter().cloned())
        })
        .collect::<Vec<_>>();
    allowed_projects.sort();
    allowed_projects.dedup();

    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "project",
            "priority",
            "title",
            "bodyMarkdown",
            "confidence",
            "reason"
        ],
        "properties": {
            "project": {
                "type": ["string", "null"],
                "enum": allowed_projects
                    .into_iter()
                    .map(serde_json::Value::String)
                    .chain(std::iter::once(serde_json::Value::Null))
                    .collect::<Vec<_>>(),
            },
            "priority": {
                "type": "string",
                "enum": ["high", "medium", "low"],
            },
            "title": {
                "type": "string",
                "minLength": 1,
            },
            "bodyMarkdown": {
                "type": "string",
            },
            "confidence": {
                "type": "string",
                "enum": ["high", "low"],
            },
            "reason": {
                "type": ["string", "null"],
            },
        },
    })
}

pub fn build_llama_cpp_prompt(raw_text: &str, project_catalog: &ProjectCatalog) -> LlamaCppPrompt {
    // The in-process backend works best when we keep the interaction in a
    // normal chat shape: one stable system instruction plus one user turn that
    // carries the current capture request and project allowlist.
    LlamaCppPrompt {
        system_prompt: TASK_PARSER_SYSTEM_PROMPT.to_owned(),
        user_prompt: [
            TASK_PARSER_DEVELOPER_PROMPT.to_owned(),
            "Return only JSON. Do not use Markdown fences.".to_owned(),
            serde_json::to_string_pretty(&build_task_parser_payload(raw_text, project_catalog))
                .expect("prompt payload serialization should succeed"),
        ]
        .join("\n\n"),
    }
}
