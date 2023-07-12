#[allow(dead_code)]
mod execution;
mod global_hash;
mod scm;
mod task;

use global_hash::GlobalHashSummary;
use serde::{Deserialize, Serialize};
use svix_ksuid::Ksuid;
use thiserror::Error;
use turbopath::{AbsoluteSystemPathBuf, AnchoredSystemPathBuf};
use turborepo_api_client::APIClient;
use turborepo_env::EnvironmentVariableMap;

use crate::{
    cli::EnvMode,
    opts::RunOpts,
    run::summary::{execution::ExecutionSummary, task::TaskSummary},
};

#[derive(Debug, Error)]
enum Error {}

// NOTE: When changing this, please ensure that the server side is updated to
// handle the new version on vercel.com this is required to ensure safe handling
// of env vars (unknown run summary versions will be ignored on the server)
const RUN_SUMMARY_SCHEMA_VERSION: &str = "1";

enum RunType {
    Real,
    DryText,
    DryJson,
}

// Wrapper around the serializable RunSummary, with some extra information
// about the Run and references to other things that we need.
struct Meta {
    run_summary: RunSummary,
    repo_root: AbsoluteSystemPathBuf,
    repo_path: AbsoluteSystemPathBuf,
    single_package: bool,
    should_save: bool,
    run_type: RunType,
    synthesized_command: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunSummary {
    id: Ksuid,
    version: String,
    turbo_version: String,
    monorepo: bool,
    global_hash_summary: GlobalHashSummary,
    packages: Vec<String>,
    env_mode: EnvMode,
    framework_inference: bool,
    execution_summary: Option<ExecutionSummary>,
    tasks: Vec<TaskSummary>,
    user: String,
    scm: ScmState,
}

impl Meta {
    pub fn new_run_summary(
        start_at: chrono::NaiveDateTime,
        repo_root: AbsoluteSystemPathBuf,
        repo_path: AnchoredSystemPathBuf,
        turbo_version: &'static str,
        api_client: APIClient,
        run_opts: RunOpts,
        packages: &[String],
        global_env_mode: EnvMode,
        env_at_execution_start: EnvironmentVariableMap,
        global_hash_summary: GlobalHashSummary,
        synthesized_command: String,
    ) -> Meta {
        let single_package = run_opts.single_package;
        let profile = run_opts.profile;
        let should_save = run_opts.summarize;
        let space_id = &run_opts.experimental_space_id;

        let run_type = if run_opts.dry_run {
            if run_opts.dry_run_json {
                RunType::DryJson
            } else {
                RunType::DryText
            }
        } else {
            RunType::Real
        };
    }

    fn normalize(&mut self) {
        // Remove execution summary for dry runs
        if matches!(self.run_type, RunType::DryJson) {
            self.run_summary.execution_summary = None;
        }

        // For single packages, we don't need the packages
        // and each task summary needs some cleaning
        if self.single_package {
            self.run_summary.packages = vec![];

            for task_summary in &mut self.run_summary.tasks {
                task_summary.clean_for_single_package();
            }
        }

        self.run_summary.tasks.sort_by(|a, b| a.cmp(&b.name));
    }

    fn save(&self) -> Result<(), Error> {
        let json = serde_json::to_string_pretty(&self.run_summary)?;
    }
}
