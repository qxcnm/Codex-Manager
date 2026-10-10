use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

pub(super) async fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "codexSkills/repositoryAdd" => super::value_or_error(
            crate::codex_skill_repositories::add_async(
                super::str_param(req, "source"),
                super::str_param(req, "refName"),
                super::str_param(req, "codexHome"),
            )
            .await,
        ),
        "codexSkills/repositoryDelete" => super::value_or_error(
            crate::codex_skill_repositories::delete_async(
                super::str_param(req, "repositoryId"),
                super::str_param(req, "codexHome"),
            )
            .await,
        ),
        "codexSkills/repositoryRefresh" => super::value_or_error(
            crate::codex_skill_repositories::refresh_async(
                super::str_param(req, "repositoryId"),
                super::str_param(req, "codexHome"),
            )
            .await,
        ),
        "codexSkills/repositoryInstall" => super::value_or_error(
            crate::codex_skill_repositories::install_async(
                super::str_param(req, "repositoryId"),
                super::str_param(req, "skillId"),
                super::str_param(req, "codexHome"),
            )
            .await,
        ),
        "codexSkills/registrySearch" => super::value_or_error(
            crate::codex_skill_repositories::registry_search_async(
                super::str_param(req, "query"),
                super::i64_param(req, "limit"),
                super::i64_param(req, "offset"),
                super::str_param(req, "codexHome"),
            )
            .await,
        ),
        "codexSkills/registryInstall" => super::value_or_error(
            crate::codex_skill_repositories::registry_install_async(
                super::str_param(req, "source"),
                super::str_param(req, "skillId"),
                super::str_param(req, "codexHome"),
            )
            .await,
        ),
        _ => return None,
    };
    Some(super::response(req, result))
}
