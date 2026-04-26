# AGENTS.md

- Prefer using `just` for running actions on the workspace unless there is a reason not to.
- After significant changes, run all the tests, even if the code you changed doesn't seem much.
- Add reasons for why the code exists if it's not obvious.
- Prefer few high-signal tests over a ton of low-signal ones.
- The code as of now is not very idiomatic, as the codebase has been vibe-coded initially; we're trying to clean it up -- do not make it worse.
- Do not introduce hacks or lazy solutions -- if the problem is complex, bring it up.
- Before implementing a change, think: would it be easier/more maintainable if we would do a refactoring first. If so, propose doing refactoring. Improving the code quality is a good idea as this project grows.
- Maintaining weird edge cases (e.g. non-default workflow paths that are technically possible because the configuration is/was too permissive) is a non-goal. We are focused on the default use-case path. Breaking backward compatibility for such edge cases is typically fine, especially if it improves the code quality/maintainability, but you should bring it up upfront.
