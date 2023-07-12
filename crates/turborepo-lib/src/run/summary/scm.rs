use turbopath::AbsoluteSystemPath;
use turborepo_env::EnvironmentVariableMap;

pub(crate) struct ScmState {
    ty: String,
    sha: String,
    branch: String,
}

impl ScmState {
    pub fn get(env_vars: &EnvironmentVariableMap, dir: &AbsoluteSystemPath) -> Self {}
}
