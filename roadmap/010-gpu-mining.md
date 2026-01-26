---
title: "GPU Mining Support"
priority: 4
status: planned
tags: [mining, performance]
dependencies: []
---

# GPU Mining Support

Implement GPU-accelerated mining using SHA3-512.

## Goals

- Significant hashrate improvement over CPU
- Support CUDA (NVIDIA) and/or OpenCL (AMD, others)
- Optional feature, CPU mining remains available

## Design Considerations

SHA3/Keccak is less GPU-friendly than SHA256, but still benefits from parallelization:
- Each GPU thread tries different nonces
- 256-bit nonce space allows massive parallelization
- Memory bandwidth is not a bottleneck for SHA3

## Implementation Options

1. **CUDA** - Best NVIDIA performance, NVIDIA-only
2. **OpenCL** - Cross-platform, slightly lower performance
3. **Vulkan Compute** - Modern cross-platform option

Recommend starting with OpenCL for broader compatibility.

## Tasks

- [ ] Research existing SHA3 GPU implementations
- [ ] Implement GPU mining module behind feature flag
- [ ] Benchmark against CPU mining
- [ ] Auto-detect available GPUs
- [ ] Configuration for GPU selection

## Configuration

```toml
[mining]
enabled = true
device = "gpu"  # or "cpu"
gpu_devices = [0, 1]  # Which GPUs to use
```

## Acceptance Criteria

- GPU mining works on at least one platform
- >10x speedup over single-threaded CPU
- Falls back gracefully if no GPU available
