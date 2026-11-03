# How Vet works

Vet helps users choose game settings inside a Varmint VM. It reads the current
Steam settings, finds a tested setup, and changes anything only after the user
confirms it.

## Where settings live

Steam stores the real game settings:

- `LaunchOptions` contains the graphics mode and game arguments;
- `CompatToolMapping` contains the Proton override for a game;
- if there is no override, Vet uses Steam's global choice and then Varmint's
  default, Proton Experimental.

The graphics mode appears directly in `LaunchOptions`:

```text
varmint-graphics venus %command%
varmint-graphics neptune %command%
varmint-graphics neptune-dx12 %command%
```

Vet does not keep another copy of these settings. Its own files contain only
preferences, install tracking, caches, and optional history. Vet reads the
current values from Steam whenever it needs them.

## Where recommendations come from

The game guides in `docs/games` are written for both people and Vet. The build
checks their structured setup profiles and creates
`guest/varmint-vet-catalog.json`. Vet uses this catalog instead of parsing
Markdown in the VM.

A Varmint guide wins over PCGamingWiki. Vet uses a PCGamingWiki result only
when the page contains the exact Steam AppID and enough structured information.
It does not turn an uncertain match into a recommendation.

When there is no Varmint guide, Vet follows these defaults:

```text
D3D9             -> Venus
32-bit D3D11     -> Venus
64-bit D3D11     -> Neptune
D3D12            -> Neptune DX12
D3D11 and D3D12  -> Neptune, starting with D3D11
unknown          -> no recommendation
```

Vet adds `-dx11`, `-dx12`, or similar arguments only when a trusted source says
they are needed. A guide may show terminal commands, but Vet never runs command
text downloaded in a catalog. Any automated command must be implemented and
validated in Vet itself.

Vet checks the format and minimum Vet version of a downloaded catalog. If it
cannot use the download, it tries the last valid copy and then the catalog
included in the VM. Developers can choose a local catalog with
`VARMINT_VET_CATALOG`; errors in a local catalog are shown immediately.

## Host graphics backend

Venus and native Vulkan use MoltenVK. Neptune DirectX 11 works with DXMT or
D3DMetal, and normally recommends DXMT. Neptune DX12 needs D3DMetal.

The host tells the VM which Neptune backend it selected through
`varmint.neptune_backend=dxmt|d3dmetal` on the kernel command line. Vet uses
this value to warn about an incompatible DX12 setup. An older host may not
provide it; in that case Vet shows the backend as unknown instead of guessing.
Changing the host backend requires restarting the VM.

## Changing Steam settings

The normal path copies the complete `LaunchOptions` value and opens the game's
Steam properties. Steam stays open and the user pastes the value.

Vet can also apply settings automatically. It stops Steam first so Steam cannot
overwrite the change from memory. It then reads the files again, checks that
the settings have not changed since confirmation, writes the new values, and
checks the result. If any part fails, Vet restores the affected files before
restarting Steam.

Vet only edits parts of `LaunchOptions` that it understands. It keeps unrelated
environment variables and game arguments. It rejects broken quoting, shell
operators, multiple `%command%` markers, and unknown Varmint wrappers.

## New installs and reports

When its user service starts, Vet treats installed games as the starting list.
It suggests settings only for games installed afterwards. Turning automatic
suggestions off does not disable the Vet window or diagnostic reports.

The user can ask Vet to capture the next launch of a game. `varmint-graphics`
adds Proton logging to that launch only, waits for it to finish, and records the
result. It does not edit Steam settings, and a problem while saving diagnostics
must not change the game's exit status.

Reports contain the captured setup, facts Vet recognized, clearly marked
guesses, and a bounded part of the log that fits through the clipboard. Vet
marks an old report as stale when the graphics mode, Proton version, launch
options, or game build has changed. Time alone does not make a report stale.

## Scope

Vet uses Steam, existing guest services, desktop notifications, and the shared
clipboard. It does not add a host controller or a new host/guest protocol.
