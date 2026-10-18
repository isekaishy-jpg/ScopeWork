<p align="center">
  <img src="assets/scopework.png" width="96" height="96" alt="Scopework icon">
</p>

<h1 align="center">Scopework</h1>
<p align="center">A local research workbench for people and agents.</p>
<p align="center">
  <a href="https://github.com/isekaishy-jpg/ScopeWork/actions/workflows/windows.yml"><img src="https://github.com/isekaishy-jpg/ScopeWork/actions/workflows/windows.yml/badge.svg" alt="Windows build"></a>
  <a href="https://github.com/isekaishy-jpg/ScopeWork/releases"><img src="https://img.shields.io/github/v/release/isekaishy-jpg/ScopeWork?include_prereleases" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT license"></a>
  <img src="https://img.shields.io/badge/platform-Windows_x64-0078D4" alt="Windows x64">
</p>

Scopework organizes binary research into a hierarchy of canonical reports. A desktop dashboard shows progress; a JSON CLI lets agents use the same records, dependencies and completion rules.

**[Get started](docs/installation.md)** · **[Releases](https://github.com/isekaishy-jpg/ScopeWork/releases)** · **[CLI guide](docs/cli.md)** · **[Contributing](CONTRIBUTING.md)**

## Research that stays bounded

```text
App / project understanding
  Architecture
    Chunk
      Function + evidence
```

Each task has a fixed scope. Functions establish their own behavior and connections. Chunks synthesize completed functions; architectures synthesize chunks. Completed children make a parent ready for synthesis, but never automatically complete it.

- **Isolated projects:** separate binary identities, reports and supporting data.
- **One canonical report per item:** supporting captures stay beside the function they explain.
- **Explicit prerequisites:** shared understanding is referenced, with dependent work reopened when it changes.
- **Whole-binary inventory:** batch enrollment and reorganization, searchable collapsed records, and explicit context membership.
- **Visible progress:** project tabs, chunk meters, required and inventory function counts, and a short research-topic header.
- **GUI and CLI together:** background snapshots update meters; independent draft edits can proceed during additive imports.
- **Several agents at once:** lock waiting instead of busy failures, advisory work claims, a review queue for records reopened by moves, and a readable project history.
- **Resumable imports:** durable reservations, preparation checkpoints, pause/resume/cancel, and catalog-last publication.
- **Local files:** your research home stays under your control and survives uninstall.

## Install

Get a Windows x64 MSI from [Releases](https://github.com/isekaishy-jpg/ScopeWork/releases), or [build it yourself](docs/building.md). The wizard lets you choose the application folder, research home, PATH access and desktop shortcut.

Open Scopework from Start or the desktop shortcut. With PATH enabled, open a new terminal:

```powershell
research --help
research home
research project list
```

In the app, use **File > New project...** to create a project and **View > Research records** to build and research its hierarchy. Set **Research scope** (Project tab) to a few words such as *Rendering* or *Asset platform*.

## Status

Scopework is in early development. MSI packages are unsigned. Read each release's validation notes before using it for important work. Completion records are evidence-backed attestations, not automatic proof of research quality. New installations start empty. Create your own projects; dashboard progress comes only from canonical research records.

## Documentation

| Guide | What it covers |
| --- | --- |
| [Installation](docs/installation.md) | Setup choices, research locations, upgrades and uninstall |
| [Research model](docs/research-model.md) | Hierarchy, evidence, completion and prerequisites |
| [Agent CLI](docs/cli.md) | Creating records, writing reports and managing dependencies |
| [Build guide](docs/building.md) | Source builds, MSI packaging and repository layout |
| [Changelog](CHANGELOG.md) | Changes by version |
| [0.8.0 qualification](docs/qualification-0.8.0.md) | Multi-agent throughput measurements, coverage and known limits |
| [0.5.0 qualification](docs/qualification-0.5.0.md) | Recovery, concurrency, scale measurements and installer checks |
| [Concurrent import design](docs/concurrent-import-design.md) | Storage and concurrency design for imports, edits, and responsive snapshots |

## Contribute

Bug reports and focused improvements are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md). Research datasets, captures and proprietary binaries do not belong in this repository.

## License

Scopework is licensed under [MIT](LICENSE). Third-party dependencies retain their respective licenses.
