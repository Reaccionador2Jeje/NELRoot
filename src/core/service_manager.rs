//! In-memory service lifecycle coordinator.
//!
//! This module connects validated service definitions to a future guest
//! runtime adapter. It never executes manifest commands on the host. A service
//! becomes Running only after an execution backend reports a real process ID.
//! State is intentionally in-memory until durable session recovery is designed.

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::core::service_manifest::{RestartPolicy, ServiceDefinition, ServiceManifest};

const MAX_LOG_ENTRIES_PER_SERVICE: usize = 256;
const MAX_LOG_MESSAGE_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Defined,
    StartRequested,
    Running,
    StopRequested,
    Exited,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceStatus {
    pub environment: String,
    pub name: String,
    pub state: ServiceState,
    pub process_id: Option<u32>,
    pub exit_code: Option<i32>,
    pub restart: RestartPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceLogEntry {
    pub sequence: u64,
    pub message: String,
}

#[derive(Debug, Clone)]
struct ManagedService {
    definition: ServiceDefinition,
    state: ServiceState,
    process_id: Option<u32>,
    exit_code: Option<i32>,
    logs: VecDeque<ServiceLogEntry>,
    next_log_sequence: u64,
}

/// Coordinates service lifecycle requests without launching host processes.
#[derive(Debug, Default)]
pub struct ServiceManager {
    services: BTreeMap<(String, String), ManagedService>,
}

impl ServiceManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register all definitions from a validated manifest for one environment.
    /// Re-registering an environment is rejected to avoid losing live state.
    pub fn register_manifest(
        &mut self,
        environment: &str,
        manifest: &ServiceManifest,
    ) -> Result<Vec<ServiceStatus>, String> {
        if !valid_identifier(environment) {
            return Err("invalid environment name".to_owned());
        }
        manifest.validate()?;

        if self.services.keys().any(|(name, _)| name == environment) {
            return Err(format!("services are already registered for '{environment}'"));
        }

        let mut inserted = Vec::with_capacity(manifest.services.len());
        for definition in &manifest.services {
            let key = (environment.to_owned(), definition.name.clone());
            let managed = ManagedService {
                definition: definition.clone(),
                state: ServiceState::Defined,
                process_id: None,
                exit_code: None,
                logs: VecDeque::new(),
                next_log_sequence: 1,
            };
            inserted.push((key, managed));
        }

        let mut statuses = Vec::with_capacity(inserted.len());
        for (key, service) in inserted {
            statuses.push(status_for(&key.0, &service));
            self.services.insert(key, service);
        }
        Ok(statuses)
    }

    pub fn list(&self, environment: &str) -> Vec<ServiceStatus> {
        self.services
            .iter()
            .filter(|((name, _), _)| name == environment)
            .map(|((name, _), service)| status_for(name, service))
            .collect()
    }

    pub fn inspect(&self, environment: &str, service: &str) -> Option<ServiceStatus> {
        self.services
            .get(&(environment.to_owned(), service.to_owned()))
            .map(|managed| status_for(environment, managed))
    }

    /// Record user intent. No process is spawned by this operation.
    pub fn request_start(&mut self, environment: &str, service: &str) -> Result<(), String> {
        let managed = self.get_mut(environment, service)?;
        match managed.state {
            ServiceState::Defined
            | ServiceState::Exited
            | ServiceState::Failed
            | ServiceState::Stopped => {
                managed.state = ServiceState::StartRequested;
                managed.process_id = None;
                managed.exit_code = None;
                Ok(())
            }
            state => Err(format!("cannot request start while service is {state:?}")),
        }
    }

    /// Confirm a successful spawn from the future isolated guest runtime.
    /// The caller must supply the actual supervised child PID, never a guess.
    pub fn confirm_started(
        &mut self,
        environment: &str,
        service: &str,
        process_id: u32,
    ) -> Result<(), String> {
        if process_id == 0 {
            return Err("process ID must be greater than zero".to_owned());
        }
        let managed = self.get_mut(environment, service)?;
        if managed.state != ServiceState::StartRequested {
            return Err("service has no pending start request".to_owned());
        }
        managed.process_id = Some(process_id);
        managed.state = ServiceState::Running;
        Ok(())
    }

    /// Record that the runtime could not spawn the requested guest process.
    pub fn record_start_failure(
        &mut self,
        environment: &str,
        service: &str,
        reason: &str,
    ) -> Result<(), String> {
        if reason.len() > MAX_LOG_MESSAGE_BYTES || reason.contains('\\0') {
            return Err("start failure reason is invalid or exceeds 4096 bytes".to_owned());
        }
        let managed = self.get_mut(environment, service)?;
        if managed.state != ServiceState::StartRequested {
            return Err("service has no pending start request".to_owned());
        }
        managed.process_id = None;
        managed.exit_code = None;
        managed.state = ServiceState::Failed;
        append_log_entry(managed, reason);
        Ok(())
    }

    /// Record a stop request. The runtime adapter is responsible for stopping
    /// and reaping the owned process before calling confirm_stopped.
    pub fn request_stop(&mut self, environment: &str, service: &str) -> Result<(), String> {
        let managed = self.get_mut(environment, service)?;
        match managed.state {
            ServiceState::Running | ServiceState::StartRequested => {
                managed.state = ServiceState::StopRequested;
                Ok(())
            }
            state => Err(format!("cannot request stop while service is {state:?}")),
        }
    }

    pub fn confirm_stopped(&mut self, environment: &str, service: &str) -> Result<(), String> {
        let managed = self.get_mut(environment, service)?;
        if managed.state != ServiceState::StopRequested {
            return Err("service has no pending stop request".to_owned());
        }
        managed.state = ServiceState::Stopped;
        managed.process_id = None;
        managed.exit_code = None;
        Ok(())
    }

    /// Record the observed exit of a supervised guest service process.
    pub fn record_exit(
        &mut self,
        environment: &str,
        service: &str,
        exit_code: i32,
    ) -> Result<(), String> {
        let managed = self.get_mut(environment, service)?;
        if managed.state != ServiceState::Running
            && managed.state != ServiceState::StopRequested
        {
            return Err("only a running service can report process exit".to_owned());
        }
        managed.process_id = None;
        managed.exit_code = Some(exit_code);
        managed.state = if exit_code == 0 {
            ServiceState::Exited
        } else {
            ServiceState::Failed
        };
        Ok(())
    }

    /// Whether policy permits a later restart attempt after an observed exit.
    /// This reports policy only; it does not schedule or perform a restart.
    pub fn restart_allowed(&self, environment: &str, service: &str) -> Result<bool, String> {
        let managed = self.get(environment, service)?;
        Ok(managed.definition.restart == RestartPolicy::OnFailure
            && managed.state == ServiceState::Failed)
    }

    pub fn append_log(
        &mut self,
        environment: &str,
        service: &str,
        message: &str,
    ) -> Result<(), String> {
        if message.len() > MAX_LOG_MESSAGE_BYTES || message.contains('\0') {
            return Err("log message is invalid or exceeds 4096 bytes".to_owned());
        }
        let managed = self.get_mut(environment, service)?;
        append_log_entry(managed, message);
        Ok(())
    }

    pub fn logs(&self, environment: &str, service: &str) -> Result<Vec<ServiceLogEntry>, String> {
        Ok(self.get(environment, service)?.logs.iter().cloned().collect())
    }

    fn get(&self, environment: &str, service: &str) -> Result<&ManagedService, String> {
        self.services
            .get(&(environment.to_owned(), service.to_owned()))
            .ok_or_else(|| format!("unknown service '{environment}/{service}'"))
    }

    fn get_mut(
        &mut self,
        environment: &str,
        service: &str,
    ) -> Result<&mut ManagedService, String> {
        self.services
            .get_mut(&(environment.to_owned(), service.to_owned()))
            .ok_or_else(|| format!("unknown service '{environment}/{service}'"))
    }
}

fn append_log_entry(service: &mut ManagedService, message: &str) {
    let entry = ServiceLogEntry {
        sequence: service.next_log_sequence,
        message: message.to_owned(),
    };
    service.next_log_sequence = service.next_log_sequence.saturating_add(1);
    if service.logs.len() == MAX_LOG_ENTRIES_PER_SERVICE {
        service.logs.pop_front();
    }
    service.logs.push_back(entry);
}

fn status_for(environment: &str, service: &ManagedService) -> ServiceStatus {
    ServiceStatus {
        environment: environment.to_owned(),
        name: service.definition.name.clone(),
        state: service.state,
        process_id: service.process_id,
        exit_code: service.exit_code,
        restart: service.definition.restart,
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn manifest() -> ServiceManifest {
        ServiceManifest {
            schema_version: 1,
            services: vec![ServiceDefinition {
                name: "web".to_owned(),
                command: "/usr/bin/python3".to_owned(),
                args: vec!["-m".to_owned(), "http.server".to_owned()],
                working_directory: "/srv/site".to_owned(),
                environment: BTreeMap::new(),
                restart: RestartPolicy::OnFailure,
            }],
        }
    }

    #[test]
    fn registers_services_as_defined_without_claiming_running() {
        let mut manager = ServiceManager::new();
        let statuses = manager.register_manifest("demo", &manifest()).unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].state, ServiceState::Defined);
        assert_eq!(statuses[0].process_id, None);
    }

    #[test]
    fn lifecycle_requires_runtime_confirmation() {
        let mut manager = ServiceManager::new();
        manager.register_manifest("demo", &manifest()).unwrap();
        manager.request_start("demo", "web").unwrap();
        assert_eq!(
            manager.inspect("demo", "web").unwrap().state,
            ServiceState::StartRequested
        );
        assert!(manager.confirm_started("demo", "web", 0).is_err());
        manager.confirm_started("demo", "web", 4242).unwrap();
        assert_eq!(
            manager.inspect("demo", "web").unwrap().state,
            ServiceState::Running
        );
        manager.request_stop("demo", "web").unwrap();
        manager.confirm_stopped("demo", "web").unwrap();
        assert_eq!(
            manager.inspect("demo", "web").unwrap().state,
            ServiceState::Stopped
        );
    }

    #[test]
    fn failed_spawn_is_reported_without_a_process_id() {
        let mut manager = ServiceManager::new();
        manager.register_manifest("demo", &manifest()).unwrap();
        manager.request_start("demo", "web").unwrap();
        manager
            .record_start_failure("demo", "web", "runtime unavailable")
            .unwrap();
        let status = manager.inspect("demo", "web").unwrap();
        assert_eq!(status.state, ServiceState::Failed);
        assert_eq!(status.process_id, None);
        assert_eq!(
            manager.logs("demo", "web").unwrap()[0].message,
            "runtime unavailable"
        );
    }

    #[test]
    fn restart_policy_only_reports_eligibility() {
        let mut manager = ServiceManager::new();
        manager.register_manifest("demo", &manifest()).unwrap();
        manager.request_start("demo", "web").unwrap();
        manager.confirm_started("demo", "web", 99).unwrap();
        manager.record_exit("demo", "web", 2).unwrap();
        assert!(manager.restart_allowed("demo", "web").unwrap());
        assert_eq!(
            manager.inspect("demo", "web").unwrap().state,
            ServiceState::Failed
        );
    }

    #[test]
    fn log_buffer_is_bounded_and_keeps_recent_entries() {
        let mut manager = ServiceManager::new();
        manager.register_manifest("demo", &manifest()).unwrap();
        for index in 0..300 {
            manager
                .append_log("demo", "web", &format!("entry-{index}"))
                .unwrap();
        }
        let logs = manager.logs("demo", "web").unwrap();
        assert_eq!(logs.len(), MAX_LOG_ENTRIES_PER_SERVICE);
        assert_eq!(logs.first().unwrap().message, "entry-44");
        assert_eq!(logs.last().unwrap().message, "entry-299");
    }

    #[test]
    fn rejects_duplicate_registration_and_invalid_names() {
        let mut manager = ServiceManager::new();
        assert!(manager.register_manifest("../host", &manifest()).is_err());
        manager.register_manifest("demo", &manifest()).unwrap();
        assert!(manager.register_manifest("demo", &manifest()).is_err());
    }
}
