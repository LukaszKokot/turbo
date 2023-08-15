use turbopath::AbsoluteSystemPath;
use turborepo_ci::Vendor;
use turborepo_env::EnvironmentVariableMap;
use turborepo_scm::{Git, SCM};

enum SCMType {
    Git,
}

pub(crate) struct SCMState {
    ty: SCMType,
    sha: Option<String>,
    branch: Option<String>,
}

impl SCMState {
    pub fn get(env_vars: &EnvironmentVariableMap, dir: &AbsoluteSystemPath) -> Self {
        let mut state = SCMState {
            ty: ScmType::Git,
            sha: None,
            branch: None,
        };

        if turborepo_ci::is_ci() {
            if let Some(vendor) = Vendor::get_info() {
                if let Some(sha_env_var) = vendor.sha_env_var {
                    state.sha = env_vars.get(sha_env_var).cloned()
                }

                if let Some(branch_env_var) = vendor.branch_env_var {
                    state.branch = env_vars.get(branch_env_var).cloned()
                }
            }
        }

        // Fall back to using `git`
        if state.branch.is_none() && state.sha.is_none() {
            let scm = SCM::new(dir);

            if state.branch.is_none() {
                state.branch = scm.get_current_branch(dir).ok();
            }
            if state.sha.is_none() {
                state.sha = scm.get_current_sha(dir).ok();
            }
        }

        state
    }
}
