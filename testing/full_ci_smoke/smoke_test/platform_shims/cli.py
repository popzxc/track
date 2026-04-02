from .docker_compose import docker_compose_main
from .docker_image import docker_image_main
from .transport import scp_main, ssh_main


# ==============================================================================
# Strict Host-Mode CLI Shims
# ==============================================================================
#
# These shims exist only for the macOS smoke scenarios. They are intentionally
# narrow and fail fast when the calling contract changes, because the value of
# the smoke comes from proving that the installed wrapper scripts still invoke
# the expected Docker and SSH commands.


def main(argv: list[str]) -> int:
    if not argv:
        raise SystemExit("Missing shim kind.")

    command = argv[0]
    if command == "docker":
        return docker_main(argv[1:])
    if command == "ssh":
        return ssh_main(argv[1:])
    if command == "scp":
        return scp_main(argv[1:])

    raise SystemExit(f"Unsupported shim kind: {command}")


def docker_main(argv: list[str]) -> int:
    if argv[:1] == ["build"] or argv[:2] == ["image", "inspect"]:
        return docker_image_main(argv)
    if argv == ["compose", "version"] or argv[:1] == ["compose"]:
        return docker_compose_main(argv)

    raise SystemExit(f"Unsupported docker invocation: {argv}")
