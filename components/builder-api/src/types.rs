// Copyright (c) 2017 Chef Software Inc. and/or applicable contributors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

pub use github_api_client::types::*;

#[derive(Clone, Serialize, Deserialize)]
pub struct JobCreateReq {
    pub project_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProjectCreateReq {
    pub origin: String,
    pub plan_path: String,
    pub github: GitHubProject,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProjectUpdateReq {
    pub plan_path: String,
    pub github: GitHubProject,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GitHubProject {
    pub organization: String,
    pub repo: String,
    pub installation_id: Option<u32>,
}
