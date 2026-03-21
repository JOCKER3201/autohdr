# AutoHDR Vulkan Layer

AutoHDR is a high-performance, system-aware Vulkan layer designed to bring advanced HDR reconstruction capabilities to the Linux ecosystem. Inspired by technologies like NVIDIA RTX HDR, this project focuses on providing the highest possible image quality with a "zero-compromise" approach to precision.

## Technical Merits

### 1. 32-bit Computational Depth (FP32)
Unlike many real-time HDR solutions that use 16-bit (FP16) math to save on performance, AutoHDR performs **all** color space transformations and luminance mastering using **full 32-bit floating point (FP32) precision**. This ensures that even the most subtle gradients in SDR content are preserved and expanded without rounding errors or banding before the final encoding stage.

### 2. Modern Encoding Support: scRGB & PQ
AutoHDR supports the two primary standards for HDR transmission:
- **scRGB (16-bit Float):** Offers the highest possible fidelity by sending high-precision linear data directly to the compositor (e.g., KWin). This avoids the "double-tonemapping" issue and provides a mathematically perfect representation of the reconstructed HDR signal.
- **PQ (10-bit UNORM):** A robust implementation of SMPTE ST 2084 with **perceptual-space dithering**. By applying dithering in the non-linear PQ space rather than linear space, we significantly improve shadow detail and eliminate banding on 10-bit displays.

### 3. Native Desktop Integration & Automatic Detection
AutoHDR is designed to be invisible and automatic. It features:
- **Automatic Display Profiling:** Reads HDR metadata (Max/Mid Luminance) directly from the monitor's **EDID** via sysfs.
- **Environment Awareness:** Seamlessly integrates with **KDE Plasma 6** and **GNOME** to respect system-level HDR brightness overrides and primary monitor settings.
- **Intelligent Fallbacks:** Automatically detects if the compositor or display supports scRGB and performs a graceful, logged fallback to PQ if necessary.

## Configuration

Settings can be overridden via environment variables:

| Variable | Description | Default |
|----------|-------------|---------|
| `AUTOHDR_OUTPUT_FORMAT` | Output encoding: `pq` or `scrgb` | `pq` |
| `AUTOHDR_MAX_LUMINANCE` | Peak brightness of your display in nits | (Auto from EDID) |
| `AUTOHDR_MID_LUMINANCE` | Target "Paper White" (mid-tones) in nits | (Auto/Heuristic) |
| `AUTOHDR_SDR_BRIGHTNESS`| The base brightness of the SDR content (nits) | `100.0` |
| `AUTOHDR_INTENSITY` | Strength of the HDR transformation (0.0 to 1.0) | `1.0` |
| `AUTOHDR_SATURATION` | Global color saturation multiplier | `1.0` |
| `AUTOHDR_VIBRANCE` | Perceptual color enhancement (preserves skin tones) | `0.0` |
| `AUTOHDR_BLACK_LEVEL` | Fine-tune shadow depth / black floor lift | `0.0` |
| `AUTOHDR_RCAS` | AMD FidelityFX RCAS sharpening (0.0 to 1.0) | `0.0` |

## Technical Implementation Details

The core logic is implemented as a **Vulkan Implicit Layer** written in **Rust**, utilizing:
- **`ash`** for low-level Vulkan bindings.
- **Compute Shaders** for high-throughput pixel processing.
- **Inverse Tone Mapping** algorithms tailored for real-time gaming performance.

## License

This project is licensed under the **MIT License** - see the [LICENSE](LICENSE) file for details. It is intended as both a production tool for Linux gamers and a technical Proof of Concept for hardware vendors and driver developers.
