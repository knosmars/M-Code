//! Aggregated Tauri command handler, grouped by domain.
//!
//! Tauri's `generate_handler!` needs the literal ident list at one macro site
//! (compile-time routing), so per-module runtime aggregation isn't possible.
//! This macro keeps the list out of `lib.rs` and grouped by domain — new
//! commands are added here under the matching section.

#[macro_export]
macro_rules! app_handler {
    () => {
        tauri::generate_handler![
            // ── chat / models / keychain ──
            commands::stream_chat,
            commands::list_models,
            commands::get_api_key,
            commands::set_api_key,
            commands::delete_api_key,
            commands::tool_open_url,

            // ── file ops ──
            tools::tool_read_file,
            tools::tool_write_file,
            tools::tool_edit_file,
            tools::tool_list_dir,
            tools::tool_run_command,
            tools::tool_grep,
            tools::web::tool_web_scrape,
            tools::web::tool_web_crawl,
            tools::web::tool_web_map,
            tools::tool_glob,
            tools::diff_preview::tool_edit_file_preview,
            tools::multi_edit::tool_multi_edit_preview,
            tools::multi_edit::tool_multi_edit_apply,

            // ── folder/file picker ──
            commands::pick_folder::tool_pick_folder,
            commands::pick_folder::tool_pick_file,
            commands::pick_folder::tool_read_attachment,
            commands::pick_folder::tool_set_workspace,

            // ── git / github ──
            tools::git::tool_git_status,
            tools::git::tool_git_diff,
            tools::git::tool_git_diff_staged,
            tools::git::tool_git_log,
            tools::git::tool_git_commit,
            tools::git::tool_git_branch,
            tools::git::tool_git_push,
            tools::git::tool_git_remote_info,
            tools::git::tool_gh_auth_status,
            tools::git::tool_gh_auth_login,
            tools::git::tool_gh_pr_create,
            tools::github_oauth::tool_github_oauth_login,
            tools::github_oauth::tool_github_oauth_logout,
            tools::github_oauth::tool_github_oauth_status,
            tools::github_oauth::tool_github_oauth_store_token,

            // ── terminal ──
            tools::terminal::tool_terminal_start,
            tools::terminal::tool_terminal_send,
            tools::terminal::tool_terminal_stop,
            tools::terminal::tool_terminal_list,

            // ── lsp ──
            tools::lsp::tool_lsp_hover,
            tools::lsp::tool_lsp_go_to_definition,
            tools::lsp::tool_lsp_find_references,

            // ── memory / global memory ──
            tools::memory::tool_memory_read,
            tools::memory::tool_memory_write,
            tools::memory::tool_memory_search,
            tools::global_memory::tool_global_register_project,
            tools::global_memory::tool_global_add_note,
            tools::global_memory::tool_global_search,
            tools::global_memory::tool_global_stats,
            tools::global_memory::tool_global_add_pattern,
            tools::global_memory::tool_global_patterns,

            // ── analysis / index / search ──
            tools::index::tool_index_codebase,
            tools::search::tool_search_codebase,
            tools::semantic::tool_semantic_index,
            tools::semantic::tool_semantic_search,
            tools::semantic::tool_semantic_status,
            tools::semantic::tool_semantic_config_get,
            tools::semantic::tool_semantic_config_set,
            tools::impact_analysis::tool_impact_analysis,
            tools::code_graph::tool_code_graph,
            tools::test_runner::tool_test_runner,
            tools::doc_index::tool_doc_index,
            tools::doc_index::tool_doc_search,
            tools::error_diagnosis::tool_error_diagnosis,
            tools::perf_analyzer::tool_perf_analyze,
            tools::image_gen::tool_generate_image,

            // ── agents / hooks / triggers / skills / rules ──
            tools::agents::tool_agents_list,
            tools::agents_rules::tool_agents_rules_read,
            tools::hooks::tool_hooks_run,
            tools::triggers::tool_triggers_list,
            tools::triggers::tool_triggers_watch,
            tools::triggers::tool_triggers_start_auto,
            tools::skills::tool_skills_list,
            tools::skills::tool_skills_load,

            // ── mcp ──
            tools::mcp::tool_mcp_list_tools,
            tools::mcp::tool_mcp_call,
            tools::mcp::tool_mcp_status,
            tools::mcp::mcp_config_list,
            tools::mcp::mcp_config_add,
            tools::mcp::mcp_config_remove,
            tools::mcp::mcp_config_set_disabled,

            // ── checkpoint ──
            tools::checkpoint::tool_checkpoint_begin,
            tools::checkpoint::tool_checkpoint_end,
            tools::checkpoint::tool_checkpoint_restore,
            tools::checkpoint::tool_checkpoint_list,

            // ── ssh ──
            tools::ssh::tool_ssh_exec,

            // ── file sync ──
            tools::file_sync::tool_file_sync_register,
            tools::file_sync::tool_file_sync_unregister,
            tools::file_sync::tool_file_sync_publish,
            tools::file_sync::tool_file_sync_check,
            tools::file_sync::tool_file_sync_clear,

            // ── review store ──
            tools::review_store::tool_review_add,
            tools::review_store::tool_review_list,
            tools::review_store::tool_review_resolve,
            tools::review_store::tool_review_delete,
            tools::review_store::tool_review_stats,

            // ── sessions ──
            sessions::db_save_session,
            sessions::db_load_sessions,
            sessions::db_delete_session,
        ]
    };
}
