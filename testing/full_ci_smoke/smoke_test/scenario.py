from collections.abc import Callable
from dataclasses import dataclass
import sys

from .smoke_context import SmokeContext


@dataclass(frozen=True)
class ScenarioAction:
    name: str
    execute: Callable[[SmokeContext], None]


@dataclass(frozen=True)
class Scenario:
    name: str
    actions: list[ScenarioAction]
    requires_revision: bool = False
    on_success: Callable[[SmokeContext], None] | None = None
    on_failure: Callable[[SmokeContext], None] | None = None
    on_cleanup: Callable[[SmokeContext], None] | None = None

    def run(self, context: SmokeContext) -> None:
        try:
            for action in self.actions:
                print(f"\n==> {action.name}")
                action.execute(context)
            if self.on_success is not None:
                self.on_success(context)
        except Exception:
            if self.on_failure is not None:
                try:
                    self.on_failure(context)
                except Exception as error:  # noqa: BLE001
                    print(f"Could not collect failure diagnostics: {error}", file=sys.stderr)
            raise
        finally:
            if self.on_cleanup is not None:
                try:
                    self.on_cleanup(context)
                except Exception as error:  # noqa: BLE001
                    print(f"Could not clean up smoke environment: {error}", file=sys.stderr)
