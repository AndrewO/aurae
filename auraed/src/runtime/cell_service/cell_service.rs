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

use super::{
    cells::{cell_name_path, CellName, CellNamePath, Cells},
    error::CellsServiceError,
    executables::Executables,
    validation::{
        ValidatedCellServiceAllocateRequest, ValidatedCellServiceFreeRequest,
        ValidatedCellServiceStartRequest, ValidatedCellServiceStopRequest,
    },
    Result,
};
use ::validation::ValidatedType;
use aurae_client::{
    runtime::cell_service::CellServiceClient, AuraeClient, AuraeClientError,
};
use aurae_proto::runtime::{
    cell_service_server, CellServiceAllocateRequest,
    CellServiceAllocateResponse, CellServiceFreeRequest,
    CellServiceFreeResponse, CellServiceStartRequest, CellServiceStartResponse,
    CellServiceStopRequest, CellServiceStopResponse,
};
use backoff::backoff::Backoff;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tonic::{Code, Request, Response, Status};
use tracing::{info, trace};

macro_rules! do_in_cell {
    ($self:ident, $cell_name:ident, $function:ident, $request:ident) => {{
        let mut cells = $self.cells.lock().await;

        let client_config = cells
            .get(&$cell_name, |cell| cell.client_config())
            .map_err(CellsServiceError::CellsError)?;

        let mut retry_strategy = backoff::ExponentialBackoffBuilder::new()
            .with_initial_interval(Duration::from_millis(50)) // 1st retry in 50ms
            .with_multiplier(10.0) // 10x the delay after 1st retry (500ms)
            .with_randomization_factor(0.5) // with a randomness of +/-50% (250-750ms)
            .with_max_interval(Duration::from_secs(3)) // but never delay more than 3s
            .with_max_elapsed_time(Some(Duration::from_secs(20))) // or 20s total
            .build();

        let client = loop {
            match AuraeClient::new(client_config.clone()).await {
                Ok(client) => break Ok(client),
                e @ Err(AuraeClientError::ConnectionError(_)) => {
                    trace!("aurae client failed to connect: {e:?}");
                    if let Some(delay) = retry_strategy.next_backoff() {
                        trace!("retrying in {delay:?}");
                        tokio::time::sleep(delay).await
                    } else {
                        break e
                    }
                }
                e => break e
            }
        }.map_err(CellsServiceError::from)?;

        backoff::future::retry(
            retry_strategy,
            || async {
                match client.$function($request.clone()).await {
                    Ok(res) => Ok(res),
                    Err(e) if e.code() == Code::Unknown && e.message() == "transport error" => {
                        Err(e)?;
                        unreachable!();
                    }
                    Err(e) => Err(backoff::Error::Permanent(e))
                }
            },
        )
        .await
    }};
}

#[derive(Debug, Clone)]
pub struct CellService {
    cells: Arc<Mutex<Cells>>,
    executables: Arc<Mutex<Executables>>,
}

impl CellService {
    pub fn new() -> Self {
        CellService {
            cells: Default::default(),
            executables: Default::default(),
        }
    }

    #[tracing::instrument(skip(self))]
    async fn allocate(
        &self,
        request: ValidatedCellServiceAllocateRequest,
    ) -> Result<CellServiceAllocateResponse> {
        // Initialize the cell
        let ValidatedCellServiceAllocateRequest { cell } = request;
        let (cell_name, empty) =
            cell.name.clone().into_child().expect("not empty");

        // There should have been a single cell name in the path.
        // Otherwise, we should have called allocate_in_cell
        assert!(matches!(empty, CellNamePath::Empty));

        let cell_spec = cell.into();

        let mut cells = self.cells.lock().await;
        let cell = cells.allocate(cell_name, cell_spec)?;

        Ok(CellServiceAllocateResponse {
            cell_name: cell.name().clone().into_inner(),
            cgroup_v2: cell.v2().expect("allocated cell returns `Some`"),
        })
    }

    #[tracing::instrument(skip(self))]
    async fn allocate_in_cell(
        &self,
        cell_name: &CellName,
        request: CellServiceAllocateRequest,
    ) -> std::result::Result<Response<CellServiceAllocateResponse>, Status>
    {
        do_in_cell!(self, cell_name, allocate, request)
    }

    #[tracing::instrument(skip(self))]
    async fn free(
        &self,
        request: ValidatedCellServiceFreeRequest,
    ) -> Result<CellServiceFreeResponse> {
        let ValidatedCellServiceFreeRequest { cell_name } = request;

        let (cell_name, empty) = cell_name.into_child().expect("not empty");

        // There should have been a single cell name in the path.
        // Otherwise, we should have called free_in_cell
        assert!(matches!(empty, CellNamePath::Empty));

        info!("CellService: free() cell_name={:?}", cell_name);
        let mut cells = self.cells.lock().await;
        cells.free(&cell_name)?;

        Ok(CellServiceFreeResponse::default())
    }

    #[tracing::instrument(skip(self))]
    async fn free_in_cell(
        &self,
        cell_name: &CellName,
        request: CellServiceFreeRequest,
    ) -> std::result::Result<Response<CellServiceFreeResponse>, Status> {
        do_in_cell!(self, cell_name, free, request)
    }

    #[tracing::instrument(skip(self))]
    pub(crate) async fn free_all(&self) -> Result<()> {
        let mut cells = self.cells.lock().await;

        // First try to gracefully free all cells.
        cells.broadcast_free();
        // The cells that remain failed to shut down for some reason.
        cells.broadcast_kill();

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    async fn start(
        &self,
        request: ValidatedCellServiceStartRequest,
    ) -> std::result::Result<Response<CellServiceStartResponse>, Status> {
        let ValidatedCellServiceStartRequest { cell_name, executable } =
            request;

        assert!(matches!(cell_name, CellNamePath::Empty));
        info!("CellService: start() executable={:?}", executable);

        let mut executables = self.executables.lock().await;
        let executable = executables
            .start(executable)
            .map_err(CellsServiceError::ExecutablesError)?;

        let pid = executable
            .pid()
            .map_err(CellsServiceError::Io)?
            .expect("pid")
            .as_raw();

        // TODO: either tell the [ObserveService] about this executable's log channels, or
        // provide a way for the observe service to extract the log channels from here.

        Ok(Response::new(CellServiceStartResponse { pid }))
    }

    #[tracing::instrument(skip(self))]
    async fn start_in_cell(
        &self,
        cell_name: &CellName,
        request: CellServiceStartRequest,
    ) -> std::result::Result<Response<CellServiceStartResponse>, Status> {
        do_in_cell!(self, cell_name, start, request)
    }

    #[tracing::instrument(skip(self))]
    async fn stop(
        &self,
        request: ValidatedCellServiceStopRequest,
    ) -> std::result::Result<Response<CellServiceStopResponse>, Status> {
        let ValidatedCellServiceStopRequest { cell_name, executable_name } =
            request;

        assert!(matches!(cell_name, CellNamePath::Empty));
        info!("CellService: stop() executable_name={:?}", executable_name,);

        let mut executables = self.executables.lock().await;
        let _exit_status = executables
            .stop(&executable_name)
            .await
            .map_err(CellsServiceError::ExecutablesError)?;

        Ok(Response::new(CellServiceStopResponse::default()))
    }

    #[tracing::instrument(skip(self))]
    async fn stop_in_cell(
        &self,
        cell_name: &CellName,
        request: CellServiceStopRequest,
    ) -> std::result::Result<Response<CellServiceStopResponse>, Status> {
        do_in_cell!(self, cell_name, stop, request)
    }
}

/// ### Mapping cgroup options to the Cell API
///
/// Here we *only* expose options from the CgroupBuilder
/// as our features in Aurae need them! We do not try to
/// "map" everything as much as we start with a few small
/// features and add as needed.
///
// Example builder options can be found: https://github.com/kata-containers/cgroups-rs/blob/main/tests/builder.rs
// Cgroup documentation: https://man7.org/linux/man-pages/man7/cgroups.7.html
#[tonic::async_trait]
impl cell_service_server::CellService for CellService {
    async fn allocate(
        &self,
        request: Request<CellServiceAllocateRequest>,
    ) -> std::result::Result<Response<CellServiceAllocateResponse>, Status>
    {
        let request = request.into_inner();

        // We execute allocate if cell_name is a direct child
        if matches!(&request.cell, Some(cell) if !cell.name.contains(cell_name_path::SEPARATOR))
        {
            let request = ValidatedCellServiceAllocateRequest::validate(
                request.clone(),
                None,
            )?;

            Ok(Response::new(self.allocate(request).await?))
        } else {
            let validated = ValidatedCellServiceAllocateRequest::validate(
                request.clone(),
                None,
            )?;

            // validation has succeed, so we can make assumptions about the request and use expect
            let (parent, cell_name) = validated
                .cell
                .name
                .into_child()
                .expect("CellNamePath was not empty");

            let mut request = request;
            if let Some(cell) = &mut request.cell {
                cell.name = cell_name.into_string();
            } else {
                unreachable!("validation should have failed")
            }

            self.allocate_in_cell(&parent, request).await
        }
    }

    async fn free(
        &self,
        request: Request<CellServiceFreeRequest>,
    ) -> std::result::Result<Response<CellServiceFreeResponse>, Status> {
        let request = request.into_inner();

        // We execute free if cell_name is a direct child
        if !request.cell_name.contains(cell_name_path::SEPARATOR) {
            let request = ValidatedCellServiceFreeRequest::validate(
                request.clone(),
                None,
            )?;
            Ok(Response::new(self.free(request).await?))
        } else {
            let validated = ValidatedCellServiceFreeRequest::validate(
                request.clone(),
                None,
            )?;

            // validation has succeeded, so we can make assumptions about the request and use expect
            let mut request = request;
            let (parent, cell_name) = validated
                .cell_name
                .into_child()
                .expect("CellNamePath was not empty");

            request.cell_name = cell_name.into_string();

            self.free_in_cell(&parent, request).await
        }
    }

    async fn start(
        &self,
        request: Request<CellServiceStartRequest>,
    ) -> std::result::Result<Response<CellServiceStartResponse>, Status> {
        let request = request.into_inner();

        // We execute start if cell_name is empty
        if request.cell_name.is_empty() {
            let request =
                ValidatedCellServiceStartRequest::validate(request, None)?;
            Ok(self.start(request).await?)
        } else {
            // We are in a parent cell (or validation will fail)
            let validated = ValidatedCellServiceStartRequest::validate(
                request.clone(),
                None,
            )?;

            // validation has succeed, so we can make assumptions about the request and use expect
            let mut request = request;
            let (parent, cell_name) = validated
                .cell_name
                .into_child()
                .expect("CellNamePath was not empty");

            request.cell_name = cell_name.into_string();

            self.start_in_cell(&parent, request).await
        }
    }

    async fn stop(
        &self,
        request: Request<CellServiceStopRequest>,
    ) -> std::result::Result<Response<CellServiceStopResponse>, Status> {
        let request = request.into_inner();

        // We execute stop if cell_name is empty.
        // Otherwise, we execute in a child
        if request.cell_name.is_empty() {
            let request =
                ValidatedCellServiceStopRequest::validate(request, None)?;
            Ok(self.stop(request).await?)
        } else {
            // We are in a parent cell (or validation will fail)
            let validated = ValidatedCellServiceStopRequest::validate(
                request.clone(),
                None,
            )?;

            // validation has succeed, so we can make assumptions about the request and use expect
            let mut request = request;
            let (parent, cell_name) = validated
                .cell_name
                .into_child()
                .expect("CellNamePath was not empty");

            request.cell_name = cell_name.into_string();

            self.stop_in_cell(&parent, request).await
        }
    }
}
