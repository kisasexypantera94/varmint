# Running Black Myth: Wukong Benchmark Tool in Varmint

The benchmark is playable through Neptune's DirectX 12 mode using D3DMetal.

## Quick setup

Use the following configuration:

```text
Steam App IDs: 3132990
Profile:        dx12
Recommendation: Recommended
Evidence:       Tested
Setup:          Automatic
Runtime:        Proton
Proton version: Proton Experimental (ARM64)
Renderer:       Neptune DX12
Host backend:    D3DMetal
Launch options: varmint-graphics neptune-dx12 %command%
```

### 1. Select D3DMetal on the host

In the Varmint launcher, select **D3DMetal** before starting the VM.

### 2. Select Proton Experimental (ARM64)

Open the benchmark's Steam properties and select **Proton Experimental (ARM64)** under **Compatibility**.

### 3. Enable Neptune DX12

Set the following Steam launch options:

```text
varmint-graphics neptune-dx12 %command%
```
