from .actions import (
    apply_install_flow_defaults,
    apply_install_flow_overrides,
    build_backend_image,
    capture_task,
    cleanup_environment,
    close_task,
    configure_cli,
    configure_remote_agent,
    dispatch_task,
    ensure_ci_only_execution,
    install_track,
    print_check_successful,
    print_install_flow_summary,
    print_failure_diagnostics,
    prepare_install_flow_runtime,
    register_project_checkout,
    request_example_dot_com,
    request_review,
    start_backend,
    start_fixture,
)
from .scenario import Scenario, ScenarioAction


def install_flow_defaults_scenario() -> Scenario:
    return Scenario(
        name="install-flow-defaults",
        requires_revision=True,
        actions=[
            ScenarioAction("Validate CI-only guardrails", ensure_ci_only_execution),
            ScenarioAction(
                "Use default backend and remote-agent option values where practical",
                apply_install_flow_defaults,
            ),
            ScenarioAction(
                "Prepare the runtime ports and backend URL for the install flow",
                prepare_install_flow_runtime,
            ),
            ScenarioAction("Build the backend image for this revision", build_backend_image),
            ScenarioAction("Start the SSH fixture and seed remote state", start_fixture),
            ScenarioAction("Install track through trackup --ref", install_track),
            ScenarioAction("Boot the installed backend and verify it is healthy", start_backend),
            ScenarioAction("Configure the installed CLI against the packaged backend", configure_cli),
            ScenarioAction("Register a real git checkout with the installed CLI", register_project_checkout),
            ScenarioAction("Configure the installed remote agent", configure_remote_agent),
            ScenarioAction("Capture a task through the deterministic smoke seam", capture_task),
            ScenarioAction("Dispatch the task and wait for the remote run to finish", dispatch_task),
            ScenarioAction("Request a PR review and wait for submission", request_review),
            ScenarioAction("Close the task and verify it stays visible as closed", close_task),
        ],
        on_success=print_install_flow_summary,
        on_failure=print_failure_diagnostics,
        on_cleanup=cleanup_environment,
    )


def install_flow_overrides_scenario() -> Scenario:
    return Scenario(
        name="install-flow-overrides",
        requires_revision=True,
        actions=[
            ScenarioAction("Validate CI-only guardrails", ensure_ci_only_execution),
            ScenarioAction(
                "Use explicit backend and remote-agent overrides",
                apply_install_flow_overrides,
            ),
            ScenarioAction(
                "Prepare the runtime ports and backend URL for the install flow",
                prepare_install_flow_runtime,
            ),
            ScenarioAction("Build the backend image for this revision", build_backend_image),
            ScenarioAction("Start the SSH fixture and seed remote state", start_fixture),
            ScenarioAction("Install track through trackup --ref", install_track),
            ScenarioAction("Boot the installed backend and verify it is healthy", start_backend),
            ScenarioAction("Configure the installed CLI against the packaged backend", configure_cli),
            ScenarioAction("Register a real git checkout with the installed CLI", register_project_checkout),
            ScenarioAction("Configure the installed remote agent", configure_remote_agent),
            ScenarioAction("Capture a task through the deterministic smoke seam", capture_task),
            ScenarioAction("Dispatch the task and wait for the remote run to finish", dispatch_task),
            ScenarioAction("Request a PR review and wait for submission", request_review),
            ScenarioAction("Close the task and verify it stays visible as closed", close_task),
        ],
        on_success=print_install_flow_summary,
        on_failure=print_failure_diagnostics,
        on_cleanup=cleanup_environment,
    )


def connectivity_check_scenario() -> Scenario:
    return Scenario(
        name="connectivity-check",
        actions=[
            ScenarioAction("Make a requests probe to example.com", request_example_dot_com),
            ScenarioAction("Report a successful smoke check", print_check_successful),
        ],
        on_cleanup=cleanup_environment,
    )


SCENARIOS = {
    "connectivity-check": connectivity_check_scenario,
    "install-flow": install_flow_defaults_scenario,
    "install-flow-defaults": install_flow_defaults_scenario,
    "install-flow-overrides": install_flow_overrides_scenario,
}


def scenario_names() -> list[str]:
    return sorted(SCENARIOS)


def load_scenario(name: str) -> Scenario:
    try:
        return SCENARIOS[name]()
    except KeyError as error:
        raise RuntimeError(f"Unknown smoke scenario: {name}") from error
