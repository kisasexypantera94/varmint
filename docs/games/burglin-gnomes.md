# Running Burglin' Gnomes in Varmint

Burglin' Gnomes is playable in Varmint using Neptune.

## Quick setup

Use the following configuration:

```text
Steam App IDs: 3844970
Profile:        default
Recommendation: Recommended
Evidence:       Tested
Setup:          Automatic
Runtime:        Proton
Proton version: Proton Experimental
Renderer:       Neptune
Host backend:    DXMT
Launch options: varmint-graphics neptune %command%
```

### 1. Enable Neptune

Set the following Steam launch options:

```text
varmint-graphics neptune %command%
```

No additional configuration is required.

## Known issues

### Performance may drop after changing graphics settings

Changing graphics settings while the game is running may cause a significant FPS drop.

If this happens, restart the game. Performance should return to normal afterwards.

## Tested configuration

```text
Burglin' Gnomes
Proton Experimental
DirectX 11
Neptune
```
