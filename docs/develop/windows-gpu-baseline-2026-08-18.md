# Windows x64 NVIDIA GPU baseline (official device registration)

Status: registered by product decision on 2026-08-18. This machine is the
Windows x64 NVIDIA GPU baseline device for W4 GPU release evidence.

## Device identity

- GPU: NVIDIA GeForce RTX 2070 with Max-Q Design
- GPU device id: `0x1f14`
- Driver: `555.97.0.0`
- Vulkan API reported by driver: `1.3.278`
- Vulkan SDK: `1.4.357.0`
- Windows: `Microsoft Windows NT 10.0.26200.0`
- Selected backend: `Vulkan0` (NVIDIA discrete GPU)
- Secondary GPU present but not selected: Intel UHD Graphics

## Frozen run configuration

- Model: `ovisocr2`
- Profile: `compact`
- Model/mmproj revision: `8a22290a0c42dbcf84739d6d4c2763f877494ae0`
- Fixture: `benchmarks/fixtures/pdf/figure-1-page.pdf`
- Pages: `1`
- DPI: `300`
- Max output tokens: `128`
- Device: `vulkan`
- Offline: yes

## Vulkan result

```json
{"schema_version":1,"backend":"vulkan","vram_peak_bytes":1637875712,"unified_memory_peak_bytes":null,"load_time_ms":1444,"first_token_ms":49,"output_tokens":26,"output_tokens_per_sec":76.246,"peak_memory_bytes":null}
```

- `result.json` status: `success`
- `images.count`: `1`
- `result.json` elapsed_ms: `49837`

## CPU comparison

- Device: `cpu`
- `result.json` status: `success`
- `images.count`: `1`
- `result.json` elapsed_ms: `350014`

Observed speedup on this fixture: approximately `350014 / 49837 = 7.02x`.
Structural output matches CPU (one figure crop). Quality scoring remains gated
by W0.4 official BF16 reference outputs and the 30+ page regression set.

## Evidence paths

- Vulkan output: `target/w44-vulkan-official/`
- CPU output: `target/w44-cpu-official/`
