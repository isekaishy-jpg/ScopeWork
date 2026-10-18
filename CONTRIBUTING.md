# Contributing to Scopework

Scopework is a local Windows research workbench for people and agents. Focus contributions on clear research records, reliable storage and a straightforward desktop workflow.

## Get started

1. Read [AGENTS.md](AGENTS.md) for project rules.
2. Follow the [build guide](docs/building.md).
3. Keep changes focused on one problem and explain the resulting behavior.

Preserve the approved dashboard layout unless a design change is part of the request. Keep GUI and CLI behavior aligned. Installer changes must preserve research and retain the product upgrade identity.

## Issues and pull requests

For bugs, include the Scopework version, Windows version, reproduction steps, expected behavior and actual behavior. Use a small synthetic example where possible. Do not upload proprietary binaries, private research, credentials or captures.

For feature requests, explain the research task and the specific obstacle. Small concrete proposals are easier to discuss and maintain.

Pull requests should describe what changed and the validation actually performed. Do not claim installation, upgrade or runtime behavior was checked when only a build was run. Research does not get tests; this software does: add focused regression tests for meaningful behavior changes, using small self-cleaning fixtures (see AGENTS.md).

## Research and local files

Research belongs in the selected research home, outside the repository. Keep build output, MSI files, local tools and research data out of Git. Release binaries belong in GitHub release assets.

Agent complaints remain simple local Markdown reports in the shared complaints folder printed by `research home`. Filing one does not automatically create a GitHub issue.

## License

Contributions are provided under the project's [MIT license](LICENSE).
