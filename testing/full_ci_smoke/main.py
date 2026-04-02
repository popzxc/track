#!/usr/bin/env python3

import argparse
import sys

from smoke_test.scenarios import load_scenario, scenario_names
from smoke_test.smoke_context import create_context


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run a full CI smoke scenario.")
    parser.add_argument(
        "--scenario",
        default="install-flow",
        choices=scenario_names(),
        help="Named smoke scenario to run.",
    )
    parser.add_argument(
        "--revision",
        help="Git revision to install through trackup --ref when the scenario needs it.",
    )
    parser.add_argument(
        "--expected-commit",
        help="Exact commit SHA the scenario expects the install ref to resolve to.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    scenario = load_scenario(args.scenario)
    if scenario.requires_revision and not args.revision:
        raise SystemExit(f"--revision is required for scenario {args.scenario!r}")

    context = create_context(args.revision, args.expected_commit)
    scenario.run(context)
    return 0


if __name__ == "__main__":
    sys.exit(main())
