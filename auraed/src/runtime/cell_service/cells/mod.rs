/* -------------------------------------------------------------------------- *\
 *        Apache 2.0 License Copyright © 2022-2023 The Aurae Authors          *
 *                                                                            *
 *                +--------------------------------------------+              *
 *                |   █████╗ ██╗   ██╗██████╗  █████╗ ███████╗ |              *
 *                |  ██╔══██╗██║   ██║██╔══██╗██╔══██╗██╔════╝ |              *
 *                |  ███████║██║   ██║██████╔╝███████║█████╗   |              *
 *                |  ██╔══██║██║   ██║██╔══██╗██╔══██║██╔══╝   |              *
 *                |  ██║  ██║╚██████╔╝██║  ██║██║  ██║███████╗ |              *
 *                |  ╚═╝  ╚═╝ ╚═════╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚══════╝ |              *
 *                +--------------------------------------------+              *
 *                                                                            *
 *                         Distributed Systems Runtime                        *
 *                                                                            *
 * -------------------------------------------------------------------------- *
 *                                                                            *
 *   Licensed under the Apache License, Version 2.0 (the "License");          *
 *   you may not use this file except in compliance with the License.         *
 *   You may obtain a copy of the License at                                  *
 *                                                                            *
 *       http://www.apache.org/licenses/LICENSE-2.0                           *
 *                                                                            *
 *   Unless required by applicable law or agreed to in writing, software      *
 *   distributed under the License is distributed on an "AS IS" BASIS,        *
 *   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. *
 *   See the License for the specific language governing permissions and      *
 *   limitations under the License.                                           *
 *                                                                            *
\* -------------------------------------------------------------------------- */

use cell::Cell;
pub use cell_name::CellName;
pub use cell_name_path::CellNamePath;
pub use cells::Cells;
use cgroups::CgroupSpec;
pub use error::{CellsError, Result};
pub use nested_auraed::IsolationControls;

mod cell;
mod cell_name;
pub mod cell_name_path;
#[allow(clippy::module_inception)]
mod cells;
pub mod cgroups;
mod error;
mod nested_auraed;

#[derive(Debug, Clone)]
pub struct CellSpec {
    pub cgroup_spec: CgroupSpec,
    pub iso_ctl: IsolationControls,
}

impl CellSpec {
    #[cfg(test)]
    pub(crate) fn new_for_tests() -> Self {
        Self {
            cgroup_spec: CgroupSpec { cpu: None, cpuset: None },
            iso_ctl: IsolationControls {
                isolate_network: false,
                isolate_process: false,
            },
        }
    }
}
