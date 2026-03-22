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

Settings can be managed via a configuration file. The layer searches for configuration in the following order:

1.  **`AUTOHDR_CONFIG` Environment Variable:** If set, this path is used exclusively.
2.  **Process-Specific Config:** `~/.config/autohdr/[process_name].conf` (e.g., `vkcube.conf`).
3.  **Global Config:** `~/.config/autohdr/autohdr.conf`.

### Automatic Creation
- If `autohdr.conf` does not exist, it is created with default values on the first run.
- If a program is run with **`AUTOHDR_ENABLE=1`** and its process-specific config does not exist, the layer automatically creates it as a 1:1 copy of the current `autohdr.conf`.

**Precedence:** Individual environment variables (starting with `AUTOHDR_`) always take precedence over the values found in any configuration file.

### Available Settings

| Variable / Config Key | Description | Default |
|-----------------------|-------------|---------|
| `AUTOHDR_CONFIG` | Custom path to the configuration file | None |
| `AUTOHDR_OUTPUT_FORMAT` / `preferred_format` | Output encoding: `pq` or `scrgb` | `pq` |
| `AUTOHDR_MAX_LUMINANCE` / `max_lum` | Peak brightness of your display in nits | (Auto from EDID) |
| `AUTOHDR_MID_LUMINANCE` / `mid_lum` | Target "Paper White" (mid-tones) in nits | (Auto/Heuristic) |
| `AUTOHDR_SDR_BRIGHTNESS`/ `sdr_brightness`| The base brightness of the SDR content (nits) | `100.0` |
| `AUTOHDR_INTENSITY` / `intensity` | Strength of the HDR transformation (0.0 to 1.0) | `1.0` |
| `AUTOHDR_SATURATION` / `sat` | Global color saturation multiplier | `1.0` |
| `AUTOHDR_VIBRANCE` / `vibrance` | Perceptual color enhancement (preserves skin tones) | `0.0` |
| `AUTOHDR_BLACK_LEVEL` / `black_level` | Fine-tune shadow depth / black floor lift | `0.0` |
| `AUTOHDR_RCAS` / `rcas_strength` | AMD FidelityFX RCAS sharpening (0.0 to 1.0) | `0.0` |
| `AUTOHDR_FXAA` / `fxaa_strength` | Fast Approximate Anti-Aliasing (0.0 to 1.0) | `0.0` |

## Technical Implementation Details

The core logic is implemented as a **Vulkan Implicit Layer** written in **Rust**, utilizing:
- **`ash`** for low-level Vulkan bindings.
- **Compute Shaders** for high-throughput pixel processing.
- **Inverse Tone Mapping** algorithms tailored for real-time gaming performance.

## License

This project is licensed under the **MIT License** - see the [LICENSE](LICENSE) file for details. It is intended as both a production tool for Linux gamers and a technical Proof of Concept for hardware vendors and driver developers.
