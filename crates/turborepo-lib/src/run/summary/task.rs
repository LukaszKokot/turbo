use std::collections::HashMap;

use turbopath::{AnchoredSystemPathBuf, RelativeUnixPathBuf};

use crate::{
    cli::EnvMode,
    run::{summary::execution::TaskExecutionSummary, task_id::strip_package_name},
    task_graph::TaskDefinition,
};

struct TaskCacheSummary {
    local: bool,
    remote: bool,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    time_saved: u32,
}

pub(crate) struct TaskSummary {
    task_id: String,
    task: String,
    package: Option<String>,
    hash: String,
    expanded_inputs: HashMap<RelativeUnixPathBuf, String>,
    external_deps_hash: String,
    cache_summary: TaskCacheSummary,
    comamnd: String,
    command_arguments: Vec<String>,
    outputs: Vec<String>,
    excluded_outputs: Vec<String>,
    log_file_relative_path: String,
    dir: Option<String>,
    dependencies: Vec<String>,
    dependents: Vec<String>,
    resolved_task_definition: TaskDefinition,
    expanded_outputs: Vec<AnchoredSystemPathBuf>,
    framework: String,
    env_mode: EnvMode,
    env_vars: TaskEnvVarSummary,
    dot_env: Vec<RelativeUnixPathBuf>,
    execution: TaskExecutionSummary,
}

struct TaskEnvConfiguration {
    env: Vec<String>,
    pass_through_Env: Vec<String>,
}

struct TaskEnvVarSummary {
    specified: TaskEnvConfiguration,

    configured: Vec<String>,
    inferred: Vec<String>,
    pass_through: Vec<String>,
}

impl TaskSummary {
    pub fn clean_for_single_package(&mut self) {
        let mut dependencies = Vec::with_capacity(self.dependencies.len());

        for dependency in &self.dependencies {
            dependencies.push(strip_package_name(dependency));
        }

        let mut dependents = Vec::with_capacity(self.dependent.len());

        for dependent in &self.dependents {
            dependents.push(strip_package_name(dependent));
        }

        let task = strip_package_name(&self.task_id);

        self.task_id = task.clone();
        self.task = task;
        self.dependencies = dependencies;
        self.dependents = dependents;
        self.dir = None;
        self.package = None;
    }
}
