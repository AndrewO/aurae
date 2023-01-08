/* -------------------------------------------------------------------------- *\
 *             Apache 2.0 License Copyright © 2022 The Aurae Authors          *
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

use iter_tools::Itertools;
use libc::c_char;
use std::ffi::CString;
use std::io::{self, ErrorKind};
use std::path::PathBuf;
use std::ptr;
use tracing::info;

#[derive(Debug, Clone, Default)]
pub struct IsolationControls {
    pub isolate_process: bool,
    pub isolate_network: bool,
}

#[derive(Default)]
pub(crate) struct Isolation {
    name: String,
}

impl Isolation {
    pub fn new(name: &str) -> Isolation {
        Isolation { name: name.to_string() }
    }
    pub fn setup(&mut self, iso_ctl: &IsolationControls) -> io::Result<()> {
        // The only setup we will need to do is for isolate_process at this time.
        // We can exit quickly if we are sharing the process controls with the host.
        if !iso_ctl.isolate_process {
            return Ok(());
        }

        // Bind mount root:root with MS_REC and MS_PRIVATE flags
        // We are not sharing the mounts at this point (in other words we are in a new mount namespace)
        let root = PathBuf::from("/");
        nix::mount::mount(
            Some(&root),
            &root,
            None::<&str>, // ignored
            nix::mount::MsFlags::MS_PRIVATE | nix::mount::MsFlags::MS_REC,
            None::<&str>, // ignored
        )
        .map_err(|e| io::Error::from_raw_os_error(e as i32))?;
        info!("Isolation: Bind mounted root dir (/) in cell");
        Ok(())
    }

    pub fn isolate_process(
        &mut self,
        iso_ctl: &IsolationControls,
    ) -> io::Result<()> {
        if !iso_ctl.isolate_process {
            return Ok(());
        }

        //Mount proc in the new pid and mount namespace
        let target = PathBuf::from("/proc");
        nix::mount::mount(
            Some("/proc"),
            &target,
            Some("proc"),
            nix::mount::MsFlags::empty(),
            None::<&str>,
        )
        .map_err(|e| io::Error::from_raw_os_error(e as i32))?;

        // We are in a new UTS namespace so we manage hostname and domainname

        // Set hostname
        // CString::fr
        // let c_hostname = CString::new(&self.name).unwrap();
        // let c_const_hostname: *const c_char =
        //     c_hostname.as_ptr() as *const c_char;
        if unsafe {
            libc::sethostname(
                self.name.as_ptr() as *const c_char,
                self.name.len(),
            )
        } == -1
        {
            return Err(io::Error::last_os_error());
        }

        // Set domainname
        // let c_domain = CString::new(&self.name).unwrap();
        // let c_const_domainname: *const c_char =
        //     c_domain.as_ptr() as *const c_char;
        if unsafe {
            libc::setdomainname(
                self.name.as_ptr() as *const c_char,
                self.name.len(),
            )
        } == -1
        {
            return Err(io::Error::last_os_error());
        }
        info!("Isolate: Process");
        Ok(())
    }

    pub fn isolate_network(
        &mut self,
        iso_ctl: &IsolationControls,
    ) -> io::Result<()> {
        if !iso_ctl.isolate_network {
            return Ok(());
        }
        info!("Isolate: Network");
        Ok(())
    }
}
