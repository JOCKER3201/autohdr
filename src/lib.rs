#![allow(non_snake_case)]
use ash::vk;
use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::sync::RwLock;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use std::fs;
use std::path::PathBuf;

/*
 * AutoHDR Vulkan Layer
 * 
 * A high-performance Vulkan layer that automatically enhances SDR content to HDR.
 * Key Features:
 * - 32-bit (FP32) computational depth for maximum precision.
 * - Automatic display detection via EDID and Desktop Environment (KDE/GNOME) integration.
 * - Support for PQ (10-bit) and scRGB (16-bit float) output formats.
 */

include!("shader.rs");

#[repr(C)] pub struct NegotiateLayerInterface { pub s_type: u32, pub p_next: *const c_void, pub loader_layer_interface_version: u32, pub pfn_get_instance_proc_addr: Option<vk::PFN_vkGetInstanceProcAddr>, pub pfn_get_device_proc_addr: Option<vk::PFN_vkGetDeviceProcAddr>, pub pfn_get_physical_device_tool_properties: Option<*const c_void> }
#[repr(C)] pub struct VkLayerInstanceLink { pub p_next: *mut VkLayerInstanceLink, pub pfn_next_get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr, pub pfn_next_get_device_proc_addr: vk::PFN_vkGetDeviceProcAddr }
#[repr(C)] pub struct VkLayerInstanceCreateInfo { pub s_type: vk::StructureType, pub p_next: *const c_void, pub function: u32, pub p_layer_info: *mut VkLayerInstanceLink }
#[repr(C)] pub struct VkLayerDeviceLink { pub p_next: *mut VkLayerDeviceLink, pub pfn_next_get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr, pub pfn_next_get_device_proc_addr: vk::PFN_vkGetDeviceProcAddr }
#[repr(C)] pub struct VkLayerDeviceCreateInfo { pub s_type: vk::StructureType, pub p_next: *const c_void, pub function: u32, pub p_layer_info: *mut VkLayerDeviceLink }

lazy_static::lazy_static! {
    static ref INSTANCE_GIPA: RwLock<HashMap<vk::Instance, vk::PFN_vkGetInstanceProcAddr>> = RwLock::new(HashMap::new());
    static ref DEVICE_GDPA: RwLock<HashMap<vk::Device, vk::PFN_vkGetDeviceProcAddr>> = RwLock::new(HashMap::new());
    static ref DEVICE_CONTEXTS: RwLock<HashMap<vk::Device, DeviceContext>> = RwLock::new(HashMap::new());
    static ref SWAPCHAIN_STATES: RwLock<HashMap<vk::SwapchainKHR, SwapchainState>> = RwLock::new(HashMap::new());
    static ref QUEUE_TO_DEVICE: RwLock<HashMap<vk::Queue, (vk::Device, u32)>> = RwLock::new(HashMap::new());
    static ref PHYSICAL_DEVICE_TO_INSTANCE: RwLock<HashMap<vk::PhysicalDevice, vk::Instance>> = RwLock::new(HashMap::new());
    static ref SURFACE_NATIVE_HDR: RwLock<HashMap<vk::SurfaceKHR, bool>> = RwLock::new(HashMap::new());
    static ref GLOBAL_GIPA: RwLock<Option<vk::PFN_vkGetInstanceProcAddr>> = RwLock::new(None);
    static ref GLOBAL_INSTANCE: RwLock<Option<vk::Instance>> = RwLock::new(None);
    static ref HDR_CONFIG: HdrConfig = HdrConfig::from_env();
}

#[derive(Deserialize)] struct KScreenDoctorOutput { outputs: Vec<KScreenOutput> }
#[derive(Deserialize)] struct KScreenOutput { 
    name: String, 
    primary: bool, 
    #[serde(rename = "maxBrightnessOverride")] max_brightness_override: Option<f32>, 
    #[serde(rename = "maxBrightness")] max_brightness: Option<f32>,
    #[serde(rename = "sdrBrightness")] sdr_brightness: Option<f32> 
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)] 
pub enum OutputFormat { 
    #[serde(rename = "pq")] PQ, 
    #[serde(rename = "scrgb")] ScRGB 
}

#[derive(Serialize, Deserialize, Clone)]
struct HdrConfig { 
    pub max_lum: f32, 
    pub mid_lum: f32, 
    pub sat: f32, 
    pub vibrance: f32, 
    pub intensity: f32, 
    pub black_level: f32, 
    pub rcas_strength: f32, 
    pub fxaa_strength: f32, 
    pub sdr_brightness: f32,
    #[serde(skip)]
    pub sdr_gain: f32,
    pub preferred_format: OutputFormat
}

impl HdrConfig {
    /// Attempts to read peak and average luminance from the monitor's EDID via sysfs.
    fn get_edid_luminance(connector: &str) -> (Option<f32>, Option<f32>) {
        let drm_dir = "/sys/class/drm";
        if let Ok(entries) = std::fs::read_dir(drm_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                // Match connector name (e.g., "DP-5" matches directory containing "DP-5")
                if name.contains(connector) && name.contains("card") {
                    let edid_path = entry.path().join("edid");
                    if let Ok(edid) = std::fs::read(edid_path) {
                        if edid.len() >= 128 {
                            let extensions = edid[126] as usize;
                            for ext in 1..=extensions {
                                let block_start = ext * 128;
                                if edid.len() < block_start + 128 { break; }
                                let block = &edid[block_start..block_start + 128];
                                if block[0] == 0x02 { // CEA-861 Extension
                                    let d_start = block[2] as usize;
                                    let mut i = 4;
                                    while i < d_start && i < 127 {
                                        let tag = (block[i] & 0xE0) >> 5;
                                        let len = (block[i] & 0x1F) as usize;
                                        if tag == 0x07 && len >= 3 { // Extended Tag
                                            if block[i+1] == 0x06 { // HDR Static Metadata Block
                                                let mut max_lum = None;
                                                let mut avg_lum = None;
                                                // CEA-861-G: Byte 6 (i+4) is Max Lum, Byte 7 (i+5) is Max Frame-avg
                                                if len >= 4 {
                                                    let v = block[i+4]; 
                                                    if v > 0 { max_lum = Some(50.0 * (v as f32 / 32.0).exp2()); }
                                                }
                                                if len >= 5 {
                                                    let v = block[i+5];
                                                    if v > 0 { avg_lum = Some(50.0 * (v as f32 / 32.0).exp2()); }
                                                }
                                                return (max_lum, avg_lum);
                                            }
                                        }
                                        i += 1 + len;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        (None, None)
    }

    /// Detects system HDR configuration by querying the Desktop Environment or reading EDID.
    fn detect_system_config() -> (Option<f32>, Option<f32>, String) {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
        let mut connector_name = String::new();
        let mut sys_max_lum = None;
        let mut sys_sdr_brightness = None;

        if desktop.contains("kde") {
            if let Ok(output) = std::process::Command::new("kscreen-doctor").arg("-j").output() {
                if let Ok(data) = serde_json::from_slice::<KScreenDoctorOutput>(&output.stdout) {
                    if let Some(primary) = data.outputs.into_iter().find(|o| o.primary) {
                        connector_name = primary.name;
                        sys_max_lum = primary.max_brightness_override.or(primary.max_brightness);
                        sys_sdr_brightness = primary.sdr_brightness;
                    }
                }
            }
        } else if desktop.contains("gnome") {
            let home = std::env::var("HOME").unwrap_or_default();
            let path = format!("{}/.config/monitors.xml", home);
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Some(pos) = content.find("<primary>yes</primary>") {
                    let start = content[..pos].rfind("<logicalmonitor>").unwrap_or(0);
                    let end = content[pos..].find("</logicalmonitor>").map(|e| pos + e).unwrap_or(content.len());
                    let block = &content[start..end];
                    if let Some(c_start) = block.find("<connector>") {
                        if let Some(c_end) = block[c_start..].find("</connector>") {
                            connector_name = block[c_start + 11..c_start + c_end].to_string();
                        }
                    }
                }
            }
        }

        if !connector_name.is_empty() {
            let (edid_max, edid_avg) = Self::get_edid_luminance(&connector_name);
            // System values (DE overrides) take precedence over raw hardware EDID.
            return (sys_max_lum.or(edid_max), sys_sdr_brightness.or(edid_avg), connector_name);
        }
        (None, None, "Unknown".to_string())
    }

    fn from_env() -> Self {
        let (sys_max_lum, sys_mid_lum, monitor_name) = Self::detect_system_config();
        
        // 1. Definiujemy domyślne wartości na podstawie detekcji systemu
        let default_max_lum = sys_max_lum.unwrap_or(1000.0);
        let default_mid_lum = sys_mid_lum.unwrap_or(default_max_lum * 0.3);
        let default_sdr_brightness = sys_mid_lum.unwrap_or(100.0);
        
        let mut config = Self {
            max_lum: default_max_lum,
            mid_lum: default_mid_lum,
            sat: 1.0,
            vibrance: 0.0,
            intensity: 1.0,
            black_level: 0.0,
            rcas_strength: 0.0,
            fxaa_strength: 0.0,
            sdr_brightness: default_sdr_brightness,
            sdr_gain: default_sdr_brightness / 100.0,
            preferred_format: OutputFormat::PQ,
        };

        // 2. Wyznaczamy ścieżki
        let config_dir = dirs::config_dir().map(|p| p.join("autohdr"));
        let global_config_path = config_dir.as_ref().map(|d| d.join("autohdr.conf"));
        let mut proc_config_path = None;
        
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_name) = exe_path.file_name().and_then(|n| n.to_str()) {
                proc_config_path = config_dir.as_ref().map(|d| d.join(format!("{}.conf", exe_name)));
            }
        }

        // Automatyczne tworzenie plików konfiguracyjnych
        if let (Some(global_path), Some(proc_path)) = (&global_config_path, &proc_config_path) {
            // Najpierw upewnij się, że autohdr.conf istnieje
            if !global_path.exists() {
                if let Some(parent) = global_path.parent() { let _ = fs::create_dir_all(parent); }
                if let Ok(toml_str) = toml::to_string_pretty(&config) {
                    let _ = fs::write(global_path, toml_str);
                    eprintln!("[AutoHDR] Created default global config at {:?}", global_path);
                }
            }
            
            // Jeśli AUTOHDR_ENABLE=1 i nie ma pliku procesu, skopiuj autohdr.conf
            if std::env::var("AUTOHDR_ENABLE").ok() == Some("1".to_string()) && !proc_path.exists() {
                if let Ok(_) = fs::copy(global_path, proc_path) {
                    eprintln!("[AutoHDR] Created process-specific config by copying global config to {:?}", proc_path);
                }
            }
        }

        // Wybór pliku do wczytania (priorytety)
        let mut selected_config_path = None;

        // 1. AUTOHDR_CONFIG
        if let Ok(path) = std::env::var("AUTOHDR_CONFIG") {
            selected_config_path = Some(PathBuf::from(path));
        }
        // 2. [process].conf
        if selected_config_path.is_none() {
            if let Some(ref path) = proc_config_path {
                if path.exists() { selected_config_path = Some(path.clone()); }
            }
        }
        // 3. autohdr.conf
        if selected_config_path.is_none() {
            selected_config_path = global_config_path;
        }

        if let Some(path) = selected_config_path {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(loaded) = toml::from_str::<Self>(&content) {
                    config = loaded;
                    eprintln!("[AutoHDR] Loaded config from {:?}", path);
                }
            }
        }

        // 3. Zmienne środowiskowe mają najwyższy priorytet
        if let Some(v) = std::env::var("AUTOHDR_MAX_LUMINANCE").ok().and_then(|v| v.parse().ok()) { config.max_lum = v; }
        if let Some(v) = std::env::var("AUTOHDR_MID_LUMINANCE").ok().and_then(|v| v.parse().ok()) { config.mid_lum = v; }
        if let Some(v) = std::env::var("AUTOHDR_SATURATION").ok().and_then(|v| v.parse().ok()) { config.sat = v; }
        if let Some(v) = std::env::var("AUTOHDR_VIBRANCE").ok().and_then(|v| v.parse().ok()) { config.vibrance = v; }
        if let Some(v) = std::env::var("AUTOHDR_INTENSITY").ok().and_then(|v| v.parse().ok()) { config.intensity = v; }
        if let Some(v) = std::env::var("AUTOHDR_BLACK_LEVEL").ok().and_then(|v| v.parse().ok()) { config.black_level = v; }
        if let Some(v) = std::env::var("AUTOHDR_RCAS").ok().and_then(|v| v.parse().ok()) { config.rcas_strength = v; }
        if let Some(v) = std::env::var("AUTOHDR_FXAA").ok().and_then(|v| v.parse().ok()) { config.fxaa_strength = v; }
        if let Some(v) = std::env::var("AUTOHDR_SDR_BRIGHTNESS").ok().and_then(|v| v.parse().ok()) { config.sdr_brightness = v; }
        
        if let Ok(v) = std::env::var("AUTOHDR_OUTPUT_FORMAT") {
             config.preferred_format = match v.to_lowercase().as_str() {
                "scrgb" => OutputFormat::ScRGB,
                _ => OutputFormat::PQ,
            };
        }

        // Przeliczamy gain po nałożeniu wszystkich ustawień
        config.sdr_gain = config.sdr_brightness / 100.0;

        eprintln!("[AutoHDR] Monitor: {} | Max={} Mid={} Sat={} Vib={} Int={} Black={} RCAS={} FXAA={} SDR={} Format={:?}", 
            monitor_name, config.max_lum, config.mid_lum, config.sat, config.vibrance, config.intensity, config.black_level, config.rcas_strength, config.fxaa_strength, config.sdr_brightness, config.preferred_format);
            
        config
    }
}

#[repr(C)] #[derive(Clone, Copy)]
struct PushConstants { 
    max_lum: f32, mid_lum: f32, sat: f32, vibrance: f32, width: u32, height: u32, 
    rcas_strength: f32, fxaa_strength: f32, intensity: f32, black_level: f32, sdr_gain: f32, output_mode: u32 
}

pub struct DeviceContext {
    pub pd: vk::PhysicalDevice, pub inst: vk::Instance, pub gdpa: vk::PFN_vkGetDeviceProcAddr, pub gipa: vk::PFN_vkGetInstanceProcAddr,
    pub is_nvidia: bool,
    pub create_image: Option<vk::PFN_vkCreateImage>, pub get_image_mem_req: Option<vk::PFN_vkGetImageMemoryRequirements>,
    pub allocate_mem: Option<vk::PFN_vkAllocateMemory>, pub bind_image_mem: Option<vk::PFN_vkBindImageMemory>,
    pub create_image_view: Option<vk::PFN_vkCreateImageView>, pub create_shader_module: Option<vk::PFN_vkCreateShaderModule>,
    pub create_desc_set_layout: Option<vk::PFN_vkCreateDescriptorSetLayout>, pub create_pipe_layout: Option<vk::PFN_vkCreatePipelineLayout>,
    pub create_compute_pipes: Option<vk::PFN_vkCreateComputePipelines>, pub create_desc_pool: Option<vk::PFN_vkCreateDescriptorPool>,
    pub alloc_desc_sets: Option<vk::PFN_vkAllocateDescriptorSets>, pub update_desc_sets: Option<vk::PFN_vkUpdateDescriptorSets>,
    pub create_cmd_pool: Option<vk::PFN_vkCreateCommandPool>, pub alloc_cmd_bufs: Option<vk::PFN_vkAllocateCommandBuffers>,
    pub begin_cmd_buf: Option<vk::PFN_vkBeginCommandBuffer>, pub end_cmd_buf: Option<vk::PFN_vkEndCommandBuffer>,
    pub cmd_pipeline_barrier: Option<vk::PFN_vkCmdPipelineBarrier>, pub cmd_bind_pipe: Option<vk::PFN_vkCmdBindPipeline>,
    pub cmd_bind_desc_sets: Option<vk::PFN_vkCmdBindDescriptorSets>, pub cmd_push_constants: Option<vk::PFN_vkCmdPushConstants>,
    pub cmd_dispatch: Option<vk::PFN_vkCmdDispatch>, pub queue_submit: Option<vk::PFN_vkQueueSubmit>,
    pub queue_wait_idle: Option<vk::PFN_vkQueueWaitIdle>, pub create_sampler: Option<vk::PFN_vkCreateSampler>,
    pub cmd_copy_image: Option<vk::PFN_vkCmdCopyImage>,
    pub real_create_swapchain: Option<vk::PFN_vkCreateSwapchainKHR>, pub real_get_swapchain_images: Option<vk::PFN_vkGetSwapchainImagesKHR>,
    pub real_queue_present: Option<vk::PFN_vkQueuePresentKHR>, pub real_get_device_queue: Option<vk::PFN_vkGetDeviceQueue>,
    pub real_acquire_next_image: Option<vk::PFN_vkAcquireNextImageKHR>,
    pub real_destroy_swapchain: Option<vk::PFN_vkDestroySwapchainKHR>,
    pub real_get_device_queue2: Option<vk::PFN_vkGetDeviceQueue2>,
    pub destroy_image: Option<vk::PFN_vkDestroyImage>,
    pub free_memory: Option<vk::PFN_vkFreeMemory>,
    pub destroy_image_view: Option<vk::PFN_vkDestroyImageView>,
    pub destroy_pipeline: Option<vk::PFN_vkDestroyPipeline>,
    pub destroy_pipe_layout: Option<vk::PFN_vkDestroyPipelineLayout>,
    pub destroy_desc_set_layout: Option<vk::PFN_vkDestroyDescriptorSetLayout>,
    pub destroy_desc_pool: Option<vk::PFN_vkDestroyDescriptorPool>,
    pub destroy_cmd_pool: Option<vk::PFN_vkDestroyCommandPool>,
    pub destroy_sampler: Option<vk::PFN_vkDestroySampler>,
    pub create_semaphore: Option<vk::PFN_vkCreateSemaphore>,
    pub destroy_semaphore: Option<vk::PFN_vkDestroySemaphore>,
}

pub struct SwapchainState {
    pub width: u32, pub height: u32, pub sdr_format: vk::Format,
    pub proxy_images: Vec<vk::Image>, pub proxy_mems: Vec<vk::DeviceMemory>, pub proxy_views: Vec<vk::ImageView>,
    pub work_images: Vec<vk::Image>, pub work_mems: Vec<vk::DeviceMemory>, pub work_views: Vec<vk::ImageView>,
    pub real_images: Vec<vk::Image>,
    pub pipe: vk::Pipeline, pub pipe_layout: vk::PipelineLayout, pub desc_layout: vk::DescriptorSetLayout,
    pub desc_pool: vk::DescriptorPool, pub desc_sets: Vec<vk::DescriptorSet>,
    pub cmd_pool: vk::CommandPool, pub cmd_bufs: Vec<vk::CommandBuffer>,
    pub sampler: vk::Sampler,
    pub present_semaphores: Vec<vk::Semaphore>,
    pub bypass: bool,
    pub output_mode: u32, // 0 = PQ, 1 = scRGB
}

#[no_mangle] pub unsafe extern "system" fn vkNegotiateLoaderLayerInterfaceVersion(p_vs: *mut NegotiateLayerInterface) -> vk::Result { (*p_vs).pfn_get_instance_proc_addr = Some(hook_get_instance_proc_addr); (*p_vs).pfn_get_device_proc_addr = Some(hook_get_device_proc_addr); vk::Result::SUCCESS }

unsafe extern "system" fn hook_enumerate_physical_devices(inst: vk::Instance, p_pdc: *mut u32, p_pd: *mut vk::PhysicalDevice) -> vk::Result {
    let gipa = match INSTANCE_GIPA.read().unwrap().get(&inst).copied().or_else(|| *GLOBAL_GIPA.read().unwrap()) {
        Some(f) => f,
        None => return vk::Result::ERROR_INITIALIZATION_FAILED,
    };
    let f: vk::PFN_vkEnumeratePhysicalDevices = match gipa(inst, b"vkEnumeratePhysicalDevices\0".as_ptr() as *const c_char) {
        Some(ptr) => std::mem::transmute(ptr),
        None => return vk::Result::ERROR_INITIALIZATION_FAILED,
    };
    let res = (f)(inst, p_pdc, p_pd);
    if (res == vk::Result::SUCCESS || res == vk::Result::INCOMPLETE) && !p_pd.is_null() {
        let mut map = PHYSICAL_DEVICE_TO_INSTANCE.write().unwrap();
        for i in 0..*p_pdc as usize { map.insert(*p_pd.add(i), inst); }
    }
    res
}

unsafe extern "system" fn hook_get_instance_proc_addr(inst: vk::Instance, p_name: *const c_char) -> vk::PFN_vkVoidFunction {
    let name = CStr::from_ptr(p_name).to_str().unwrap_or("");
    match name {
        "vkGetInstanceProcAddr" => Some(std::mem::transmute(hook_get_instance_proc_addr as *const ())),
        "vkGetDeviceProcAddr" => Some(std::mem::transmute(hook_get_device_proc_addr as *const ())),
        "vkCreateInstance" => Some(std::mem::transmute(hook_create_instance as *const ())),
        "vkCreateDevice" => Some(std::mem::transmute(hook_create_device as *const ())),
        "vkEnumeratePhysicalDevices" => Some(std::mem::transmute(hook_enumerate_physical_devices as *const ())),
        "vkGetPhysicalDeviceSurfaceFormatsKHR" => Some(std::mem::transmute(hook_get_pd_surface_formats as *const ())),
        "vkGetPhysicalDeviceSurfaceFormats2KHR" => Some(std::mem::transmute(hook_get_pd_surface_formats2 as *const ())),
        "vkGetPhysicalDeviceSurfaceCapabilitiesKHR" => Some(std::mem::transmute(hook_get_pd_surface_caps as *const ())),
        "vkGetPhysicalDeviceSurfaceCapabilities2KHR" => Some(std::mem::transmute(hook_get_pd_surface_caps2 as *const ())),
        "vkCreateSwapchainKHR" => Some(std::mem::transmute(hook_create_swapchain_khr as *const ())),
        "vkDestroySwapchainKHR" => Some(std::mem::transmute(hook_destroy_swapchain_khr as *const ())),
        "vkGetSwapchainImagesKHR" => Some(std::mem::transmute(hook_get_swapchain_images_khr as *const ())),
        "vkQueuePresentKHR" => Some(std::mem::transmute(hook_queue_present_khr as *const ())),
        "vkAcquireNextImageKHR" => Some(std::mem::transmute(hook_acquire_next_image_khr as *const ())),
        "vkGetDeviceQueue" => Some(std::mem::transmute(hook_get_device_queue as *const ())),
        "vkGetDeviceQueue2" => Some(std::mem::transmute(hook_get_device_queue2 as *const ())),
        _ => { if inst == vk::Instance::null() { None } else { INSTANCE_GIPA.read().unwrap().get(&inst).and_then(|f| f(inst, p_name)) } }
    }
}

unsafe extern "system" fn hook_get_device_proc_addr(dev: vk::Device, p_name: *const c_char) -> vk::PFN_vkVoidFunction {
    let name = CStr::from_ptr(p_name).to_str().unwrap_or("");
    match name {
        "vkGetDeviceProcAddr" => Some(std::mem::transmute(hook_get_device_proc_addr as *const ())),
        "vkCreateSwapchainKHR" => Some(std::mem::transmute(hook_create_swapchain_khr as *const ())),
        "vkDestroySwapchainKHR" => Some(std::mem::transmute(hook_destroy_swapchain_khr as *const ())),
        "vkGetSwapchainImagesKHR" => Some(std::mem::transmute(hook_get_swapchain_images_khr as *const ())),
        "vkQueuePresentKHR" => Some(std::mem::transmute(hook_queue_present_khr as *const ())),
        "vkAcquireNextImageKHR" => Some(std::mem::transmute(hook_acquire_next_image_khr as *const ())),
        "vkGetDeviceQueue" => Some(std::mem::transmute(hook_get_device_queue as *const ())),
        "vkGetDeviceQueue2" => Some(std::mem::transmute(hook_get_device_queue2 as *const ())),
        _ => { DEVICE_GDPA.read().unwrap().get(&dev).and_then(|f| f(dev, p_name)) }
    }
}

unsafe extern "system" fn hook_get_device_queue(dev: vk::Device, qfi: u32, qi: u32, p_q: *mut vk::Queue) {
    if let Some(c) = DEVICE_CONTEXTS.read().unwrap().get(&dev) {
        if let Some(real_gdq) = c.real_get_device_queue {
            (real_gdq)(dev, qfi, qi, p_q);
            if !p_q.is_null() && *p_q != vk::Queue::null() { QUEUE_TO_DEVICE.write().unwrap().insert(*p_q, (dev, qfi)); }
        }
    }
}

unsafe extern "system" fn hook_get_device_queue2(dev: vk::Device, p_info: *const vk::DeviceQueueInfo2, p_q: *mut vk::Queue) {
    if let Some(c) = DEVICE_CONTEXTS.read().unwrap().get(&dev) {
        if let Some(real_gdq2) = c.real_get_device_queue2 {
            (real_gdq2)(dev, p_info, p_q);
            if !p_q.is_null() && !p_info.is_null() && *p_q != vk::Queue::null() { 
                QUEUE_TO_DEVICE.write().unwrap().insert(*p_q, (dev, (*p_info).queue_family_index)); 
            }
        } else if let Some(real_gdq) = c.real_get_device_queue {
            if !p_info.is_null() {
                (real_gdq)(dev, (*p_info).queue_family_index, (*p_info).queue_index, p_q);
                if !p_q.is_null() && *p_q != vk::Queue::null() { 
                    QUEUE_TO_DEVICE.write().unwrap().insert(*p_q, (dev, (*p_info).queue_family_index)); 
                }
            }
        }
    }
}

unsafe extern "system" fn hook_get_pd_surface_formats(pd: vk::PhysicalDevice, surface: vk::SurfaceKHR, p_fc: *mut u32, p_formats: *mut vk::SurfaceFormatKHR) -> vk::Result {
    let inst = PHYSICAL_DEVICE_TO_INSTANCE.read().unwrap().get(&pd).copied().or_else(|| *GLOBAL_INSTANCE.read().unwrap()).unwrap_or(vk::Instance::null());
    let next_gipa = match INSTANCE_GIPA.read().unwrap().get(&inst).copied().or_else(|| *GLOBAL_GIPA.read().unwrap()) {
        Some(f) => f,
        None => return vk::Result::ERROR_INITIALIZATION_FAILED,
    };
    let real_f: vk::PFN_vkGetPhysicalDeviceSurfaceFormatsKHR = std::mem::transmute(next_gipa(inst, b"vkGetPhysicalDeviceSurfaceFormatsKHR\0".as_ptr() as *const c_char).expect("Failed to locate real vkGetPhysicalDeviceSurfaceFormatsKHR"));
    let mut count = 0;
    let mut res = (real_f)(pd, surface, &mut count, std::ptr::null_mut());
    if res != vk::Result::SUCCESS { return res; }
    let mut formats = vec![vk::SurfaceFormatKHR::default(); count as usize];
    res = (real_f)(pd, surface, &mut count, formats.as_mut_ptr());
    if res != vk::Result::SUCCESS && res != vk::Result::INCOMPLETE { return res; }
    
    let has_pq = formats.iter().any(|f| f.color_space == vk::ColorSpaceKHR::HDR10_ST2084_EXT);
    let has_scrgb = formats.iter().any(|f| f.color_space == vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT);
    SURFACE_NATIVE_HDR.write().unwrap().insert(surface, has_pq || has_scrgb);
    
    if !has_pq {
        formats.push(vk::SurfaceFormatKHR { format: vk::Format::A2B10G10R10_UNORM_PACK32, color_space: vk::ColorSpaceKHR::HDR10_ST2084_EXT });
    }
    if !has_scrgb {
        formats.push(vk::SurfaceFormatKHR { format: vk::Format::R16G16B16A16_SFLOAT, color_space: vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT });
    }

    if p_formats.is_null() { *p_fc = formats.len() as u32; return vk::Result::SUCCESS; }
    let input_count = *p_fc as usize;
    *p_fc = formats.len() as u32;
    let copy_count = std::cmp::min(input_count, formats.len());
    std::ptr::copy_nonoverlapping(formats.as_ptr(), p_formats, copy_count);
    if input_count < formats.len() { vk::Result::INCOMPLETE } else { vk::Result::SUCCESS }
}

unsafe extern "system" fn hook_get_pd_surface_formats2(pd: vk::PhysicalDevice, p_info: *const vk::PhysicalDeviceSurfaceInfo2KHR, p_fc: *mut u32, p_formats: *mut vk::SurfaceFormat2KHR) -> vk::Result {
    let inst = PHYSICAL_DEVICE_TO_INSTANCE.read().unwrap().get(&pd).copied().or_else(|| *GLOBAL_INSTANCE.read().unwrap()).unwrap_or(vk::Instance::null());
    let next_gipa = match INSTANCE_GIPA.read().unwrap().get(&inst).copied().or_else(|| *GLOBAL_GIPA.read().unwrap()) {
        Some(f) => f,
        None => return vk::Result::ERROR_INITIALIZATION_FAILED,
    };
    let real_f: vk::PFN_vkGetPhysicalDeviceSurfaceFormats2KHR = std::mem::transmute(next_gipa(inst, b"vkGetPhysicalDeviceSurfaceFormats2KHR\0".as_ptr() as *const c_char).expect("Failed to locate real vkGetPhysicalDeviceSurfaceFormats2KHR"));
    let mut count = 0;
    let mut res = (real_f)(pd, p_info, &mut count, std::ptr::null_mut());
    if res != vk::Result::SUCCESS { return res; }
    let mut formats = vec![vk::SurfaceFormat2KHR { s_type: vk::StructureType::SURFACE_FORMAT_2_KHR, p_next: std::ptr::null_mut(), surface_format: vk::SurfaceFormatKHR::default() }; count as usize];
    res = (real_f)(pd, p_info, &mut count, formats.as_mut_ptr());
    if res != vk::Result::SUCCESS && res != vk::Result::INCOMPLETE { return res; }
    
    let has_pq = formats.iter().any(|f| f.surface_format.color_space == vk::ColorSpaceKHR::HDR10_ST2084_EXT);
    let has_scrgb = formats.iter().any(|f| f.surface_format.color_space == vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT);
    SURFACE_NATIVE_HDR.write().unwrap().insert((*p_info).surface, has_pq || has_scrgb);

    if !has_pq {
        formats.push(vk::SurfaceFormat2KHR { s_type: vk::StructureType::SURFACE_FORMAT_2_KHR, p_next: std::ptr::null_mut(), surface_format: vk::SurfaceFormatKHR { format: vk::Format::A2B10G10R10_UNORM_PACK32, color_space: vk::ColorSpaceKHR::HDR10_ST2084_EXT } });
    }
    if !has_scrgb {
        formats.push(vk::SurfaceFormat2KHR { s_type: vk::StructureType::SURFACE_FORMAT_2_KHR, p_next: std::ptr::null_mut(), surface_format: vk::SurfaceFormatKHR { format: vk::Format::R16G16B16A16_SFLOAT, color_space: vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT } });
    }

    if p_formats.is_null() { *p_fc = formats.len() as u32; return vk::Result::SUCCESS; }
    let input_count = *p_fc as usize;
    *p_fc = formats.len() as u32;
    let copy_count = std::cmp::min(input_count, formats.len());
    std::ptr::copy_nonoverlapping(formats.as_ptr(), p_formats, copy_count);
    if input_count < formats.len() { vk::Result::INCOMPLETE } else { vk::Result::SUCCESS }
}

unsafe extern "system" fn hook_get_pd_surface_caps(pd: vk::PhysicalDevice, surface: vk::SurfaceKHR, p_caps: *mut vk::SurfaceCapabilitiesKHR) -> vk::Result {
    let inst = PHYSICAL_DEVICE_TO_INSTANCE.read().unwrap().get(&pd).copied().or_else(|| *GLOBAL_INSTANCE.read().unwrap()).unwrap_or(vk::Instance::null());
    let next_gipa = INSTANCE_GIPA.read().unwrap().get(&inst).copied().or_else(|| *GLOBAL_GIPA.read().unwrap());
    let next_gipa = match next_gipa {
        Some(f) => f,
        None => return vk::Result::ERROR_INITIALIZATION_FAILED,
    };
    if let Some(f) = next_gipa(inst, b"vkGetPhysicalDeviceSurfaceCapabilitiesKHR\0".as_ptr() as *const c_char) {
        let pfn: vk::PFN_vkGetPhysicalDeviceSurfaceCapabilitiesKHR = std::mem::transmute(f);
        let res = (pfn)(pd, surface, p_caps);
        if res == vk::Result::SUCCESS { unsafe { (*p_caps).supported_usage_flags |= vk::ImageUsageFlags::TRANSFER_DST; } }
        return res;
    }
    vk::Result::ERROR_INITIALIZATION_FAILED
}

unsafe extern "system" fn hook_get_pd_surface_caps2(pd: vk::PhysicalDevice, p_info: *const vk::PhysicalDeviceSurfaceInfo2KHR, p_caps: *mut vk::SurfaceCapabilities2KHR) -> vk::Result {
    let inst = PHYSICAL_DEVICE_TO_INSTANCE.read().unwrap().get(&pd).copied().or_else(|| *GLOBAL_INSTANCE.read().unwrap()).unwrap_or(vk::Instance::null());
    let next_gipa = INSTANCE_GIPA.read().unwrap().get(&inst).copied().or_else(|| *GLOBAL_GIPA.read().unwrap());
    let next_gipa = match next_gipa {
        Some(f) => f,
        None => return vk::Result::ERROR_INITIALIZATION_FAILED,
    };
    if let Some(f) = next_gipa(inst, b"vkGetPhysicalDeviceSurfaceCapabilities2KHR\0".as_ptr() as *const c_char) {
        let pfn: vk::PFN_vkGetPhysicalDeviceSurfaceCapabilities2KHR = std::mem::transmute(f);
        let res = (pfn)(pd, p_info, p_caps);
        if res == vk::Result::SUCCESS { unsafe { (*p_caps).surface_capabilities.supported_usage_flags |= vk::ImageUsageFlags::TRANSFER_DST; } }
        return res;
    }
    vk::Result::ERROR_INITIALIZATION_FAILED
}

unsafe extern "system" fn hook_create_instance(p_ci: *const vk::InstanceCreateInfo, p_al: *const vk::AllocationCallbacks, p_inst: *mut vk::Instance) -> vk::Result {
    lazy_static::initialize(&HDR_CONFIG);
    let mut exts: Vec<*const c_char> = if (*p_ci).enabled_extension_count > 0 {
        std::slice::from_raw_parts((*p_ci).pp_enabled_extension_names, (*p_ci).enabled_extension_count as usize).to_vec()
    } else {
        Vec::new()
    };
    let ext_names = [b"VK_KHR_get_physical_device_properties2\0".as_ptr() as *const c_char, b"VK_EXT_swapchain_colorspace\0".as_ptr() as *const c_char];
    for &name_ptr in &ext_names {
        let name = CStr::from_ptr(name_ptr);
        if !exts.iter().any(|&e| CStr::from_ptr(e) == name) { exts.push(name_ptr); }
    }
    let mut ci = *p_ci;
    ci.enabled_extension_count = exts.len() as u32;
    ci.pp_enabled_extension_names = exts.as_ptr();

    let mut li = ci.p_next as *mut VkLayerInstanceCreateInfo;
    while !li.is_null() {
        if (*li).s_type == vk::StructureType::from_raw(47) && (*li).function == 0 {
            let next_gipa = (*(*li).p_layer_info).pfn_next_get_instance_proc_addr;
            (*li).p_layer_info = (*(*li).p_layer_info).p_next;
            if let Some(f) = next_gipa(vk::Instance::null(), b"vkCreateInstance\0".as_ptr() as *const c_char) {
                let res = (std::mem::transmute::<_, vk::PFN_vkCreateInstance>(f))(&ci, p_al, p_inst);
                if res == vk::Result::SUCCESS { 
                    INSTANCE_GIPA.write().unwrap().insert(*p_inst, next_gipa); 
                    GLOBAL_INSTANCE.write().unwrap().replace(*p_inst); 
                    GLOBAL_GIPA.write().unwrap().replace(next_gipa); 
                }
                return res;
            }
        }
        li = (*li).p_next as *mut VkLayerInstanceCreateInfo;
    }
    vk::Result::ERROR_INITIALIZATION_FAILED
}

unsafe extern "system" fn hook_create_device(pd: vk::PhysicalDevice, p_ci: *const vk::DeviceCreateInfo, p_al: *const vk::AllocationCallbacks, p_dev: *mut vk::Device) -> vk::Result {
    let mut exts: Vec<*const c_char> = if (*p_ci).enabled_extension_count > 0 {
        std::slice::from_raw_parts((*p_ci).pp_enabled_extension_names, (*p_ci).enabled_extension_count as usize).to_vec()
    } else {
        Vec::new()
    };
    let ext_names = [b"VK_EXT_hdr_metadata\0".as_ptr() as *const c_char, b"VK_KHR_swapchain\0".as_ptr() as *const c_char];
    for &name_ptr in &ext_names {
        let name = CStr::from_ptr(name_ptr);
        if !exts.iter().any(|&e| CStr::from_ptr(e) == name) { exts.push(name_ptr); }
    }
    let mut ci = *p_ci;
    ci.enabled_extension_count = exts.len() as u32;
    ci.pp_enabled_extension_names = exts.as_ptr();

    let mut li = ci.p_next as *mut VkLayerDeviceCreateInfo;
    while !li.is_null() {
        if (*li).s_type == vk::StructureType::from_raw(48) && (*li).function == 0 {
            let next_gipa = (*(*li).p_layer_info).pfn_next_get_instance_proc_addr;
            let next_gdpa = (*(*li).p_layer_info).pfn_next_get_device_proc_addr;
            (*li).p_layer_info = (*(*li).p_layer_info).p_next;
            let inst = PHYSICAL_DEVICE_TO_INSTANCE.read().unwrap().get(&pd).copied().or_else(|| *GLOBAL_INSTANCE.read().unwrap()).unwrap_or(vk::Instance::null());
            
            let mut props = vk::PhysicalDeviceProperties::default();
            if let Some(f_props) = next_gipa(inst, b"vkGetPhysicalDeviceProperties\0".as_ptr() as *const c_char) {
                let pfn_props: vk::PFN_vkGetPhysicalDeviceProperties = std::mem::transmute(f_props);
                (pfn_props)(pd, &mut props);
            }
            let is_nvidia = props.vendor_id == 0x10DE;

            if let Some(f) = next_gipa(inst, b"vkCreateDevice\0".as_ptr() as *const c_char) {
                let res = (std::mem::transmute::<_, vk::PFN_vkCreateDevice>(f))(pd, &ci, p_al, p_dev);
                if res == vk::Result::SUCCESS {
                    DEVICE_GDPA.write().unwrap().insert(*p_dev, next_gdpa);
                    let f_dev = |n: &[u8]| next_gdpa(*p_dev, n.as_ptr() as *const c_char).or_else(|| next_gipa(inst, n.as_ptr() as *const c_char));
                    DEVICE_CONTEXTS.write().unwrap().insert(*p_dev, DeviceContext {
                        pd, inst, gdpa: next_gdpa, gipa: next_gipa,
                        is_nvidia,
                        create_image: f_dev(b"vkCreateImage\0").map(|p| std::mem::transmute(p)),
                        get_image_mem_req: f_dev(b"vkGetImageMemoryRequirements\0").map(|p| std::mem::transmute(p)),
                        allocate_mem: f_dev(b"vkAllocateMemory\0").map(|p| std::mem::transmute(p)),
                        bind_image_mem: f_dev(b"vkBindImageMemory\0").map(|p| std::mem::transmute(p)),
                        create_image_view: f_dev(b"vkCreateImageView\0").map(|p| std::mem::transmute(p)),
                        create_shader_module: f_dev(b"vkCreateShaderModule\0").map(|p| std::mem::transmute(p)),
                        create_desc_set_layout: f_dev(b"vkCreateDescriptorSetLayout\0").map(|p| std::mem::transmute(p)),
                        create_pipe_layout: f_dev(b"vkCreatePipelineLayout\0").map(|p| std::mem::transmute(p)),
                        create_compute_pipes: f_dev(b"vkCreateComputePipelines\0").map(|p| std::mem::transmute(p)),
                        create_desc_pool: f_dev(b"vkCreateDescriptorPool\0").map(|p| std::mem::transmute(p)),
                        alloc_desc_sets: f_dev(b"vkAllocateDescriptorSets\0").map(|p| std::mem::transmute(p)),
                        update_desc_sets: f_dev(b"vkUpdateDescriptorSets\0").map(|p| std::mem::transmute(p)),
                        create_cmd_pool: f_dev(b"vkCreateCommandPool\0").map(|p| std::mem::transmute(p)),
                        alloc_cmd_bufs: f_dev(b"vkAllocateCommandBuffers\0").map(|p| std::mem::transmute(p)),
                        begin_cmd_buf: f_dev(b"vkBeginCommandBuffer\0").map(|p| std::mem::transmute(p)),
                        end_cmd_buf: f_dev(b"vkEndCommandBuffer\0").map(|p| std::mem::transmute(p)),
                        cmd_pipeline_barrier: f_dev(b"vkCmdPipelineBarrier\0").map(|p| std::mem::transmute(p)),
                        cmd_bind_pipe: f_dev(b"vkCmdBindPipeline\0").map(|p| std::mem::transmute(p)),
                        cmd_bind_desc_sets: f_dev(b"vkCmdBindDescriptorSets\0").map(|p| std::mem::transmute(p)),
                        cmd_push_constants: f_dev(b"vkCmdPushConstants\0").map(|p| std::mem::transmute(p)),
                        cmd_dispatch: f_dev(b"vkCmdDispatch\0").map(|p| std::mem::transmute(p)),
                        queue_submit: f_dev(b"vkQueueSubmit\0").map(|p| std::mem::transmute(p)),
                        queue_wait_idle: f_dev(b"vkQueueWaitIdle\0").map(|p| std::mem::transmute(p)),
                        create_sampler: f_dev(b"vkCreateSampler\0").map(|p| std::mem::transmute(p)),
                        cmd_copy_image: f_dev(b"vkCmdCopyImage\0").map(|p| std::mem::transmute(p)),
                        real_create_swapchain: f_dev(b"vkCreateSwapchainKHR\0").map(|p| std::mem::transmute(p)),
                        real_get_swapchain_images: f_dev(b"vkGetSwapchainImagesKHR\0").map(|p| std::mem::transmute(p)),
                        real_queue_present: f_dev(b"vkQueuePresentKHR\0").map(|p| std::mem::transmute(p)),
                        real_get_device_queue: f_dev(b"vkGetDeviceQueue\0").map(|p| std::mem::transmute(p)),
                        real_acquire_next_image: f_dev(b"vkAcquireNextImageKHR\0").map(|p| std::mem::transmute(p)),
                        real_destroy_swapchain: f_dev(b"vkDestroySwapchainKHR\0").map(|p| std::mem::transmute(p)),
                        real_get_device_queue2: f_dev(b"vkGetDeviceQueue2\0").map(|p| std::mem::transmute(p)),
                        destroy_image: f_dev(b"vkDestroyImage\0").map(|p| std::mem::transmute(p)),
                        free_memory: f_dev(b"vkFreeMemory\0").map(|p| std::mem::transmute(p)),
                        destroy_image_view: f_dev(b"vkDestroyImageView\0").map(|p| std::mem::transmute(p)),
                        destroy_pipeline: f_dev(b"vkDestroyPipeline\0").map(|p| std::mem::transmute(p)),
                        destroy_pipe_layout: f_dev(b"vkDestroyPipelineLayout\0").map(|p| std::mem::transmute(p)),
                        destroy_desc_set_layout: f_dev(b"vkDestroyDescriptorSetLayout\0").map(|p| std::mem::transmute(p)),
                        destroy_desc_pool: f_dev(b"vkDestroyDescriptorPool\0").map(|p| std::mem::transmute(p)),
                        destroy_cmd_pool: f_dev(b"vkDestroyCommandPool\0").map(|p| std::mem::transmute(p)),
                        destroy_sampler: f_dev(b"vkDestroySampler\0").map(|p| std::mem::transmute(p)),
                        create_semaphore: f_dev(b"vkCreateSemaphore\0").map(|p| std::mem::transmute(p)),
                        destroy_semaphore: f_dev(b"vkDestroySemaphore\0").map(|p| std::mem::transmute(p)),
                    });
                }
                return res;
            }
        }
        li = (*li).p_next as *mut VkLayerDeviceCreateInfo;
    }
    vk::Result::ERROR_INITIALIZATION_FAILED
}

unsafe extern "system" fn hook_create_swapchain_khr(dev: vk::Device, p_ci: *const vk::SwapchainCreateInfoKHR, p_al: *const vk::AllocationCallbacks, p_sc: *mut vk::SwapchainKHR) -> vk::Result {
    let ctx_map = DEVICE_CONTEXTS.read().unwrap();
    let c = match ctx_map.get(&dev) { Some(v) => v, None => return vk::Result::ERROR_INITIALIZATION_FAILED };
    let real_f = match c.real_create_swapchain { 
        Some(f) => f, 
        None => match (c.gdpa)(dev, b"vkCreateSwapchainKHR\0".as_ptr() as *const c_char) {
            Some(ptr) => std::mem::transmute::<_, vk::PFN_vkCreateSwapchainKHR>(ptr),
            None => return vk::Result::ERROR_INITIALIZATION_FAILED,
        }
    };
    
    let inst = match *GLOBAL_INSTANCE.read().unwrap() { Some(i) => i, None => return vk::Result::ERROR_INITIALIZATION_FAILED };
    let real_get_formats: vk::PFN_vkGetPhysicalDeviceSurfaceFormatsKHR = std::mem::transmute((c.gipa)(inst, b"vkGetPhysicalDeviceSurfaceFormatsKHR\0".as_ptr() as *const c_char).expect("Failed to locate real vkGetPhysicalDeviceSurfaceFormatsKHR"));
    let mut count = 0;
    let _ = (real_get_formats)(c.pd, (*p_ci).surface, &mut count, std::ptr::null_mut());
    let mut formats = vec![vk::SurfaceFormatKHR::default(); count as usize];
    let _ = (real_get_formats)(c.pd, (*p_ci).surface, &mut count, formats.as_mut_ptr());

    let mut requested_format = HDR_CONFIG.preferred_format;
    let mut output_mode = if requested_format == OutputFormat::ScRGB { 1 } else { 0 };
    
    let mut bypass = false;
    if (*p_ci).image_color_space == vk::ColorSpaceKHR::HDR10_ST2084_EXT || (*p_ci).image_color_space == vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT {
        bypass = true;
    }

    let mut mi = *p_ci;
    let sdr_format = mi.image_format;
    if !bypass {
        if requested_format == OutputFormat::ScRGB {
            if formats.iter().any(|f| f.color_space == vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT) {
                mi.image_format = vk::Format::R16G16B16A16_SFLOAT;
                mi.image_color_space = vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT;
            } else {
                eprintln!("[AutoHDR] Info: scRGB not supported by DE/Monitor. Falling back to PQ.");
                requested_format = OutputFormat::PQ;
                output_mode = 0;
            }
        }
        
        if requested_format == OutputFormat::PQ {
            mi.image_format = vk::Format::A2B10G10R10_UNORM_PACK32;
            mi.image_color_space = vk::ColorSpaceKHR::HDR10_ST2084_EXT;
        }
        mi.image_usage |= vk::ImageUsageFlags::TRANSFER_DST;
    }
    
    let mut res = (real_f)(dev, &mi, p_al, p_sc);
    if res != vk::Result::SUCCESS && !bypass {
        eprintln!("[AutoHDR] Error: HDR Swapchain creation failed, retrying with original SDR format: {:?}", res);
        res = (real_f)(dev, p_ci, p_al, p_sc);
        if res == vk::Result::SUCCESS {
            bypass = true;
        }
    }

    if res != vk::Result::SUCCESS {
        return res;
    }

    if bypass {
        SWAPCHAIN_STATES.write().unwrap().insert(*p_sc, SwapchainState { 
            width: mi.image_extent.width, height: mi.image_extent.height, sdr_format, 
            proxy_images: Vec::new(), proxy_mems: Vec::new(), proxy_views: Vec::new(), 
            work_images: Vec::new(), work_mems: Vec::new(), work_views: Vec::new(), 
            real_images: Vec::new(), pipe: vk::Pipeline::null(), pipe_layout: vk::PipelineLayout::null(), 
            desc_layout: vk::DescriptorSetLayout::null(), desc_pool: vk::DescriptorPool::null(), 
            desc_sets: Vec::new(), cmd_pool: vk::CommandPool::null(), cmd_bufs: Vec::new(), 
            sampler: vk::Sampler::null(), present_semaphores: Vec::new(), bypass: true,
            output_mode
        });
        return res;
    }
    
    if let (Some(csm), Some(cdsl), Some(cpl), Some(ccp), Some(csamp)) = (c.create_shader_module, c.create_desc_set_layout, c.create_pipe_layout, c.create_compute_pipes, c.create_sampler) {
        let mut sm = vk::ShaderModule::null(); let _ = (csm)(dev, &vk::ShaderModuleCreateInfo { s_type: vk::StructureType::SHADER_MODULE_CREATE_INFO, p_next: std::ptr::null(), flags: vk::ShaderModuleCreateFlags::empty(), code_size: SHADER_CODE.len() * 4, p_code: SHADER_CODE.as_ptr() }, std::ptr::null(), &mut sm);
        let bds = [vk::DescriptorSetLayoutBinding { binding: 0, descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER, descriptor_count: 1, stage_flags: vk::ShaderStageFlags::COMPUTE, p_immutable_samplers: std::ptr::null() },
                   vk::DescriptorSetLayoutBinding { binding: 1, descriptor_type: vk::DescriptorType::STORAGE_IMAGE, descriptor_count: 1, stage_flags: vk::ShaderStageFlags::COMPUTE, p_immutable_samplers: std::ptr::null() }];
        let mut dsl = vk::DescriptorSetLayout::null(); let _ = (cdsl)(dev, &vk::DescriptorSetLayoutCreateInfo { s_type: vk::StructureType::DESCRIPTOR_SET_LAYOUT_CREATE_INFO, p_next: std::ptr::null(), flags: vk::DescriptorSetLayoutCreateFlags::empty(), binding_count: 2, p_bindings: bds.as_ptr() }, std::ptr::null(), &mut dsl);
        let mut pl = vk::PipelineLayout::null(); let _ = (cpl)(dev, &vk::PipelineLayoutCreateInfo { s_type: vk::StructureType::PIPELINE_LAYOUT_CREATE_INFO, p_next: std::ptr::null(), flags: vk::PipelineLayoutCreateFlags::empty(), set_layout_count: 1, p_set_layouts: &dsl, push_constant_range_count: 1, p_push_constant_ranges: &vk::PushConstantRange { stage_flags: vk::ShaderStageFlags::COMPUTE, offset: 0, size: 44 } }, std::ptr::null(), &mut pl);
        let stage = vk::PipelineShaderStageCreateInfo { s_type: vk::StructureType::PIPELINE_SHADER_STAGE_CREATE_INFO, p_next: std::ptr::null(), flags: vk::PipelineShaderStageCreateFlags::empty(), stage: vk::ShaderStageFlags::COMPUTE, module: sm, p_name: b"main\0".as_ptr() as *const c_char, p_specialization_info: std::ptr::null() };
        let mut pipe = vk::Pipeline::null(); let _ = (ccp)(dev, vk::PipelineCache::null(), 1, &vk::ComputePipelineCreateInfo { s_type: vk::StructureType::COMPUTE_PIPELINE_CREATE_INFO, p_next: std::ptr::null(), flags: vk::PipelineCreateFlags::empty(), stage, layout: pl, base_pipeline_handle: vk::Pipeline::null(), base_pipeline_index: -1 }, std::ptr::null(), &mut pipe);
        let mut sampler = vk::Sampler::null(); let _ = (csamp)(dev, &vk::SamplerCreateInfo { s_type: vk::StructureType::SAMPLER_CREATE_INFO, p_next: std::ptr::null(), flags: vk::SamplerCreateFlags::empty(), mag_filter: vk::Filter::LINEAR, min_filter: vk::Filter::LINEAR, mipmap_mode: vk::SamplerMipmapMode::LINEAR, address_mode_u: vk::SamplerAddressMode::CLAMP_TO_EDGE, address_mode_v: vk::SamplerAddressMode::CLAMP_TO_EDGE, address_mode_w: vk::SamplerAddressMode::CLAMP_TO_EDGE, mip_lod_bias: 0.0, anisotropy_enable: vk::FALSE, max_anisotropy: 1.0, compare_enable: vk::FALSE, compare_op: vk::CompareOp::ALWAYS, min_lod: 0.0, max_lod: 0.0, border_color: vk::BorderColor::FLOAT_TRANSPARENT_BLACK, unnormalized_coordinates: vk::FALSE }, std::ptr::null(), &mut sampler);
        if let Some(f) = (c.gdpa)(dev, b"vkDestroyShaderModule\0".as_ptr() as *const c_char) {
            let dsm: vk::PFN_vkDestroyShaderModule = std::mem::transmute(f);
            (dsm)(dev, sm, std::ptr::null());
        }
        SWAPCHAIN_STATES.write().unwrap().insert(*p_sc, SwapchainState { width: mi.image_extent.width, height: mi.image_extent.height, sdr_format, proxy_images: Vec::new(), proxy_mems: Vec::new(), proxy_views: Vec::new(), work_images: Vec::new(), work_mems: Vec::new(), work_views: Vec::new(), real_images: Vec::new(), pipe, pipe_layout: pl, desc_layout: dsl, desc_pool: vk::DescriptorPool::null(), desc_sets: Vec::new(), cmd_pool: vk::CommandPool::null(), cmd_bufs: Vec::new(), sampler, present_semaphores: Vec::new(), bypass: false, output_mode });
    }
    res
}

unsafe extern "system" fn hook_destroy_swapchain_khr(dev: vk::Device, sc: vk::SwapchainKHR, p_al: *const vk::AllocationCallbacks) {
    let ctx_map = DEVICE_CONTEXTS.read().unwrap();
    if let Some(c) = ctx_map.get(&dev) {
        if let Some(st) = SWAPCHAIN_STATES.write().unwrap().remove(&sc) {
            if let Some(f) = c.destroy_sampler { (f)(dev, st.sampler, std::ptr::null()); }
            if let Some(f) = c.destroy_pipeline { (f)(dev, st.pipe, std::ptr::null()); }
            if let Some(f) = c.destroy_pipe_layout { (f)(dev, st.pipe_layout, std::ptr::null()); }
            if let Some(f) = c.destroy_desc_set_layout { (f)(dev, st.desc_layout, std::ptr::null()); }
            if let Some(f) = c.destroy_desc_pool { if st.desc_pool != vk::DescriptorPool::null() { (f)(dev, st.desc_pool, std::ptr::null()); } }
            if let Some(f) = c.destroy_cmd_pool { if st.cmd_pool != vk::CommandPool::null() { (f)(dev, st.cmd_pool, std::ptr::null()); } }
            
            for (i, m) in st.proxy_images.iter().zip(st.proxy_mems.iter()) {
                if let Some(f) = c.destroy_image { (f)(dev, *i, std::ptr::null()); }
                if let Some(f) = c.free_memory { (f)(dev, *m, std::ptr::null()); }
            }
            for (i, m) in st.work_images.iter().zip(st.work_mems.iter()) {
                if let Some(f) = c.destroy_image { (f)(dev, *i, std::ptr::null()); }
                if let Some(f) = c.free_memory { (f)(dev, *m, std::ptr::null()); }
            }
            for v in st.proxy_views.iter().chain(st.work_views.iter()) {
                if let Some(f) = c.destroy_image_view { (f)(dev, *v, std::ptr::null()); }
            }
            if let Some(f) = c.destroy_semaphore {
                for s in st.present_semaphores {
                    (f)(dev, s, std::ptr::null());
                }
            }
        }
        if let Some(real_f) = c.real_destroy_swapchain {
            (real_f)(dev, sc, p_al);
        } else if let Some(ptr) = (c.gdpa)(dev, b"vkDestroySwapchainKHR\0".as_ptr() as *const c_char) {
            let real_f = std::mem::transmute::<_, vk::PFN_vkDestroySwapchainKHR>(ptr);
            (real_f)(dev, sc, p_al);
        }
    }
}

unsafe extern "system" fn hook_get_swapchain_images_khr(dev: vk::Device, sc: vk::SwapchainKHR, p_sic: *mut u32, p_si: *mut vk::Image) -> vk::Result {
    let ctx_map = DEVICE_CONTEXTS.read().unwrap();
    let c = match ctx_map.get(&dev) { Some(v) => v, None => return vk::Result::ERROR_INITIALIZATION_FAILED };
    let real_f = match c.real_get_swapchain_images { 
        Some(ptr) => ptr, 
        None => match (c.gdpa)(dev, b"vkGetSwapchainImagesKHR\0".as_ptr() as *const c_char) {
            Some(ptr) => std::mem::transmute::<_, vk::PFN_vkGetSwapchainImagesKHR>(ptr),
            None => return vk::Result::ERROR_INITIALIZATION_FAILED,
        }
    };
    let res = (real_f)(dev, sc, p_sic, p_si);
    if (res == vk::Result::SUCCESS || res == vk::Result::INCOMPLETE) && !p_si.is_null() {
        let mut st_map = SWAPCHAIN_STATES.write().unwrap();
        let st = match st_map.get_mut(&sc) { Some(v) => v, None => return res };
        if st.bypass { return res; }
        let count = *p_sic as usize;
        if st.real_images.is_empty() {
            st.real_images = std::slice::from_raw_parts(p_si, count).to_vec();
            if let (Some(ci), Some(gimr), Some(am), Some(bim), Some(civ), Some(cdp), Some(ads), Some(uds), Some(csem)) = (c.create_image, c.get_image_mem_req, c.allocate_mem, c.bind_image_mem, c.create_image_view, c.create_desc_pool, c.alloc_desc_sets, c.update_desc_sets, c.create_semaphore) {
                let next_gipa = match *GLOBAL_GIPA.read().unwrap() { Some(f) => f, None => return vk::Result::ERROR_INITIALIZATION_FAILED };
                let inst = match *GLOBAL_INSTANCE.read().unwrap() { Some(i) => i, None => return vk::Result::ERROR_INITIALIZATION_FAILED };
                let f_mp = match next_gipa(inst, b"vkGetPhysicalDeviceMemoryProperties\0".as_ptr() as *const c_char) {
                    Some(f) => f,
                    None => return vk::Result::ERROR_INITIALIZATION_FAILED,
                };
                let pfn_gpdmp: vk::PFN_vkGetPhysicalDeviceMemoryProperties = std::mem::transmute(f_mp);
                
                let work_format = if st.output_mode == 1 { vk::Format::R16G16B16A16_SFLOAT } else { vk::Format::A2B10G10R10_UNORM_PACK32 };
                
                for _ in 0..count {
                    let mut pi = vk::Image::null(); let _ = (ci)(dev, &vk::ImageCreateInfo { s_type: vk::StructureType::IMAGE_CREATE_INFO, p_next: std::ptr::null(), flags: vk::ImageCreateFlags::empty(), image_type: vk::ImageType::TYPE_2D, format: st.sdr_format, extent: vk::Extent3D { width: st.width, height: st.height, depth: 1 }, mip_levels: 1, array_layers: 1, samples: vk::SampleCountFlags::TYPE_1, tiling: vk::ImageTiling::OPTIMAL, usage: vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_SRC, sharing_mode: vk::SharingMode::EXCLUSIVE, queue_family_index_count: 0, p_queue_family_indices: std::ptr::null(), initial_layout: vk::ImageLayout::UNDEFINED }, std::ptr::null(), &mut pi);
                    let mut wi = vk::Image::null(); let _ = (ci)(dev, &vk::ImageCreateInfo { s_type: vk::StructureType::IMAGE_CREATE_INFO, p_next: std::ptr::null(), flags: vk::ImageCreateFlags::empty(), image_type: vk::ImageType::TYPE_2D, format: work_format, extent: vk::Extent3D { width: st.width, height: st.height, depth: 1 }, mip_levels: 1, array_layers: 1, samples: vk::SampleCountFlags::TYPE_1, tiling: vk::ImageTiling::OPTIMAL, usage: vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_SRC, sharing_mode: vk::SharingMode::EXCLUSIVE, queue_family_index_count: 0, p_queue_family_indices: std::ptr::null(), initial_layout: vk::ImageLayout::UNDEFINED }, std::ptr::null(), &mut wi);
                    let mut mr_p = vk::MemoryRequirements::default(); (gimr)(dev, pi, &mut mr_p);
                    let mut mr_w = vk::MemoryRequirements::default(); (gimr)(dev, wi, &mut mr_w);
                    let mut mp = vk::PhysicalDeviceMemoryProperties::default(); (pfn_gpdmp)(c.pd, &mut mp);
                    let mti_p = (0..mp.memory_type_count).find(|&j| (mr_p.memory_type_bits & (1 << j)) != 0 && (mp.memory_types[j as usize].property_flags & vk::MemoryPropertyFlags::DEVICE_LOCAL) != vk::MemoryPropertyFlags::empty()).expect("Failed to find memory type");
                    let mti_w = (0..mp.memory_type_count).find(|&j| (mr_w.memory_type_bits & (1 << j)) != 0 && (mp.memory_types[j as usize].property_flags & vk::MemoryPropertyFlags::DEVICE_LOCAL) != vk::MemoryPropertyFlags::empty()).expect("Failed to find memory type");
                    let mut pm = vk::DeviceMemory::null(); let _ = (am)(dev, &vk::MemoryAllocateInfo { s_type: vk::StructureType::MEMORY_ALLOCATE_INFO, p_next: std::ptr::null(), allocation_size: mr_p.size, memory_type_index: mti_p }, std::ptr::null(), &mut pm);
                    let mut wm = vk::DeviceMemory::null(); let _ = (am)(dev, &vk::MemoryAllocateInfo { s_type: vk::StructureType::MEMORY_ALLOCATE_INFO, p_next: std::ptr::null(), allocation_size: mr_w.size, memory_type_index: mti_w }, std::ptr::null(), &mut wm);
                    let _ = (bim)(dev, pi, pm, 0); let _ = (bim)(dev, wi, wm, 0);
                    st.proxy_images.push(pi); st.proxy_mems.push(pm); st.work_images.push(wi); st.work_mems.push(wm);
                    let mut pv = vk::ImageView::null(); let _ = (civ)(dev, &vk::ImageViewCreateInfo { s_type: vk::StructureType::IMAGE_VIEW_CREATE_INFO, p_next: std::ptr::null(), flags: vk::ImageViewCreateFlags::empty(), image: pi, view_type: vk::ImageViewType::TYPE_2D, format: st.sdr_format, components: vk::ComponentMapping::default(), subresource_range: vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 } }, std::ptr::null(), &mut pv);
                    let mut wv = vk::ImageView::null(); let _ = (civ)(dev, &vk::ImageViewCreateInfo { s_type: vk::StructureType::IMAGE_VIEW_CREATE_INFO, p_next: std::ptr::null(), flags: vk::ImageViewCreateFlags::empty(), image: wi, view_type: vk::ImageViewType::TYPE_2D, format: work_format, components: vk::ComponentMapping::default(), subresource_range: vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 } }, std::ptr::null(), &mut wv);
                    st.proxy_views.push(pv); st.work_views.push(wv);
                    
                    let mut sem = vk::Semaphore::null();
                    let _ = (csem)(dev, &vk::SemaphoreCreateInfo { s_type: vk::StructureType::SEMAPHORE_CREATE_INFO, p_next: std::ptr::null(), flags: vk::SemaphoreCreateFlags::empty() }, std::ptr::null(), &mut sem);
                    st.present_semaphores.push(sem);
                }
                let ps = [vk::DescriptorPoolSize { ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER, descriptor_count: count as u32 }, vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_IMAGE, descriptor_count: count as u32 }];
                let _ = (cdp)(dev, &vk::DescriptorPoolCreateInfo { s_type: vk::StructureType::DESCRIPTOR_POOL_CREATE_INFO, p_next: std::ptr::null(), flags: vk::DescriptorPoolCreateFlags::empty(), max_sets: count as u32, pool_size_count: 2, p_pool_sizes: ps.as_ptr() }, std::ptr::null(), &mut st.desc_pool);
                st.desc_sets.resize(count, vk::DescriptorSet::null());
                let layouts = vec![st.desc_layout; count];
                let _ = (ads)(dev, &vk::DescriptorSetAllocateInfo { s_type: vk::StructureType::DESCRIPTOR_SET_ALLOCATE_INFO, p_next: std::ptr::null(), descriptor_pool: st.desc_pool, descriptor_set_count: count as u32, p_set_layouts: layouts.as_ptr() }, st.desc_sets.as_mut_ptr());
                for i in 0..count {
                    let pi = vk::DescriptorImageInfo { sampler: st.sampler, image_view: st.proxy_views[i], image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL };
                    let wi = vk::DescriptorImageInfo { sampler: vk::Sampler::null(), image_view: st.work_views[i], image_layout: vk::ImageLayout::GENERAL };
                    let writes = [vk::WriteDescriptorSet { s_type: vk::StructureType::WRITE_DESCRIPTOR_SET, p_next: std::ptr::null(), dst_set: st.desc_sets[i], dst_binding: 0, dst_array_element: 0, descriptor_count: 1, descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER, p_image_info: &pi, p_buffer_info: std::ptr::null(), p_texel_buffer_view: std::ptr::null() },
                                  vk::WriteDescriptorSet { s_type: vk::StructureType::WRITE_DESCRIPTOR_SET, p_next: std::ptr::null(), dst_set: st.desc_sets[i], dst_binding: 1, dst_array_element: 0, descriptor_count: 1, descriptor_type: vk::DescriptorType::STORAGE_IMAGE, p_image_info: &wi, p_buffer_info: std::ptr::null(), p_texel_buffer_view: std::ptr::null() }];
                    (uds)(dev, 2, writes.as_ptr(), 0, std::ptr::null());
                }
            }
        }
        std::ptr::copy_nonoverlapping(st.proxy_images.as_ptr(), p_si, count);
    }
    res
}

unsafe extern "system" fn hook_acquire_next_image_khr(dev: vk::Device, sc: vk::SwapchainKHR, t: u64, s: vk::Semaphore, f_h: vk::Fence, p_ii: *mut u32) -> vk::Result {
    if let Some(c) = DEVICE_CONTEXTS.read().unwrap().get(&dev) { 
        if let Some(ptr) = c.real_acquire_next_image { return (ptr)(dev, sc, t, s, f_h, p_ii); }
        else { 
            let ptr = match (c.gdpa)(dev, b"vkAcquireNextImageKHR\0".as_ptr() as *const c_char) {
                Some(ptr) => ptr,
                None => return vk::Result::ERROR_INITIALIZATION_FAILED,
            };
            return (std::mem::transmute::<_, vk::PFN_vkAcquireNextImageKHR>(ptr))(dev, sc, t, s, f_h, p_ii); 
        }
    }
    vk::Result::ERROR_INITIALIZATION_FAILED
}

unsafe extern "system" fn hook_queue_present_khr(q: vk::Queue, p_pi: *const vk::PresentInfoKHR) -> vk::Result {
    let (dev, qfi) = QUEUE_TO_DEVICE.read().unwrap().get(&q).copied().unwrap_or((vk::Device::null(), 0));
    if dev != vk::Device::null() {
        let ctx_map = DEVICE_CONTEXTS.read().unwrap();
        if let Some(c) = ctx_map.get(&dev) {
            let mut st_map = SWAPCHAIN_STATES.write().unwrap();
            let mut wait_semaphores = Vec::new();
            for j in 0..(*p_pi).wait_semaphore_count as usize {
                wait_semaphores.push(*(*p_pi).p_wait_semaphores.add(j));
            }
            
            let mut cmd_bufs = Vec::new();
            let mut signal_semaphores = Vec::new();
            let mut any_hdr = false;
            
            for i in 0..(*p_pi).swapchain_count as usize {
                let sc = *(*p_pi).p_swapchains.add(i);
                let ii = *(*p_pi).p_image_indices.add(i) as usize;
                if let Some(st) = st_map.get_mut(&sc) {
                    if st.bypass { continue; }
                    if !st.proxy_images.is_empty() {
                        if let (Some(ccp), Some(acb), Some(bcb), Some(ecb), Some(cpb), Some(cbp), Some(cbds), Some(cpc), Some(cd), Some(cci)) = (c.create_cmd_pool, c.alloc_cmd_bufs, c.begin_cmd_buf, c.end_cmd_buf, c.cmd_pipeline_barrier, c.cmd_bind_pipe, c.cmd_bind_desc_sets, c.cmd_push_constants, c.cmd_dispatch, c.cmd_copy_image) {
                            any_hdr = true;
                            if st.cmd_pool == vk::CommandPool::null() { let _ = (ccp)(dev, &vk::CommandPoolCreateInfo { s_type: vk::StructureType::COMMAND_POOL_CREATE_INFO, p_next: std::ptr::null(), flags: vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER, queue_family_index: qfi }, std::ptr::null(), &mut st.cmd_pool); }
                            if st.cmd_bufs.is_empty() { st.cmd_bufs.resize(st.proxy_images.len(), vk::CommandBuffer::null()); let _ = (acb)(dev, &vk::CommandBufferAllocateInfo { s_type: vk::StructureType::COMMAND_BUFFER_ALLOCATE_INFO, p_next: std::ptr::null(), command_pool: st.cmd_pool, level: vk::CommandBufferLevel::PRIMARY, command_buffer_count: st.proxy_images.len() as u32 }, st.cmd_bufs.as_mut_ptr()); }
                            let cb = st.cmd_bufs[ii];
                            let _ = (bcb)(cb, &vk::CommandBufferBeginInfo { s_type: vk::StructureType::COMMAND_BUFFER_BEGIN_INFO, p_next: std::ptr::null(), flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, p_inheritance_info: std::ptr::null() });
                            let sr = vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 };
                            (cpb)(cb, vk::PipelineStageFlags::ALL_COMMANDS, vk::PipelineStageFlags::COMPUTE_SHADER, vk::DependencyFlags::empty(), 0, std::ptr::null(), 0, std::ptr::null(), 2, [vk::ImageMemoryBarrier { s_type: vk::StructureType::IMAGE_MEMORY_BARRIER, p_next: std::ptr::null(), src_access_mask: vk::AccessFlags::MEMORY_WRITE, dst_access_mask: vk::AccessFlags::SHADER_READ, old_layout: vk::ImageLayout::PRESENT_SRC_KHR, new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL, src_queue_family_index: vk::QUEUE_FAMILY_IGNORED, dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED, image: st.proxy_images[ii], subresource_range: sr }, vk::ImageMemoryBarrier { s_type: vk::StructureType::IMAGE_MEMORY_BARRIER, p_next: std::ptr::null(), src_access_mask: vk::AccessFlags::empty(), dst_access_mask: vk::AccessFlags::SHADER_WRITE, old_layout: vk::ImageLayout::UNDEFINED, new_layout: vk::ImageLayout::GENERAL, src_queue_family_index: vk::QUEUE_FAMILY_IGNORED, dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED, image: st.work_images[ii], subresource_range: sr }].as_ptr());
                            (cbp)(cb, vk::PipelineBindPoint::COMPUTE, st.pipe);
                            (cbds)(cb, vk::PipelineBindPoint::COMPUTE, st.pipe_layout, 0, 1, &st.desc_sets[ii], 0, std::ptr::null());
                            (cpc)(cb, st.pipe_layout, vk::ShaderStageFlags::COMPUTE, 0, 44, &PushConstants { 
                                max_lum: HDR_CONFIG.max_lum, 
                                mid_lum: HDR_CONFIG.mid_lum, 
                                sat: HDR_CONFIG.sat, 
                                vibrance: HDR_CONFIG.vibrance,
                                width: st.width, 
                                height: st.height, 
                                rcas_strength: HDR_CONFIG.rcas_strength,
                                fxaa_strength: HDR_CONFIG.fxaa_strength,
                                intensity: HDR_CONFIG.intensity,
                                black_level: HDR_CONFIG.black_level,
                                sdr_gain: HDR_CONFIG.sdr_gain,
                                output_mode: st.output_mode,
                            } as *const _ as *const _);                            (cd)(cb, (st.width + 15) / 16, (st.height + 15) / 16, 1);
                            (cpb)(cb, vk::PipelineStageFlags::COMPUTE_SHADER, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), 0, std::ptr::null(), 0, std::ptr::null(), 2, [vk::ImageMemoryBarrier { s_type: vk::StructureType::IMAGE_MEMORY_BARRIER, p_next: std::ptr::null(), src_access_mask: vk::AccessFlags::SHADER_WRITE, dst_access_mask: vk::AccessFlags::TRANSFER_READ, old_layout: vk::ImageLayout::GENERAL, new_layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL, src_queue_family_index: vk::QUEUE_FAMILY_IGNORED, dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED, image: st.work_images[ii], subresource_range: sr }, vk::ImageMemoryBarrier { s_type: vk::StructureType::IMAGE_MEMORY_BARRIER, p_next: std::ptr::null(), src_access_mask: vk::AccessFlags::empty(), dst_access_mask: vk::AccessFlags::TRANSFER_WRITE, old_layout: vk::ImageLayout::UNDEFINED, new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL, src_queue_family_index: vk::QUEUE_FAMILY_IGNORED, dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED, image: st.real_images[ii], subresource_range: sr }].as_ptr());
                            let region = vk::ImageCopy { src_subresource: vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: 0, base_array_layer: 0, layer_count: 1 }, src_offset: vk::Offset3D { x: 0, y: 0, z: 0 }, dst_subresource: vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: 0, base_array_layer: 0, layer_count: 1 }, dst_offset: vk::Offset3D { x: 0, y: 0, z: 0 }, extent: vk::Extent3D { width: st.width, height: st.height, depth: 1 } };
                            (cci)(cb, st.work_images[ii], vk::ImageLayout::TRANSFER_SRC_OPTIMAL, st.real_images[ii], vk::ImageLayout::TRANSFER_DST_OPTIMAL, 1, &region);
                            (cpb)(cb, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::BOTTOM_OF_PIPE, vk::DependencyFlags::empty(), 0, std::ptr::null(), 0, std::ptr::null(), 1, [vk::ImageMemoryBarrier { s_type: vk::StructureType::IMAGE_MEMORY_BARRIER, p_next: std::ptr::null(), src_access_mask: vk::AccessFlags::TRANSFER_WRITE, dst_access_mask: vk::AccessFlags::MEMORY_READ, old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL, new_layout: vk::ImageLayout::PRESENT_SRC_KHR, src_queue_family_index: vk::QUEUE_FAMILY_IGNORED, dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED, image: st.real_images[ii], subresource_range: sr }].as_ptr());
                            let _ = (ecb)(cb);
                            cmd_bufs.push(cb);
                            signal_semaphores.push(st.present_semaphores[ii]);
                        }
                    }
                }
            }
            
            let mut new_present_info = *p_pi;
            if any_hdr {
                if let Some(qs) = c.queue_submit {
                    let wait_stages = vec![vk::PipelineStageFlags::COMPUTE_SHADER; wait_semaphores.len()];
                    let _ = (qs)(q, 1, &vk::SubmitInfo { 
                        s_type: vk::StructureType::SUBMIT_INFO, 
                        p_next: std::ptr::null(), 
                        wait_semaphore_count: wait_semaphores.len() as u32, 
                        p_wait_semaphores: if wait_semaphores.is_empty() { std::ptr::null() } else { wait_semaphores.as_ptr() }, 
                        p_wait_dst_stage_mask: if wait_stages.is_empty() { std::ptr::null() } else { wait_stages.as_ptr() }, 
                        command_buffer_count: cmd_bufs.len() as u32, 
                        p_command_buffers: cmd_bufs.as_ptr(), 
                        signal_semaphore_count: signal_semaphores.len() as u32, 
                        p_signal_semaphores: signal_semaphores.as_ptr() 
                    }, vk::Fence::null());
                    new_present_info.wait_semaphore_count = signal_semaphores.len() as u32;
                    new_present_info.p_wait_semaphores = signal_semaphores.as_ptr();
                }
            }
            
            if let Some(ptr) = c.real_queue_present { return (ptr)(q, &new_present_info); }
            else { 
                if let Some(f) = (c.gdpa)(dev, b"vkQueuePresentKHR\0".as_ptr() as *const c_char) {
                    return (std::mem::transmute::<_, vk::PFN_vkQueuePresentKHR>(f))(q, &new_present_info);
                }
            }
        }
    }
    vk::Result::SUCCESS
}
