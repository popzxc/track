from .actions import (
    apply_install_flow_linux_docker_defaults,
    apply_install_flow_linux_docker_overrides,
    apply_install_flow_macos_host_defaults,
    apply_install_flow_macos_host_overrides,
    align_project_metadata_with_fixture,
    build_backend_image,
    capture_task,
    cleanup_environment,
    close_task,
    configure_cli,
    configure_remote_agent,
    dispatch_task,
    ensure_ci_only_execution,
    install_track,
    prepare_install_flow_runtime,
    prepare_macos_host_tooling,
    print_check_successful,
    print_failure_diagnostics,
    print_install_flow_summary,
    register_project_checkout,
    request_example_dot_com,
    request_review,
    start_backend,
    start_linux_docker_fixture,
    start_macos_host_fixture,
)
from .scenario import Scenario, ScenarioAction


def install_flow_linux_docker_defaults_scenario() -> Scenario:
    return install_flow_scenario(
        name="install-flow-linux-docker-defaults",
        configure_defaults=apply_install_flow_linux_docker_defaults,
        platform_setup_actions=[],
        fixture_action=ScenarioAction(
            "Start the SSH fixture container and seed remote state",
            start_linux_docker_fixture,
        ),
    )


def install_flow_linux_docker_overrides_scenario() -> Scenario:
    return install_flow_scenario(
        name="install-flow-linux-docker-overrides",
        configure_defaults=apply_install_flow_linux_docker_overrides,
        platform_setup_actions=[],
        fixture_action=ScenarioAction(
            "Start the SSH fixture container and seed remote state",
            start_linux_docker_fixture,
        ),
    )


def install_flow_macos_host_defaults_scenario() -> Scenario:
    return install_flow_scenario(
        name="install-flow-macos-host-defaults",
        configure_defaults=apply_install_flow_macos_host_defaults,
        platform_setup_actions=[
            ScenarioAction(
                "Install the strict host-mode Docker and SSH transport shims",
                prepare_macos_host_tooling,
            )
        ],
        fixture_action=ScenarioAction(
            "Prepare the host-mode remote fixture state",
            start_macos_host_fixture,
        ),
    )


def install_flow_macos_host_overrides_scenario() -> Scenario:
    return install_flow_scenario(
        name="install-flow-macos-host-overrides",
        configure_defaults=apply_install_flow_macos_host_overrides,
        platform_setup_actions=[
            ScenarioAction(
                "Install the strict host-mode Docker and SSH transport shims",
                prepare_macos_host_tooling,
            )
        ],
        fixture_action=ScenarioAction(
            "Prepare the host-mode remote fixture state",
            start_macos_host_fixture,
        ),
    )


def install_flow_scenario(
    *,
    name: str,
    configure_defaults,
    platform_setup_actions: list[ScenarioAction],
    fixture_action: ScenarioAction,
) -> Scenario:
    return Scenario(
        name=name,
        requires_revision=True,
        actions=[
            ScenarioAction("Validate CI-only guardrails", ensure_ci_only_execution),
            ScenarioAction("Apply the scenario-specific install-flow defaults", configure_defaults),
            ScenarioAction(
                "Prepare the runtime ports and backend URL for the install flow",
                prepare_install_flow_runtime,
            ),
            *platform_setup_actions,
            ScenarioAction("Build the backend image for this revision", build_backend_image),
            ScenarioAction("Install track through trackup --ref", install_track),
            ScenarioAction("Boot the installed backend and verify it is healthy", start_backend),
            fixture_action,
            ScenarioAction("Configure the installed CLI against the packaged backend", configure_cli),
            ScenarioAction("Register a real git checkout with the installed CLI", register_project_checkout),
            ScenarioAction(
                "Align the registered project metadata with the seeded Git fixture",
                align_project_metadata_with_fixture,
            ),
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
    "install-flow": install_flow_linux_docker_defaults_scenario,
    "install-flow-defaults": install_flow_linux_docker_defaults_scenario,
    "install-flow-overrides": install_flow_linux_docker_overrides_scenario,
    "install-flow-linux-docker-defaults": install_flow_linux_docker_defaults_scenario,
    "install-flow-linux-docker-overrides": install_flow_linux_docker_overrides_scenario,
    "install-flow-macos-host-defaults": install_flow_macos_host_defaults_scenario,
    "install-flow-macos-host-overrides": install_flow_macos_host_overrides_scenario,
}


def scenario_names() -> list[str]:
    return sorted(SCENARIOS)


def load_scenario(name: str) -> Scenario:
    try:
        return SCENARIOS[name]()
    except KeyError as error:
        raise RuntimeError(f"Unknown smoke scenario: {name}") from error
