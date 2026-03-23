use serde_json::json;

use crate::project_catalog::ProjectCatalog;

pub const DEFAULT_LLAMA_CPP_COMPLETION_BINARY: &str = "llama-completion";

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

pub fn build_llama_cpp_prompt(raw_text: &str, project_catalog: &ProjectCatalog) -> LlamaCppPrompt {
    // `llama-completion` becomes much more reliable with instruct-tuned models
    // when we give it a real system prompt and a single user turn instead of
    // flattening the whole exchange into one raw completion string.
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
