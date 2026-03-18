use ash::vk;
use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::sync::RwLock;
use std::collections::HashMap;

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
    static ref GLOBAL_GIPA: RwLock<Option<vk::PFN_vkGetInstanceProcAddr>> = RwLock::new(None);
    static ref GLOBAL_INSTANCE: RwLock<Option<vk::Instance>> = RwLock::new(None);
    static ref HDR_CONFIG: HdrConfig = HdrConfig::from_env();
}

struct HdrConfig { pub max_lum: f32, pub mid_lum: f32, pub sat: f32 }
impl HdrConfig {
    fn from_env() -> Self {
        let max_lum = std::env::var("AUTOHDR_MAX_LUMINANCE").ok().and_then(|v| v.parse().ok()).unwrap_or(1000.0);
        let mid_lum = std::env::var("AUTOHDR_MID_LUMINANCE").ok().and_then(|v| v.parse().ok()).unwrap_or(50.0);
        let sat = std::env::var("AUTOHDR_SATURATION").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
        eprintln!("[Vulkan HDR Layer] Finałowa Naprawa: Max={} Mid={} Sat={}", max_lum, mid_lum, sat);
        Self { max_lum, mid_lum, sat }
    }
}

#[repr(C)] #[derive(Clone, Copy)]
struct PushConstants { max_lum: f32, mid_lum: f32, sat: f32, width: u32, height: u32, pad: [u32; 3] }

pub struct DeviceContext {
    pub pd: vk::PhysicalDevice, pub gdpa: vk::PFN_vkGetDeviceProcAddr, pub gipa: vk::PFN_vkGetInstanceProcAddr,
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
    pub real_create_swapchain: vk::PFN_vkCreateSwapchainKHR, pub real_get_swapchain_images: vk::PFN_vkGetSwapchainImagesKHR,
    pub real_queue_present: vk::PFN_vkQueuePresentKHR, pub real_get_device_queue: vk::PFN_vkGetDeviceQueue,
    pub real_acquire_next_image: vk::PFN_vkAcquireNextImageKHR,
}

pub struct SwapchainState {
    pub width: u32, pub height: u32, pub sdr_format: vk::Format,
    pub proxy_images: Vec<vk::Image>, pub proxy_mems: Vec<vk::DeviceMemory>, pub proxy_views: Vec<vk::ImageView>,
    pub real_images: Vec<vk::Image>, pub real_views: Vec<vk::ImageView>,
    pub pipe: vk::Pipeline, pub pipe_layout: vk::PipelineLayout, pub desc_layout: vk::DescriptorSetLayout,
    pub desc_pool: vk::DescriptorPool, pub desc_sets: Vec<vk::DescriptorSet>,
    pub cmd_pool: vk::CommandPool, pub cmd_bufs: Vec<vk::CommandBuffer>,
    pub sampler: vk::Sampler,
}

#[no_mangle] pub unsafe extern "system" fn vkNegotiateLoaderLayerInterfaceVersion(p_vs: *mut NegotiateLayerInterface) -> vk::Result { (*p_vs).pfn_get_instance_proc_addr = Some(hook_get_instance_proc_addr); (*p_vs).pfn_get_device_proc_addr = Some(hook_get_device_proc_addr); vk::Result::SUCCESS }

unsafe extern "system" fn hook_get_instance_proc_addr(inst: vk::Instance, p_name: *const c_char) -> vk::PFN_vkVoidFunction {
    let name = CStr::from_ptr(p_name).to_str().unwrap_or("");
    match name {
        "vkCreateInstance" => Some(std::mem::transmute(hook_create_instance as *const ())),
        "vkCreateDevice" => Some(std::mem::transmute(hook_create_device as *const ())),
        "vkGetPhysicalDeviceSurfaceFormatsKHR" => Some(std::mem::transmute(hook_get_pd_surface_formats as *const ())),
        "vkCreateSwapchainKHR" => Some(std::mem::transmute(hook_create_swapchain_khr as *const ())),
        "vkGetSwapchainImagesKHR" => Some(std::mem::transmute(hook_get_swapchain_images_khr as *const ())),
        "vkQueuePresentKHR" => Some(std::mem::transmute(hook_queue_present_khr as *const ())),
        "vkAcquireNextImageKHR" => Some(std::mem::transmute(hook_acquire_next_image_khr as *const ())),
        "vkGetDeviceQueue" => Some(std::mem::transmute(hook_get_device_queue as *const ())),
        _ => { if inst == vk::Instance::null() { None } else { INSTANCE_GIPA.read().unwrap().get(&inst).and_then(|f| f(inst, p_name)) } }
    }
}

unsafe extern "system" fn hook_get_device_proc_addr(dev: vk::Device, p_name: *const c_char) -> vk::PFN_vkVoidFunction {
    let name = CStr::from_ptr(p_name).to_str().unwrap_or("");
    match name {
        "vkCreateSwapchainKHR" => Some(std::mem::transmute(hook_create_swapchain_khr as *const ())),
        "vkGetSwapchainImagesKHR" => Some(std::mem::transmute(hook_get_swapchain_images_khr as *const ())),
        "vkQueuePresentKHR" => Some(std::mem::transmute(hook_queue_present_khr as *const ())),
        "vkAcquireNextImageKHR" => Some(std::mem::transmute(hook_acquire_next_image_khr as *const ())),
        "vkGetDeviceQueue" => Some(std::mem::transmute(hook_get_device_queue as *const ())),
        _ => { DEVICE_GDPA.read().unwrap().get(&dev).and_then(|f| f(dev, p_name)) }
    }
}

unsafe extern "system" fn hook_get_device_queue(dev: vk::Device, qfi: u32, qi: u32, p_q: *mut vk::Queue) {
    if let Some(c) = DEVICE_CONTEXTS.read().unwrap().get(&dev) {
        (c.real_get_device_queue)(dev, qfi, qi, p_q);
        if !p_q.is_null() && *p_q != vk::Queue::null() { QUEUE_TO_DEVICE.write().unwrap().insert(*p_q, (dev, qfi)); }
    }
}

unsafe extern "system" fn hook_get_pd_surface_formats(_pd: vk::PhysicalDevice, _surface: vk::SurfaceKHR, p_fc: *mut u32, p_formats: *mut vk::SurfaceFormatKHR) -> vk::Result {
    if p_formats.is_null() { *p_fc = 1; return vk::Result::SUCCESS; }
    *p_fc = 1;
    // Format A2B10G10R10 naprawia Teal -> Gold
    *p_formats = vk::SurfaceFormatKHR { format: vk::Format::A2B10G10R10_UNORM_PACK32, color_space: vk::ColorSpaceKHR::HDR10_ST2084_EXT };
    vk::Result::SUCCESS
}

unsafe extern "system" fn hook_create_instance(p_ci: *const vk::InstanceCreateInfo, p_al: *const vk::AllocationCallbacks, p_inst: *mut vk::Instance) -> vk::Result {
    lazy_static::initialize(&HDR_CONFIG);
    let mut li = (*p_ci).p_next as *mut VkLayerInstanceCreateInfo;
    while !li.is_null() {
        if (*li).s_type == vk::StructureType::from_raw(47) && (*li).function == 0 {
            let next_gipa = (*(*li).p_layer_info).pfn_next_get_instance_proc_addr;
            (*li).p_layer_info = (*(*li).p_layer_info).p_next;
            let res = (std::mem::transmute::<_, vk::PFN_vkCreateInstance>(next_gipa(vk::Instance::null(), b"vkCreateInstance\0".as_ptr() as *const c_char).unwrap()))(p_ci, p_al, p_inst);
            if res == vk::Result::SUCCESS { INSTANCE_GIPA.write().unwrap().insert(*p_inst, next_gipa); GLOBAL_INSTANCE.write().unwrap().replace(*p_inst); GLOBAL_GIPA.write().unwrap().replace(next_gipa); }
            return res;
        }
        li = (*li).p_next as *mut VkLayerInstanceCreateInfo;
    }
    vk::Result::ERROR_INITIALIZATION_FAILED
}

unsafe extern "system" fn hook_create_device(pd: vk::PhysicalDevice, p_ci: *const vk::DeviceCreateInfo, p_al: *const vk::AllocationCallbacks, p_dev: *mut vk::Device) -> vk::Result {
    let mut li = (*p_ci).p_next as *mut VkLayerDeviceCreateInfo;
    while !li.is_null() {
        if (*li).s_type == vk::StructureType::from_raw(48) && (*li).function == 0 {
            let next_gipa = (*(*li).p_layer_info).pfn_next_get_instance_proc_addr;
            let next_gdpa = (*(*li).p_layer_info).pfn_next_get_device_proc_addr;
            (*li).p_layer_info = (*(*li).p_layer_info).p_next;
            let res = (std::mem::transmute::<_, vk::PFN_vkCreateDevice>(next_gipa(vk::Instance::null(), b"vkCreateDevice\0".as_ptr() as *const c_char).unwrap()))(pd, p_ci, p_al, p_dev);
            if res == vk::Result::SUCCESS {
                DEVICE_GDPA.write().unwrap().insert(*p_dev, next_gdpa);
                let f = |n: &[u8]| next_gdpa(*p_dev, n.as_ptr() as *const c_char);
                DEVICE_CONTEXTS.write().unwrap().insert(*p_dev, DeviceContext {
                    pd, gdpa: next_gdpa, gipa: next_gipa,
                    create_image: f(b"vkCreateImage\0").map(|p| std::mem::transmute(p)),
                    get_image_mem_req: f(b"vkGetImageMemoryRequirements\0").map(|p| std::mem::transmute(p)),
                    allocate_mem: f(b"vkAllocateMemory\0").map(|p| std::mem::transmute(p)),
                    bind_image_mem: f(b"vkBindImageMemory\0").map(|p| std::mem::transmute(p)),
                    create_image_view: f(b"vkCreateImageView\0").map(|p| std::mem::transmute(p)),
                    create_shader_module: f(b"vkCreateShaderModule\0").map(|p| std::mem::transmute(p)),
                    create_desc_set_layout: f(b"vkCreateDescriptorSetLayout\0").map(|p| std::mem::transmute(p)),
                    create_pipe_layout: f(b"vkCreatePipelineLayout\0").map(|p| std::mem::transmute(p)),
                    create_compute_pipes: f(b"vkCreateComputePipelines\0").map(|p| std::mem::transmute(p)),
                    create_desc_pool: f(b"vkCreateDescriptorPool\0").map(|p| std::mem::transmute(p)),
                    alloc_desc_sets: f(b"vkAllocateDescriptorSets\0").map(|p| std::mem::transmute(p)),
                    update_desc_sets: f(b"vkUpdateDescriptorSets\0").map(|p| std::mem::transmute(p)),
                    create_cmd_pool: f(b"vkCreateCommandPool\0").map(|p| std::mem::transmute(p)),
                    alloc_cmd_bufs: f(b"vkAllocateCommandBuffers\0").map(|p| std::mem::transmute(p)),
                    begin_cmd_buf: f(b"vkBeginCommandBuffer\0").map(|p| std::mem::transmute(p)),
                    end_cmd_buf: f(b"vkEndCommandBuffer\0").map(|p| std::mem::transmute(p)),
                    cmd_pipeline_barrier: f(b"vkCmdPipelineBarrier\0").map(|p| std::mem::transmute(p)),
                    cmd_bind_pipe: f(b"vkCmdBindPipeline\0").map(|p| std::mem::transmute(p)),
                    cmd_bind_desc_sets: f(b"vkCmdBindDescriptorSets\0").map(|p| std::mem::transmute(p)),
                    cmd_push_constants: f(b"vkCmdPushConstants\0").map(|p| std::mem::transmute(p)),
                    cmd_dispatch: f(b"vkCmdDispatch\0").map(|p| std::mem::transmute(p)),
                    queue_submit: f(b"vkQueueSubmit\0").map(|p| std::mem::transmute(p)),
                    queue_wait_idle: f(b"vkQueueWaitIdle\0").map(|p| std::mem::transmute(p)),
                    create_sampler: f(b"vkCreateSampler\0").map(|p| std::mem::transmute(p)),
                    real_create_swapchain: std::mem::transmute(f(b"vkCreateSwapchainKHR\0").unwrap()),
                    real_get_swapchain_images: std::mem::transmute(f(b"vkGetSwapchainImagesKHR\0").unwrap()),
                    real_queue_present: std::mem::transmute(f(b"vkQueuePresentKHR\0").unwrap()),
                    real_get_device_queue: std::mem::transmute(f(b"vkGetDeviceQueue\0").unwrap()),
                    real_acquire_next_image: std::mem::transmute(f(b"vkAcquireNextImageKHR\0").unwrap()),
                });
            }
            return res;
        }
        li = (*li).p_next as *mut VkLayerDeviceCreateInfo;
    }
    vk::Result::ERROR_INITIALIZATION_FAILED
}

unsafe extern "system" fn hook_create_swapchain_khr(dev: vk::Device, p_ci: *const vk::SwapchainCreateInfoKHR, p_al: *const vk::AllocationCallbacks, p_sc: *mut vk::SwapchainKHR) -> vk::Result {
    let mut mi = *p_ci;
    mi.image_format = vk::Format::A2B10G10R10_UNORM_PACK32;
    mi.image_color_space = vk::ColorSpaceKHR::HDR10_ST2084_EXT;
    mi.image_usage |= vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_DST;
    let ctx_map = DEVICE_CONTEXTS.read().unwrap();
    let c = ctx_map.get(&dev).unwrap();
    let res = (c.real_create_swapchain)(dev, &mi, p_al, p_sc);
    if res == vk::Result::SUCCESS {
        if let (Some(csm), Some(cdsl), Some(cpl), Some(ccp), Some(csamp)) = (c.create_shader_module, c.create_desc_set_layout, c.create_pipe_layout, c.create_compute_pipes, c.create_sampler) {
            let mut sm = vk::ShaderModule::null(); let _ = (csm)(dev, &vk::ShaderModuleCreateInfo { s_type: vk::StructureType::SHADER_MODULE_CREATE_INFO, p_next: std::ptr::null(), flags: vk::ShaderModuleCreateFlags::empty(), code_size: SHADER_CODE.len() * 4, p_code: SHADER_CODE.as_ptr() }, std::ptr::null(), &mut sm);
            let bds = [vk::DescriptorSetLayoutBinding { binding: 0, descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER, descriptor_count: 1, stage_flags: vk::ShaderStageFlags::COMPUTE, p_immutable_samplers: std::ptr::null() },
                       vk::DescriptorSetLayoutBinding { binding: 1, descriptor_type: vk::DescriptorType::STORAGE_IMAGE, descriptor_count: 1, stage_flags: vk::ShaderStageFlags::COMPUTE, p_immutable_samplers: std::ptr::null() }];
            let mut dsl = vk::DescriptorSetLayout::null(); let _ = (cdsl)(dev, &vk::DescriptorSetLayoutCreateInfo { s_type: vk::StructureType::DESCRIPTOR_SET_LAYOUT_CREATE_INFO, p_next: std::ptr::null(), flags: vk::DescriptorSetLayoutCreateFlags::empty(), binding_count: 2, p_bindings: bds.as_ptr() }, std::ptr::null(), &mut dsl);
            let mut pl = vk::PipelineLayout::null(); let _ = (cpl)(dev, &vk::PipelineLayoutCreateInfo { s_type: vk::StructureType::PIPELINE_LAYOUT_CREATE_INFO, p_next: std::ptr::null(), flags: vk::PipelineLayoutCreateFlags::empty(), set_layout_count: 1, p_set_layouts: &dsl, push_constant_range_count: 1, p_push_constant_ranges: &vk::PushConstantRange { stage_flags: vk::ShaderStageFlags::COMPUTE, offset: 0, size: 32 } }, std::ptr::null(), &mut pl);
            let stage = vk::PipelineShaderStageCreateInfo { s_type: vk::StructureType::PIPELINE_SHADER_STAGE_CREATE_INFO, p_next: std::ptr::null(), flags: vk::PipelineShaderStageCreateFlags::empty(), stage: vk::ShaderStageFlags::COMPUTE, module: sm, p_name: b"main\0".as_ptr() as *const c_char, p_specialization_info: std::ptr::null() };
            let mut pipe = vk::Pipeline::null(); let _ = (ccp)(dev, vk::PipelineCache::null(), 1, &vk::ComputePipelineCreateInfo { s_type: vk::StructureType::COMPUTE_PIPELINE_CREATE_INFO, p_next: std::ptr::null(), flags: vk::PipelineCreateFlags::empty(), stage, layout: pl, base_pipeline_handle: vk::Pipeline::null(), base_pipeline_index: -1 }, std::ptr::null(), &mut pipe);
            let mut sampler = vk::Sampler::null(); let _ = (csamp)(dev, &vk::SamplerCreateInfo { s_type: vk::StructureType::SAMPLER_CREATE_INFO, p_next: std::ptr::null(), flags: vk::SamplerCreateFlags::empty(), mag_filter: vk::Filter::LINEAR, min_filter: vk::Filter::LINEAR, mipmap_mode: vk::SamplerMipmapMode::LINEAR, address_mode_u: vk::SamplerAddressMode::CLAMP_TO_EDGE, address_mode_v: vk::SamplerAddressMode::CLAMP_TO_EDGE, address_mode_w: vk::SamplerAddressMode::CLAMP_TO_EDGE, mip_lod_bias: 0.0, anisotropy_enable: vk::FALSE, max_anisotropy: 1.0, compare_enable: vk::FALSE, compare_op: vk::CompareOp::ALWAYS, min_lod: 0.0, max_lod: 0.0, border_color: vk::BorderColor::FLOAT_TRANSPARENT_BLACK, unnormalized_coordinates: vk::FALSE }, std::ptr::null(), &mut sampler);
            SWAPCHAIN_STATES.write().unwrap().insert(*p_sc, SwapchainState { width: mi.image_extent.width, height: mi.image_extent.height, sdr_format: (*p_ci).image_format, proxy_images: Vec::new(), proxy_mems: Vec::new(), proxy_views: Vec::new(), real_images: Vec::new(), real_views: Vec::new(), pipe, pipe_layout: pl, desc_layout: dsl, desc_pool: vk::DescriptorPool::null(), desc_sets: Vec::new(), cmd_pool: vk::CommandPool::null(), cmd_bufs: Vec::new(), sampler });
        }
    }
    res
}

unsafe extern "system" fn hook_get_swapchain_images_khr(dev: vk::Device, sc: vk::SwapchainKHR, p_sic: *mut u32, p_si: *mut vk::Image) -> vk::Result {
    let ctx_map = DEVICE_CONTEXTS.read().unwrap();
    let c = ctx_map.get(&dev).unwrap();
    let res = (c.real_get_swapchain_images)(dev, sc, p_sic, p_si);
    if res == vk::Result::SUCCESS && !p_si.is_null() {
        let mut st_map = SWAPCHAIN_STATES.write().unwrap();
        let st = st_map.get_mut(&sc).unwrap();
        let count = *p_sic as usize;
        if st.real_images.is_empty() {
            st.real_images = std::slice::from_raw_parts(p_si, count).to_vec();
            let next_gipa = GLOBAL_GIPA.read().unwrap().unwrap();
            let inst = GLOBAL_INSTANCE.read().unwrap().unwrap();
            let pfn_gpdmp: vk::PFN_vkGetPhysicalDeviceMemoryProperties = std::mem::transmute(next_gipa(inst, b"vkGetPhysicalDeviceMemoryProperties\0".as_ptr() as *const c_char).unwrap());
            for i in 0..count {
                let mut pi = vk::Image::null(); let _ = (c.create_image.unwrap())(dev, &vk::ImageCreateInfo { s_type: vk::StructureType::IMAGE_CREATE_INFO, p_next: std::ptr::null(), flags: vk::ImageCreateFlags::empty(), image_type: vk::ImageType::TYPE_2D, format: st.sdr_format, extent: vk::Extent3D { width: st.width, height: st.height, depth: 1 }, mip_levels: 1, array_layers: 1, samples: vk::SampleCountFlags::TYPE_1, tiling: vk::ImageTiling::OPTIMAL, usage: vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::SAMPLED, sharing_mode: vk::SharingMode::EXCLUSIVE, queue_family_index_count: 0, p_queue_family_indices: std::ptr::null(), initial_layout: vk::ImageLayout::UNDEFINED }, std::ptr::null(), &mut pi);
                let mr = { let mut r = vk::MemoryRequirements::default(); (c.get_image_mem_req.unwrap())(dev, pi, &mut r); r };
                let mut mp = vk::PhysicalDeviceMemoryProperties::default(); (pfn_gpdmp)(c.pd, &mut mp);
                let mti = (0..mp.memory_type_count).find(|&j| (mr.memory_type_bits & (1 << j)) != 0 && (mp.memory_types[j as usize].property_flags & vk::MemoryPropertyFlags::DEVICE_LOCAL) != vk::MemoryPropertyFlags::empty()).unwrap_or(0);
                let mut pm = vk::DeviceMemory::null(); let _ = (c.allocate_mem.unwrap())(dev, &vk::MemoryAllocateInfo { s_type: vk::StructureType::MEMORY_ALLOCATE_INFO, p_next: std::ptr::null(), allocation_size: mr.size, memory_type_index: mti }, std::ptr::null(), &mut pm);
                let _ = (c.bind_image_mem.unwrap())(dev, pi, pm, 0);
                st.proxy_images.push(pi); st.proxy_mems.push(pm);
                let mut pv = vk::ImageView::null(); let _ = (c.create_image_view.unwrap())(dev, &vk::ImageViewCreateInfo { s_type: vk::StructureType::IMAGE_VIEW_CREATE_INFO, p_next: std::ptr::null(), flags: vk::ImageViewCreateFlags::empty(), image: pi, view_type: vk::ImageViewType::TYPE_2D, format: st.sdr_format, components: vk::ComponentMapping::default(), subresource_range: vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 } }, std::ptr::null(), &mut pv);
                st.proxy_views.push(pv);
                let mut rv = vk::ImageView::null(); let _ = (c.create_image_view.unwrap())(dev, &vk::ImageViewCreateInfo { s_type: vk::StructureType::IMAGE_VIEW_CREATE_INFO, p_next: std::ptr::null(), flags: vk::ImageViewCreateFlags::empty(), image: st.real_images[i], view_type: vk::ImageViewType::TYPE_2D, format: vk::Format::A2B10G10R10_UNORM_PACK32, components: vk::ComponentMapping::default(), subresource_range: vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 } }, std::ptr::null(), &mut rv);
                st.real_views.push(rv);
            }
            let ps = [vk::DescriptorPoolSize { ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER, descriptor_count: count as u32 }, vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_IMAGE, descriptor_count: count as u32 }];
            let _ = (c.create_desc_pool.unwrap())(dev, &vk::DescriptorPoolCreateInfo { s_type: vk::StructureType::DESCRIPTOR_POOL_CREATE_INFO, p_next: std::ptr::null(), flags: vk::DescriptorPoolCreateFlags::empty(), max_sets: count as u32, pool_size_count: 2, p_pool_sizes: ps.as_ptr() }, std::ptr::null(), &mut st.desc_pool);
            st.desc_sets.resize(count, vk::DescriptorSet::null());
            let layouts = vec![st.desc_layout; count];
            let _ = (c.alloc_desc_sets.unwrap())(dev, &vk::DescriptorSetAllocateInfo { s_type: vk::StructureType::DESCRIPTOR_SET_ALLOCATE_INFO, p_next: std::ptr::null(), descriptor_pool: st.desc_pool, descriptor_set_count: count as u32, p_set_layouts: layouts.as_ptr() }, st.desc_sets.as_mut_ptr());
            for i in 0..count {
                let pi = vk::DescriptorImageInfo { sampler: st.sampler, image_view: st.proxy_views[i], image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL };
                let ri = vk::DescriptorImageInfo { sampler: vk::Sampler::null(), image_view: st.real_views[i], image_layout: vk::ImageLayout::GENERAL };
                let writes = [vk::WriteDescriptorSet { s_type: vk::StructureType::WRITE_DESCRIPTOR_SET, p_next: std::ptr::null(), dst_set: st.desc_sets[i], dst_binding: 0, dst_array_element: 0, descriptor_count: 1, descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER, p_image_info: &pi, p_buffer_info: std::ptr::null(), p_texel_buffer_view: std::ptr::null() },
                                  vk::WriteDescriptorSet { s_type: vk::StructureType::WRITE_DESCRIPTOR_SET, p_next: std::ptr::null(), dst_set: st.desc_sets[i], dst_binding: 1, dst_array_element: 0, descriptor_count: 1, descriptor_type: vk::DescriptorType::STORAGE_IMAGE, p_image_info: &ri, p_buffer_info: std::ptr::null(), p_texel_buffer_view: std::ptr::null() }];
                (c.update_desc_sets.unwrap())(dev, 2, writes.as_ptr(), 0, std::ptr::null());
            }
        }
        std::ptr::copy_nonoverlapping(st.proxy_images.as_ptr(), p_si, count);
    }
    res
}

unsafe extern "system" fn hook_acquire_next_image_khr(dev: vk::Device, sc: vk::SwapchainKHR, t: u64, s: vk::Semaphore, f: vk::Fence, p_ii: *mut u32) -> vk::Result {
    if let Some(c) = DEVICE_CONTEXTS.read().unwrap().get(&dev) { return (c.real_acquire_next_image)(dev, sc, t, s, f, p_ii); }
    vk::Result::ERROR_INITIALIZATION_FAILED
}

unsafe extern "system" fn hook_queue_present_khr(q: vk::Queue, p_pi: *const vk::PresentInfoKHR) -> vk::Result {
    let (dev, qfi) = QUEUE_TO_DEVICE.read().unwrap().get(&q).copied().unwrap_or((vk::Device::null(), 0));
    if dev != vk::Device::null() {
        let ctx_map = DEVICE_CONTEXTS.read().unwrap();
        if let Some(c) = ctx_map.get(&dev) {
            let mut st_map = SWAPCHAIN_STATES.write().unwrap();
            for i in 0..(*p_pi).swapchain_count as usize {
                let sc = *(*p_pi).p_swapchains.add(i);
                let ii = *(*p_pi).p_image_indices.add(i) as usize;
                if let Some(st) = st_map.get_mut(&sc) {
                    if !st.proxy_images.is_empty() {
                        if let (Some(ccp), Some(acb), Some(bcb), Some(ecb), Some(cpb), Some(cbp), Some(cbds), Some(cpc), Some(cd), Some(qs), Some(qwi)) = (c.create_cmd_pool, c.alloc_cmd_bufs, c.begin_cmd_buf, c.end_cmd_buf, c.cmd_pipeline_barrier, c.cmd_bind_pipe, c.cmd_bind_desc_sets, c.cmd_push_constants, c.cmd_dispatch, c.queue_submit, c.queue_wait_idle) {
                            if st.cmd_pool == vk::CommandPool::null() { let _ = (ccp)(dev, &vk::CommandPoolCreateInfo { s_type: vk::StructureType::COMMAND_POOL_CREATE_INFO, p_next: std::ptr::null(), flags: vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER, queue_family_index: qfi }, std::ptr::null(), &mut st.cmd_pool); }
                            if st.cmd_bufs.is_empty() { st.cmd_bufs.resize(st.proxy_images.len(), vk::CommandBuffer::null()); let _ = (acb)(dev, &vk::CommandBufferAllocateInfo { s_type: vk::StructureType::COMMAND_BUFFER_ALLOCATE_INFO, p_next: std::ptr::null(), command_pool: st.cmd_pool, level: vk::CommandBufferLevel::PRIMARY, command_buffer_count: st.proxy_images.len() as u32 }, st.cmd_bufs.as_mut_ptr()); }
                            let cb = st.cmd_bufs[ii];
                            let _ = (bcb)(cb, &vk::CommandBufferBeginInfo { s_type: vk::StructureType::COMMAND_BUFFER_BEGIN_INFO, p_next: std::ptr::null(), flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, p_inheritance_info: std::ptr::null() });
                            let sr = vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 };
                            (c.cmd_pipeline_barrier.unwrap())(cb, vk::PipelineStageFlags::ALL_COMMANDS, vk::PipelineStageFlags::COMPUTE_SHADER, vk::DependencyFlags::empty(), 0, std::ptr::null(), 0, std::ptr::null(), 2, [vk::ImageMemoryBarrier { s_type: vk::StructureType::IMAGE_MEMORY_BARRIER, p_next: std::ptr::null(), src_access_mask: vk::AccessFlags::MEMORY_WRITE, dst_access_mask: vk::AccessFlags::SHADER_READ, old_layout: vk::ImageLayout::PRESENT_SRC_KHR, new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL, src_queue_family_index: vk::QUEUE_FAMILY_IGNORED, dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED, image: st.proxy_images[ii], subresource_range: sr }, vk::ImageMemoryBarrier { s_type: vk::StructureType::IMAGE_MEMORY_BARRIER, p_next: std::ptr::null(), src_access_mask: vk::AccessFlags::empty(), dst_access_mask: vk::AccessFlags::SHADER_WRITE, old_layout: vk::ImageLayout::UNDEFINED, new_layout: vk::ImageLayout::GENERAL, src_queue_family_index: vk::QUEUE_FAMILY_IGNORED, dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED, image: st.real_images[ii], subresource_range: sr }].as_ptr());
                            (cbp)(cb, vk::PipelineBindPoint::COMPUTE, st.pipe);
                            (cbds)(cb, vk::PipelineBindPoint::COMPUTE, st.pipe_layout, 0, 1, &st.desc_sets[ii], 0, std::ptr::null());
                            (cpc)(cb, st.pipe_layout, vk::ShaderStageFlags::COMPUTE, 0, 32, &PushConstants { max_lum: HDR_CONFIG.max_lum, mid_lum: HDR_CONFIG.mid_lum, sat: HDR_CONFIG.sat, width: st.width, height: st.height, pad: [0; 3] } as *const _ as *const _);
                            (cd)(cb, (st.width + 7) / 8, (st.height + 7) / 8, 1);
                            (c.cmd_pipeline_barrier.unwrap())(cb, vk::PipelineStageFlags::COMPUTE_SHADER, vk::PipelineStageFlags::BOTTOM_OF_PIPE, vk::DependencyFlags::empty(), 0, std::ptr::null(), 0, std::ptr::null(), 2, [vk::ImageMemoryBarrier { s_type: vk::StructureType::IMAGE_MEMORY_BARRIER, p_next: std::ptr::null(), src_access_mask: vk::AccessFlags::SHADER_READ, dst_access_mask: vk::AccessFlags::MEMORY_READ, old_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL, new_layout: vk::ImageLayout::PRESENT_SRC_KHR, src_queue_family_index: vk::QUEUE_FAMILY_IGNORED, dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED, image: st.proxy_images[ii], subresource_range: sr }, vk::ImageMemoryBarrier { s_type: vk::StructureType::IMAGE_MEMORY_BARRIER, p_next: std::ptr::null(), src_access_mask: vk::AccessFlags::SHADER_WRITE, dst_access_mask: vk::AccessFlags::MEMORY_READ, old_layout: vk::ImageLayout::GENERAL, new_layout: vk::ImageLayout::PRESENT_SRC_KHR, src_queue_family_index: vk::QUEUE_FAMILY_IGNORED, dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED, image: st.real_images[ii], subresource_range: sr }].as_ptr());
                            let _ = (ecb)(cb);
                            let _ = (qs)(q, 1, &vk::SubmitInfo { s_type: vk::StructureType::SUBMIT_INFO, p_next: std::ptr::null(), wait_semaphore_count: 0, p_wait_semaphores: std::ptr::null(), p_wait_dst_stage_mask: std::ptr::null(), command_buffer_count: 1, p_command_buffers: &cb, signal_semaphore_count: 0, p_signal_semaphores: std::ptr::null() }, vk::Fence::null());
                            let _ = (qwi)(q);
                        }
                    }
                }
            }
            return (c.real_queue_present)(q, p_pi);
        }
    }
    vk::Result::SUCCESS
}
