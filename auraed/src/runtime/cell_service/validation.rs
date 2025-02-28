use super::cells::{
    cgroups::{
        self,
        cpuset::{Cpus, Mems},
        CgroupSpec, Limit, Weight,
    },
    CellNamePath, IsolationControls,
};
use super::executables::ExecutableName;
use aurae_proto::runtime::{
    Cell, CellServiceAllocateRequest, CellServiceFreeRequest,
    CellServiceStartRequest, CellServiceStopRequest, CpuController,
    CpusetController, Executable,
};
use std::ffi::OsString;
use tokio::process::Command;
use validation::{ValidatedField, ValidatedType, ValidationError};
use validation_macros::ValidatedType;

// TODO: Following the discord discussion of wanting to keep the logic on CellService,
//  versus on the validated request structs, we may not want to create a file per endpoint,
//  so I'm (future-highway) grouping it all here at least temporarily.
// TODO: ...and I (@krisnova) read the above statement.

#[derive(Debug, ValidatedType)]
pub struct ValidatedCellServiceAllocateRequest {
    #[field_type(Option<Cell>)]
    pub cell: ValidatedCell,
}

impl CellServiceAllocateRequestTypeValidator
    for CellServiceAllocateRequestValidator
{
    fn validate_cell(
        cell: Option<Cell>,
        field_name: &str,
        parent_name: Option<&str>,
    ) -> Result<ValidatedCell, ValidationError> {
        let cell = validation::required(cell, field_name, parent_name)?;

        ValidatedCell::validate(
            cell,
            Some(&validation::field_name(field_name, parent_name)),
        )
    }
}

#[derive(ValidatedType, Debug, Clone)]
pub struct ValidatedCell {
    #[field_type(String)]
    pub name: CellNamePath,

    #[field_type(Option<CpuController>)]
    pub cpu: Option<ValidatedCpuController>,

    #[field_type(Option<CpusetController>)]
    pub cpuset: Option<ValidatedCpusetController>,

    #[validate(none)]
    pub isolate_process: bool,

    #[validate(none)]
    pub isolate_network: bool,
}

impl CellTypeValidator for CellValidator {
    fn validate_name(
        name: String,
        field_name: &str,
        parent_name: Option<&str>,
    ) -> Result<CellNamePath, ValidationError> {
        let name = CellNamePath::validate(Some(name), field_name, parent_name)?;

        if matches!(name, CellNamePath::Empty) {
            return Err(ValidationError::Required {
                field: validation::field_name(field_name, parent_name),
            });
        }

        Ok(name)
    }

    fn validate_cpu(
        cpu: Option<CpuController>,
        field_name: &str,
        parent_name: Option<&str>,
    ) -> Result<Option<ValidatedCpuController>, ValidationError> {
        let Some(cpu) = cpu else {
            return Ok(None);
        };

        Ok(Some(ValidatedCpuController::validate(
            cpu,
            Some(&*validation::field_name(field_name, parent_name)),
        )?))
    }

    fn validate_cpuset(
        cpuset: Option<CpusetController>,
        field_name: &str,
        parent_name: Option<&str>,
    ) -> Result<Option<ValidatedCpusetController>, ValidationError> {
        let Some(cpuset) = cpuset else {
            return Ok(None);
        };

        Ok(Some(ValidatedCpusetController::validate(
            cpuset,
            Some(&*validation::field_name(field_name, parent_name)),
        )?))
    }
}

impl From<ValidatedCell> for super::cells::CellSpec {
    fn from(x: ValidatedCell) -> Self {
        let ValidatedCell {
            name: _,
            cpu,
            cpuset,
            isolate_process,
            isolate_network,
        } = x;

        Self {
            cgroup_spec: CgroupSpec {
                cpu: cpu.map(|x| x.into()),
                cpuset: cpuset.map(|x| x.into()),
            },
            iso_ctl: IsolationControls { isolate_process, isolate_network },
        }
    }
}

#[derive(ValidatedType, Debug, Clone)]
pub struct ValidatedCpuController {
    #[field_type(Option<u64>)]
    #[validate(opt)]
    pub weight: Option<Weight>,

    #[field_type(Option<i64>)]
    #[validate(opt)]
    pub max: Option<Limit>,
}

impl CpuControllerTypeValidator for CpuControllerValidator {}

impl From<ValidatedCpuController> for cgroups::cpu::CpuController {
    fn from(value: ValidatedCpuController) -> Self {
        let ValidatedCpuController { weight, max } = value;
        Self { weight, max }
    }
}

#[derive(ValidatedType, Debug, Clone)]
pub struct ValidatedCpusetController {
    #[field_type(Option<String>)]
    #[validate(opt)]
    pub cpus: Option<Cpus>,

    #[field_type(Option<String>)]
    #[validate(opt)]
    pub mems: Option<Mems>,
}

impl CpusetControllerTypeValidator for CpusetControllerValidator {}

impl From<ValidatedCpusetController> for cgroups::cpuset::CpusetController {
    fn from(value: ValidatedCpusetController) -> Self {
        let ValidatedCpusetController { cpus, mems } = value;
        Self { cpus, mems }
    }
}

#[derive(Debug, ValidatedType)]
pub struct ValidatedCellServiceFreeRequest {
    #[field_type(String)]
    pub cell_name: CellNamePath,
}

impl CellServiceFreeRequestTypeValidator for CellServiceFreeRequestValidator {
    fn validate_cell_name(
        cell_name: String,
        field_name: &str,
        parent_name: Option<&str>,
    ) -> Result<CellNamePath, ValidationError> {
        let cell_name =
            CellNamePath::validate(Some(cell_name), field_name, parent_name)?;

        if matches!(cell_name, CellNamePath::Empty) {
            return Err(ValidationError::Required {
                field: validation::field_name(field_name, parent_name),
            });
        }

        Ok(cell_name)
    }
}

#[derive(Debug, ValidatedType)]
pub struct ValidatedCellServiceStartRequest {
    #[field_type(String)]
    #[validate]
    pub cell_name: CellNamePath,
    #[field_type(Option<Executable>)]
    pub executable: ValidatedExecutable,
}

impl CellServiceStartRequestTypeValidator for CellServiceStartRequestValidator {
    fn validate_executable(
        executable: Option<Executable>,
        field_name: &str,
        parent_name: Option<&str>,
    ) -> Result<ValidatedExecutable, ValidationError> {
        let executable =
            validation::required(executable, field_name, parent_name)?;
        ValidatedExecutable::validate(
            executable,
            Some(&*validation::field_name(field_name, parent_name)),
        )
    }
}

#[derive(Debug, ValidatedType)]
pub struct ValidatedCellServiceStopRequest {
    #[field_type(String)]
    #[validate]
    pub cell_name: CellNamePath,
    #[field_type(String)]
    #[validate]
    pub executable_name: ExecutableName,
}

impl CellServiceStopRequestTypeValidator for CellServiceStopRequestValidator {}

#[derive(ValidatedType, Debug)]
pub struct ValidatedExecutable {
    #[field_type(String)]
    #[validate(create)]
    pub name: ExecutableName,

    #[field_type(String)]
    pub command: OsString,

    // TODO: `#[validate(none)] is used to skip validation. Actually validate when restrictions are known.
    #[validate(none)]
    pub description: String,
}

impl ExecutableTypeValidator for ExecutableValidator {
    fn validate_command(
        command: String,
        field_name: &str,
        parent_name: Option<&str>,
    ) -> Result<OsString, ValidationError> {
        let command = validation::required_not_empty(
            Some(command),
            field_name,
            parent_name,
        )?;

        Ok(OsString::from(command))
    }
}

impl From<ValidatedExecutable> for super::executables::ExecutableSpec {
    fn from(x: ValidatedExecutable) -> Self {
        let ValidatedExecutable { name, command, description } = x;

        let mut c = Command::new("sh");
        let _ = c.args([OsString::from("-c"), command]);

        // We are checking that command has an arg to assure ourselves that `command.arg`
        // mutates command, and is not making a clone to return
        assert_eq!(c.as_std().get_args().len(), 2);

        Self { name, command: c, description }
    }
}
