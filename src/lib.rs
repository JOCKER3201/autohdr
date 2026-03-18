use ash::vk;
use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::sync::RwLock;
use std::collections::HashMap;

// ==========================================
// Struktury interfejsu warstwy (Loader API)
// ==========================================

#[repr(C)]
pub enum LayerNegotiateStructType {
    NegotiateLayerInterface = 2,
}

#[repr(C)]
pub struct NegotiateLayerInterface {
    pub s_type: LayerNegotiateStructType,
    pub p_next: *const c_void,
    pub loader_layer_interface_version: u32,
    pub pfn_get_instance_proc_addr: Option<unsafe extern "system" fn(vk::Instance, *const c_char) -> vk::PFN_vkVoidFunction>,
    pub pfn_get_device_proc_addr: Option<unsafe extern "system" fn(vk::Device, *const c_char) -> vk::PFN_vkVoidFunction>,
    pub pfn_get_physical_device_tool_properties: Option<unsafe extern "system" fn(vk::PhysicalDevice, *mut u32, *mut c_void) -> vk::Result>,
}

// Mechanizm łańcucha Vulkan
const VK_STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO: vk::StructureType = vk::StructureType::from_raw(47);
const VK_STRUCTURE_TYPE_LOADER_DEVICE_CREATE_INFO: vk::StructureType = vk::StructureType::from_raw(48);
const VK_LAYER_LINK_INFO: u32 = 0;

#[repr(C)]
pub struct VkLayerInstanceLink {
    pub p_next: *mut VkLayerInstanceLink,
    pub pfn_next_get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr,
    pub pfn_next_get_device_proc_addr: vk::PFN_vkGetDeviceProcAddr,
}

#[repr(C)]
pub struct VkLayerInstanceCreateInfo {
    pub s_type: vk::StructureType,
    pub p_next: *const c_void,
    pub function: u32,
    pub p_layer_info: *mut VkLayerInstanceLink,
}

#[repr(C)]
pub struct VkLayerDeviceLink {
    pub p_next: *mut VkLayerDeviceLink,
    pub pfn_next_get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr,
    pub pfn_next_get_device_proc_addr: vk::PFN_vkGetDeviceProcAddr,
}

#[repr(C)]
pub struct VkLayerDeviceCreateInfo {
    pub s_type: vk::StructureType,
    pub p_next: *const c_void,
    pub function: u32,
    pub p_layer_info: *mut VkLayerDeviceLink,
}

// Globalne tablice do przechowywania funkcji niższych warstw (dispatch tables)
lazy_static::lazy_static! {
    static ref INSTANCE_GIPA: RwLock<HashMap<vk::Instance, vk::PFN_vkGetInstanceProcAddr>> = RwLock::new(HashMap::new());
    static ref DEVICE_GDPA: RwLock<HashMap<vk::Device, vk::PFN_vkGetDeviceProcAddr>> = RwLock::new(HashMap::new());
    
    // Mapujemy kolejkę do urządzenia i indeksu rodziny kolejek
    static ref QUEUE_TO_DEVICE: RwLock<HashMap<vk::Queue, (vk::Device, u32)>> = RwLock::new(HashMap::new());

    // Przechowujemy też wskaźniki do oryginalnych funkcji, które podmieniamy
    static ref REAL_CREATE_SWAPCHAIN: RwLock<HashMap<vk::Device, vk::PFN_vkVoidFunction>> = RwLock::new(HashMap::new());
    static ref REAL_GET_SWAPCHAIN_IMAGES: RwLock<HashMap<vk::Device, vk::PFN_vkVoidFunction>> = RwLock::new(HashMap::new());
    static ref REAL_ACQUIRE_NEXT_IMAGE: RwLock<HashMap<vk::Device, vk::PFN_vkVoidFunction>> = RwLock::new(HashMap::new());
    static ref REAL_QUEUE_PRESENT: RwLock<HashMap<vk::Device, vk::PFN_vkVoidFunction>> = RwLock::new(HashMap::new());
    static ref REAL_GET_DEVICE_QUEUE: RwLock<HashMap<vk::Device, vk::PFN_vkVoidFunction>> = RwLock::new(HashMap::new());

    // Stan Swapchaina
    static ref SWAPCHAIN_STATES: RwLock<HashMap<vk::SwapchainKHR, SwapchainState>> = RwLock::new(HashMap::new());
    
    // Kontekst Urządzenia z potrzebnymi wskaźnikami funkcji do alokacji
    static ref DEVICE_CONTEXTS: RwLock<HashMap<vk::Device, DeviceContext>> = RwLock::new(HashMap::new());

    // Do uproszczonego zarządzania wskaźnikami (ponieważ vkCreateDevice nie podaje Instancji)
    static ref GLOBAL_INSTANCE: RwLock<Option<vk::Instance>> = RwLock::new(None);
}

pub struct DeviceContext {
    pub physical_device: vk::PhysicalDevice,
    pub pfn_get_physical_device_memory_properties: vk::PFN_vkGetPhysicalDeviceMemoryProperties,
    pub pfn_create_image: vk::PFN_vkCreateImage,
    pub pfn_get_image_memory_requirements: vk::PFN_vkGetImageMemoryRequirements,
    pub pfn_allocate_memory: vk::PFN_vkAllocateMemory,
    pub pfn_bind_image_memory: vk::PFN_vkBindImageMemory,
    
    // Command Buffer & Sync
    pub pfn_create_command_pool: vk::PFN_vkCreateCommandPool,
    pub pfn_allocate_command_buffers: vk::PFN_vkAllocateCommandBuffers,
    pub pfn_begin_command_buffer: vk::PFN_vkBeginCommandBuffer,
    pub pfn_end_command_buffer: vk::PFN_vkEndCommandBuffer,
    pub pfn_cmd_pipeline_barrier: vk::PFN_vkCmdPipelineBarrier,
    pub pfn_cmd_blit_image: vk::PFN_vkCmdBlitImage,
    pub pfn_queue_submit: vk::PFN_vkQueueSubmit,
    pub pfn_queue_wait_idle: vk::PFN_vkQueueWaitIdle,
    pub pfn_destroy_command_pool: vk::PFN_vkDestroyCommandPool,
}

// Struktura przechowująca informacje o Swapchainie
pub struct SwapchainState {
    pub device: vk::Device,
    pub width: u32,
    pub height: u32,
    pub app_requested_format: vk::Format,
    pub app_requested_usage: vk::ImageUsageFlags,
    pub real_hdr_format: vk::Format,
    
    pub real_images: Vec<vk::Image>,
    pub proxy_images: Vec<vk::Image>,
    pub proxy_memories: Vec<vk::DeviceMemory>,
    
    pub command_pool: vk::CommandPool,
    pub command_buffers: Vec<vk::CommandBuffer>,
}

// ==========================================
// 1. EKSPOZOWANE FUNKCJE C (Wymagane przez loader)
// ==========================================

#[no_mangle]
pub unsafe extern "system" fn vkNegotiateLoaderLayerInterfaceVersion(
    p_version_struct: *mut NegotiateLayerInterface,
) -> vk::Result {
    let interface = &mut *p_version_struct;
    
    match interface.s_type {
        LayerNegotiateStructType::NegotiateLayerInterface => {}
    }

    if interface.loader_layer_interface_version < 2 {
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    }
    
    interface.pfn_get_instance_proc_addr = Some(hook_get_instance_proc_addr);
    interface.pfn_get_device_proc_addr = Some(hook_get_device_proc_addr);
    interface.pfn_get_physical_device_tool_properties = None;
    
    vk::Result::SUCCESS
}

// ==========================================
// 2. PRZECHWYTYWANIE GET_PROC_ADDR
// ==========================================

unsafe extern "system" fn hook_get_instance_proc_addr(
    instance: vk::Instance,
    p_name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    if p_name.is_null() {
        return None;
    }
    let name = CStr::from_ptr(p_name).to_str().unwrap_or("");
    
    match name {
        "vkGetInstanceProcAddr" => Some(std::mem::transmute(hook_get_instance_proc_addr as *const ())),
        "vkGetDeviceProcAddr" => Some(std::mem::transmute(hook_get_device_proc_addr as *const ())),
        "vkCreateInstance" => Some(std::mem::transmute(hook_create_instance as *const ())),
        "vkCreateDevice" => Some(std::mem::transmute(hook_create_device as *const ())),
        "vkGetDeviceQueue" => Some(std::mem::transmute(hook_get_device_queue as *const ())),
        "vkCreateSwapchainKHR" => Some(std::mem::transmute(hook_create_swapchain_khr as *const ())),
        "vkGetSwapchainImagesKHR" => Some(std::mem::transmute(hook_get_swapchain_images_khr as *const ())),
        "vkAcquireNextImageKHR" => Some(std::mem::transmute(hook_acquire_next_image_khr as *const ())),
        "vkQueuePresentKHR" => Some(std::mem::transmute(hook_queue_present_khr as *const ())),
        _ => {
            if instance == vk::Instance::null() {
                return None;
            }
            if name == "vkGetDeviceQueue" {
                eprintln!("[Vulkan HDR Layer] get_instance_proc_addr pyta o vkGetDeviceQueue dla instancji: {:?}", instance);
            }
            // Zwróć oryginalną funkcję z kolejnej warstwy!
            let map = INSTANCE_GIPA.read().unwrap();
            if let Some(next_gipa) = map.get(&instance) {
                return next_gipa(instance, p_name);
            }
            None
        }
    }
}

unsafe extern "system" fn hook_get_device_proc_addr(
    device: vk::Device,
    p_name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    if p_name.is_null() {
        return None;
    }
    let name = CStr::from_ptr(p_name).to_str().unwrap_or("");
    match name {
        "vkGetDeviceProcAddr" => Some(std::mem::transmute(hook_get_device_proc_addr as *const ())),
        "vkGetDeviceQueue" => Some(std::mem::transmute(hook_get_device_queue as *const ())),
        "vkCreateSwapchainKHR" => Some(std::mem::transmute(hook_create_swapchain_khr as *const ())),
        "vkGetSwapchainImagesKHR" => Some(std::mem::transmute(hook_get_swapchain_images_khr as *const ())),
        "vkAcquireNextImageKHR" => Some(std::mem::transmute(hook_acquire_next_image_khr as *const ())),
        "vkQueuePresentKHR" => Some(std::mem::transmute(hook_queue_present_khr as *const ())),
        _ => {
            if name == "vkGetDeviceQueue" {
                eprintln!("[Vulkan HDR Layer] get_device_proc_addr pyta o vkGetDeviceQueue dla urzadzenia: {:?}", device);
            }
            let map = DEVICE_GDPA.read().unwrap();
            if let Some(next_gdpa) = map.get(&device) {
                return next_gdpa(device, p_name);
            }
            None
        }
    }
}

// ==========================================
// 3. WŁASNE FUNKCJE VULKANA (HOOKS)
// ==========================================

unsafe extern "system" fn hook_create_instance(
    p_create_info: *const vk::InstanceCreateInfo,
    p_allocator: *const vk::AllocationCallbacks,
    p_instance: *mut vk::Instance,
) -> vk::Result {
    eprintln!("[Vulkan HDR Layer] Wywołano hook_create_instance!");
    let mut layer_info = (*p_create_info).p_next as *mut VkLayerInstanceCreateInfo;
    
    // Szukamy LOADER_INSTANCE_CREATE_INFO
    while !layer_info.is_null() {
        if (*layer_info).s_type == VK_STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO && (*layer_info).function == VK_LAYER_LINK_INFO {
            break;
        }
        layer_info = (*layer_info).p_next as *mut VkLayerInstanceCreateInfo;
    }
    
    if layer_info.is_null() {
        eprintln!("[Vulkan HDR Layer] FATAL: Nie znaleziono VK_STRUCTURE_TYPE_LOADER_XXXX_CREATE_INFO (LINK_INFO)!");
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    }

    let layer_link = (*layer_info).p_layer_info;
    let next_gipa = (*layer_link).pfn_next_get_instance_proc_addr;

    // Przesuń łańcuch do kolejnej warstwy
    (*layer_info).p_layer_info = (*layer_link).p_next;

    let next_gipa_func = next_gipa(
        vk::Instance::null(),
        b"vkCreateInstance\0".as_ptr() as *const c_char,
    );

    if next_gipa_func.is_none() {
        eprintln!("[Vulkan HDR Layer] FATAL: next_gipa zwróciło None dla vkCreateInstance!");
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    }

    let real_create_instance: vk::PFN_vkCreateInstance = std::mem::transmute(next_gipa_func.unwrap());

    let res = real_create_instance(p_create_info, p_allocator, p_instance);

    if res == vk::Result::SUCCESS {
        INSTANCE_GIPA.write().unwrap().insert(*p_instance, next_gipa);
        *GLOBAL_INSTANCE.write().unwrap() = Some(*p_instance);
    }
    res
}

unsafe extern "system" fn hook_create_device(
    physical_device: vk::PhysicalDevice,
    p_create_info: *const vk::DeviceCreateInfo,
    p_allocator: *const vk::AllocationCallbacks,
    p_device: *mut vk::Device,
) -> vk::Result {
    eprintln!("[Vulkan HDR Layer] Wywołano hook_create_device!");
    let mut layer_info = (*p_create_info).p_next as *mut VkLayerDeviceCreateInfo;
    
    while !layer_info.is_null() {
        if (*layer_info).s_type == VK_STRUCTURE_TYPE_LOADER_DEVICE_CREATE_INFO && (*layer_info).function == VK_LAYER_LINK_INFO {
            break;
        }
        layer_info = (*layer_info).p_next as *mut VkLayerDeviceCreateInfo;
    }
    
    if layer_info.is_null() {
        eprintln!("[Vulkan HDR Layer] FATAL: Nie znaleziono VK_STRUCTURE_TYPE_LOADER_XXXX_CREATE_INFO (LINK_INFO)!");
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    }

    let layer_link = (*layer_info).p_layer_info;
    let next_gipa = (*layer_link).pfn_next_get_instance_proc_addr;
    let next_gdpa = (*layer_link).pfn_next_get_device_proc_addr;

    (*layer_info).p_layer_info = (*layer_link).p_next;

    let next_gipa_func = next_gipa(
        vk::Instance::null(),
        b"vkCreateDevice\0".as_ptr() as *const c_char,
    );
    
    if next_gipa_func.is_none() {
        eprintln!("[Vulkan HDR Layer] FATAL: next_gipa zwróciło None dla vkCreateDevice!");
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    }

    let real_create_device: vk::PFN_vkCreateDevice = std::mem::transmute(next_gipa_func.unwrap());

    eprintln!("[Vulkan HDR Layer] Próba wywołania real_create_device...");
    let res = real_create_device(physical_device, p_create_info, p_allocator, p_device);
    eprintln!("[Vulkan HDR Layer] Wywołano real_create_device, wynik: {:?}", res);

    if res == vk::Result::SUCCESS {
        eprintln!("[Vulkan HDR Layer] Pomyślnie utworzono urządzenie: {:?}", *p_device);
        DEVICE_GDPA.write().unwrap().insert(*p_device, next_gdpa);
        
        // Zapisz oryginalne wskaźniki dla naszych funkcji!
        let real_create_swapchain = next_gdpa(*p_device, b"vkCreateSwapchainKHR\0".as_ptr() as *const c_char);
        REAL_CREATE_SWAPCHAIN.write().unwrap().insert(*p_device, real_create_swapchain);
        
        let real_get_swapchain_images = next_gdpa(*p_device, b"vkGetSwapchainImagesKHR\0".as_ptr() as *const c_char);
        REAL_GET_SWAPCHAIN_IMAGES.write().unwrap().insert(*p_device, real_get_swapchain_images);

        let real_acquire_next_image = next_gdpa(*p_device, b"vkAcquireNextImageKHR\0".as_ptr() as *const c_char);
        REAL_ACQUIRE_NEXT_IMAGE.write().unwrap().insert(*p_device, real_acquire_next_image);
        
        let real_queue_present = next_gdpa(*p_device, b"vkQueuePresentKHR\0".as_ptr() as *const c_char);
        REAL_QUEUE_PRESENT.write().unwrap().insert(*p_device, real_queue_present);
        
        let real_get_device_queue = next_gdpa(*p_device, b"vkGetDeviceQueue\0".as_ptr() as *const c_char);
        REAL_GET_DEVICE_QUEUE.write().unwrap().insert(*p_device, real_get_device_queue);
        
        // Zapisujemy wskaźniki do tworzenia i alokacji obrazków dla Proxy Images
        let instance = GLOBAL_INSTANCE.read().unwrap().unwrap();
        let pfn_get_physical_device_memory_properties: vk::PFN_vkGetPhysicalDeviceMemoryProperties = std::mem::transmute(
            next_gipa(instance, b"vkGetPhysicalDeviceMemoryProperties\0".as_ptr() as *const c_char).unwrap()
        );
        let pfn_create_image: vk::PFN_vkCreateImage = std::mem::transmute(next_gdpa(*p_device, b"vkCreateImage\0".as_ptr() as *const c_char).unwrap());
        let pfn_get_image_memory_requirements: vk::PFN_vkGetImageMemoryRequirements = std::mem::transmute(next_gdpa(*p_device, b"vkGetImageMemoryRequirements\0".as_ptr() as *const c_char).unwrap());
        let pfn_allocate_memory: vk::PFN_vkAllocateMemory = std::mem::transmute(next_gdpa(*p_device, b"vkAllocateMemory\0".as_ptr() as *const c_char).unwrap());
        let pfn_bind_image_memory: vk::PFN_vkBindImageMemory = std::mem::transmute(next_gdpa(*p_device, b"vkBindImageMemory\0".as_ptr() as *const c_char).unwrap());

        let pfn_create_command_pool: vk::PFN_vkCreateCommandPool = std::mem::transmute(next_gdpa(*p_device, b"vkCreateCommandPool\0".as_ptr() as *const c_char).unwrap());
        let pfn_allocate_command_buffers: vk::PFN_vkAllocateCommandBuffers = std::mem::transmute(next_gdpa(*p_device, b"vkAllocateCommandBuffers\0".as_ptr() as *const c_char).unwrap());
        let pfn_begin_command_buffer: vk::PFN_vkBeginCommandBuffer = std::mem::transmute(next_gdpa(*p_device, b"vkBeginCommandBuffer\0".as_ptr() as *const c_char).unwrap());
        let pfn_end_command_buffer: vk::PFN_vkEndCommandBuffer = std::mem::transmute(next_gdpa(*p_device, b"vkEndCommandBuffer\0".as_ptr() as *const c_char).unwrap());
        let pfn_cmd_pipeline_barrier: vk::PFN_vkCmdPipelineBarrier = std::mem::transmute(next_gdpa(*p_device, b"vkCmdPipelineBarrier\0".as_ptr() as *const c_char).unwrap());
        let pfn_cmd_blit_image: vk::PFN_vkCmdBlitImage = std::mem::transmute(next_gdpa(*p_device, b"vkCmdBlitImage\0".as_ptr() as *const c_char).unwrap());
        let pfn_queue_submit: vk::PFN_vkQueueSubmit = std::mem::transmute(next_gdpa(*p_device, b"vkQueueSubmit\0".as_ptr() as *const c_char).unwrap());
        let pfn_queue_wait_idle: vk::PFN_vkQueueWaitIdle = std::mem::transmute(next_gdpa(*p_device, b"vkQueueWaitIdle\0".as_ptr() as *const c_char).unwrap());
        let pfn_destroy_command_pool: vk::PFN_vkDestroyCommandPool = std::mem::transmute(next_gdpa(*p_device, b"vkDestroyCommandPool\0".as_ptr() as *const c_char).unwrap());

        let context = DeviceContext {
            physical_device,
            pfn_get_physical_device_memory_properties,
            pfn_create_image,
            pfn_get_image_memory_requirements,
            pfn_allocate_memory,
            pfn_bind_image_memory,
            pfn_create_command_pool,
            pfn_allocate_command_buffers,
            pfn_begin_command_buffer,
            pfn_end_command_buffer,
            pfn_cmd_pipeline_barrier,
            pfn_cmd_blit_image,
            pfn_queue_submit,
            pfn_queue_wait_idle,
            pfn_destroy_command_pool,
        };
        DEVICE_CONTEXTS.write().unwrap().insert(*p_device, context);
    } else {
        eprintln!("[Vulkan HDR Layer] Błąd tworzenia urządzenia: {:?}", res);
    }
    res
}

unsafe extern "system" fn hook_get_device_queue(
    device: vk::Device,
    queue_family_index: u32,
    queue_index: u32,
    p_queue: *mut vk::Queue,
) {
    let map = REAL_GET_DEVICE_QUEUE.read().unwrap();
    if let Some(real_func_opt) = map.get(&device) {
        if let Some(real_func) = real_func_opt {
            let real_get_device_queue: vk::PFN_vkGetDeviceQueue = std::mem::transmute(*real_func);
            real_get_device_queue(device, queue_family_index, queue_index, p_queue);
            
            // Map the retrieved queue to the device
            if !p_queue.is_null() && *p_queue != vk::Queue::null() {
                QUEUE_TO_DEVICE.write().unwrap().insert(*p_queue, (device, queue_family_index));
            }
        }
    }
}

unsafe extern "system" fn hook_create_swapchain_khr(
    device: vk::Device,
    p_create_info: *const vk::SwapchainCreateInfoKHR,
    p_allocator: *const vk::AllocationCallbacks,
    p_swapchain: *mut vk::SwapchainKHR,
) -> vk::Result {
    let mut modified_info = *p_create_info;
    let requested_format = modified_info.image_format;

    // Ponownie wymuszamy format HDR, ponieważ teraz tworzymy Proxy Images
    modified_info.image_format = vk::Format::R16G16B16A16_SFLOAT;
    modified_info.image_color_space = vk::ColorSpaceKHR::HDR10_ST2084_EXT; 
    
    // Dodajemy flagę TRANSFER_DST, by móc kopiować na ten obraz
    modified_info.image_usage |= vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::STORAGE;

    eprintln!("[Vulkan HDR Layer] Aplikacja zażądała Swapchaina SDR: {:?}, Wymuszono format HDR 16-bit!", requested_format);

    // Wywołaj oryginalną funkcję
    let map = REAL_CREATE_SWAPCHAIN.read().unwrap();
    if let Some(real_func) = map.get(&device) {
        if let Some(func) = real_func {
            let real_create_swapchain: vk::PFN_vkCreateSwapchainKHR = std::mem::transmute(*func);
            let result = real_create_swapchain(device, &modified_info, p_allocator, p_swapchain);
            
            if result == vk::Result::SUCCESS {
                // Inicjalizujemy stan nowego Swapchaina z myślą o Proxy Images
                let state = SwapchainState {
                    device,
                    width: modified_info.image_extent.width,
                    height: modified_info.image_extent.height,
                    app_requested_format: requested_format,
                    app_requested_usage: modified_info.image_usage,
                    real_hdr_format: modified_info.image_format,
                    real_images: Vec::new(),
                    proxy_images: Vec::new(),
                    proxy_memories: Vec::new(),
                    command_pool: vk::CommandPool::null(),
                    command_buffers: Vec::new(),
                };
                SWAPCHAIN_STATES.write().unwrap().insert(*p_swapchain, state);
            }
            return result;
        }
    }
    vk::Result::ERROR_INITIALIZATION_FAILED
}

// Funkcja pomocnicza do szukania odpowiedniej pamięci
fn find_memory_type(
    memory_type_bits: u32,
    property_flags: vk::MemoryPropertyFlags,
    device_context: &DeviceContext,
) -> Option<u32> {
    let mut mem_props = vk::PhysicalDeviceMemoryProperties::default();
    unsafe {
        (device_context.pfn_get_physical_device_memory_properties)(
            device_context.physical_device,
            &mut mem_props,
        );
    }

    for i in 0..mem_props.memory_type_count {
        if (memory_type_bits & (1 << i)) != 0
            && mem_props.memory_types[i as usize].property_flags.contains(property_flags)
        {
            return Some(i);
        }
    }
    None
}

unsafe extern "system" fn hook_get_swapchain_images_khr(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    p_swapchain_image_count: *mut u32,
    p_swapchain_images: *mut vk::Image,
) -> vk::Result {
    // Najpierw pobieramy prawdziwe obrazy HDR z warstwy pod nami
    let map = REAL_GET_SWAPCHAIN_IMAGES.read().unwrap();
    if let Some(real_func_opt) = map.get(&device) {
        if let Some(real_func) = real_func_opt {
            let real_get_swapchain_images: vk::PFN_vkGetSwapchainImagesKHR = std::mem::transmute(*real_func);
            let res = real_get_swapchain_images(device, swapchain, p_swapchain_image_count, p_swapchain_images);
            
            if res == vk::Result::SUCCESS || res == vk::Result::INCOMPLETE {
                if p_swapchain_images.is_null() {
                    // Gra tylko pyta, ile jest obrazów
                    eprintln!("[Vulkan HDR Layer] Aplikacja pyta o ilość obrazów w Swapchainie: {}", *p_swapchain_image_count);
                } else {
                    // Gra prosi o same obrazy
                    let count = *p_swapchain_image_count as usize;
                    eprintln!("[Vulkan HDR Layer] Aplikacja pobiera {} obrazów ze Swapchaina! Podmieniamy na SDR Proxy...", count);
                    
                    let mut state_map = SWAPCHAIN_STATES.write().unwrap();
                    if let Some(state) = state_map.get_mut(&swapchain) {
                        // Pobierzmy kontekst urządzenia do alokacji
                        let dev_ctx_map = DEVICE_CONTEXTS.read().unwrap();
                        let dev_ctx = dev_ctx_map.get(&device).unwrap();

                        // Zapamiętajmy prawdziwe obrazy z drivera
                        state.real_images.clear();
                        for i in 0..count {
                            state.real_images.push(*p_swapchain_images.add(i));
                        }

                        // Jeśli nie stworzyliśmy jeszcze Proxy Images, tworzymy je teraz
                        if state.proxy_images.is_empty() {
                            for _ in 0..count {
                                // 1. Definicja Proxy Image (SDR)
                                let create_info = vk::ImageCreateInfo {
                                    s_type: vk::StructureType::IMAGE_CREATE_INFO,
                                    p_next: std::ptr::null(),
                                    flags: vk::ImageCreateFlags::empty(),
                                    image_type: vk::ImageType::TYPE_2D,
                                    format: state.app_requested_format, // Format SDR
                                    extent: vk::Extent3D {
                                        width: state.width,
                                        height: state.height,
                                        depth: 1,
                                    },
                                    mip_levels: 1,
                                    array_layers: 1,
                                    samples: vk::SampleCountFlags::TYPE_1,
                                    tiling: vk::ImageTiling::OPTIMAL,
                                    // Użycie jako Attachment i jako wejście do Compute Shadera
                                    usage: state.app_requested_usage | vk::ImageUsageFlags::TRANSFER_SRC,
                                    sharing_mode: vk::SharingMode::EXCLUSIVE,
                                    queue_family_index_count: 0,
                                    p_queue_family_indices: std::ptr::null(),
                                    initial_layout: vk::ImageLayout::UNDEFINED,
                                };

                                let mut proxy_image = vk::Image::null();
                                if (dev_ctx.pfn_create_image)(device, &create_info, std::ptr::null(), &mut proxy_image) != vk::Result::SUCCESS {
                                    eprintln!("Błąd tworzenia Proxy Image!");
                                    return vk::Result::ERROR_INITIALIZATION_FAILED;
                                }

                                // 2. Alokacja Pamięci
                                let mut mem_reqs = vk::MemoryRequirements::default();
                                (dev_ctx.pfn_get_image_memory_requirements)(device, proxy_image, &mut mem_reqs);

                                let mem_type_idx = find_memory_type(
                                    mem_reqs.memory_type_bits,
                                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                                    dev_ctx,
                                ).unwrap_or(0);

                                let alloc_info = vk::MemoryAllocateInfo {
                                    s_type: vk::StructureType::MEMORY_ALLOCATE_INFO,
                                    p_next: std::ptr::null(),
                                    allocation_size: mem_reqs.size,
                                    memory_type_index: mem_type_idx,
                                };

                                let mut proxy_memory = vk::DeviceMemory::null();
                                if (dev_ctx.pfn_allocate_memory)(device, &alloc_info, std::ptr::null(), &mut proxy_memory) != vk::Result::SUCCESS {
                                    eprintln!("Błąd alokacji pamięci dla Proxy Image!");
                                    return vk::Result::ERROR_INITIALIZATION_FAILED;
                                }

                                // 3. Podpięcie pamięci
                                if (dev_ctx.pfn_bind_image_memory)(device, proxy_image, proxy_memory, 0) != vk::Result::SUCCESS {
                                    eprintln!("Błąd bindowania pamięci dla Proxy Image!");
                                    return vk::Result::ERROR_INITIALIZATION_FAILED;
                                }

                                state.proxy_images.push(proxy_image);
                                state.proxy_memories.push(proxy_memory);
                            }
                        }

                        // Wymieniamy (podmieniamy) uchwyty, aby Gra rysowała do naszego Proxy Image!
                        for i in 0..count {
                            *p_swapchain_images.add(i) = state.proxy_images[i];
                        }
                    }
                }
            }
            return res;
        }
    }
    vk::Result::ERROR_INITIALIZATION_FAILED
}

unsafe extern "system" fn hook_acquire_next_image_khr(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    timeout: u64,
    semaphore: vk::Semaphore,
    fence: vk::Fence,
    p_image_index: *mut u32,
) -> vk::Result {
    let map = REAL_ACQUIRE_NEXT_IMAGE.read().unwrap();
    if let Some(real_func_opt) = map.get(&device) {
        if let Some(real_func) = real_func_opt {
            let real_acquire_next_image: vk::PFN_vkAcquireNextImageKHR = std::mem::transmute(*real_func);
            let res = real_acquire_next_image(device, swapchain, timeout, semaphore, fence, p_image_index);
            if res != vk::Result::SUCCESS && res != vk::Result::SUBOPTIMAL_KHR {
                eprintln!("[Vulkan HDR Layer] vkAcquireNextImageKHR zwróciło błąd: {:?}", res);
            }
            return res;
        }
    }
    vk::Result::ERROR_INITIALIZATION_FAILED
}

unsafe extern "system" fn hook_queue_present_khr(
    queue: vk::Queue,
    p_present_info: *const vk::PresentInfoKHR,
) -> vk::Result {
    // Odnajdujemy urządzenie powiązane z tą kolejką
    let (device, queue_family_index) = {
        let map = QUEUE_TO_DEVICE.read().unwrap();
        map.get(&queue).copied().unwrap_or((vk::Device::null(), 0))
    };

    if device != vk::Device::null() {
        let present_info = unsafe { &*p_present_info };
        
        let dev_ctx_map = DEVICE_CONTEXTS.read().unwrap();
        if let Some(dev_ctx) = dev_ctx_map.get(&device) {

            // Prosta synchronizacja dla dema
            unsafe { let _ = (dev_ctx.pfn_queue_wait_idle)(queue); }

            let mut state_map = SWAPCHAIN_STATES.write().unwrap();

            unsafe {
                for i in 0..present_info.swapchain_count as usize {
                    let swapchain = *present_info.p_swapchains.add(i);
                    let image_index = *present_info.p_image_indices.add(i) as usize;

                    if let Some(state) = state_map.get_mut(&swapchain) {
                        if !state.proxy_images.is_empty() && image_index < state.proxy_images.len() {
                            
                            // 1. Utworzenie Command Pool
                            if state.command_pool == vk::CommandPool::null() {
                                let pool_info = vk::CommandPoolCreateInfo {
                                    s_type: vk::StructureType::COMMAND_POOL_CREATE_INFO,
                                    p_next: std::ptr::null(),
                                    flags: vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
                                    queue_family_index,
                                };
                                (dev_ctx.pfn_create_command_pool)(device, &pool_info, std::ptr::null(), &mut state.command_pool);
                            }

                            // 2. Alokacja buforów
                            if state.command_buffers.is_empty() {
                                let alloc_info = vk::CommandBufferAllocateInfo {
                                    s_type: vk::StructureType::COMMAND_BUFFER_ALLOCATE_INFO,
                                    p_next: std::ptr::null(),
                                    command_pool: state.command_pool,
                                    level: vk::CommandBufferLevel::PRIMARY,
                                    command_buffer_count: state.proxy_images.len() as u32,
                                };
                                state.command_buffers.resize(state.proxy_images.len(), vk::CommandBuffer::null());
                                (dev_ctx.pfn_allocate_command_buffers)(device, &alloc_info, state.command_buffers.as_mut_ptr());
                            }

                            let cmd_buf = state.command_buffers[image_index];
                            let proxy_img = state.proxy_images[image_index];
                            let real_img = state.real_images[image_index];

                            // 3. Rozpoczęcie nagrywania
                            let begin_info = vk::CommandBufferBeginInfo {
                                s_type: vk::StructureType::COMMAND_BUFFER_BEGIN_INFO,
                                p_next: std::ptr::null(),
                                flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
                                p_inheritance_info: std::ptr::null(),
                            };
                            (dev_ctx.pfn_begin_command_buffer)(cmd_buf, &begin_info);

                            let subresource_range = vk::ImageSubresourceRange {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                base_mip_level: 0,
                                level_count: 1,
                                base_array_layer: 0,
                                layer_count: 1,
                            };

                            // 4. Bariery do transferu
                            let proxy_to_src = vk::ImageMemoryBarrier {
                                s_type: vk::StructureType::IMAGE_MEMORY_BARRIER,
                                p_next: std::ptr::null(),
                                src_access_mask: vk::AccessFlags::MEMORY_WRITE,
                                dst_access_mask: vk::AccessFlags::TRANSFER_READ,
                                old_layout: vk::ImageLayout::PRESENT_SRC_KHR,
                                new_layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                                src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                image: proxy_img,
                                subresource_range,
                            };

                            let real_to_dst = vk::ImageMemoryBarrier {
                                s_type: vk::StructureType::IMAGE_MEMORY_BARRIER,
                                p_next: std::ptr::null(),
                                src_access_mask: vk::AccessFlags::empty(),
                                dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                                old_layout: vk::ImageLayout::UNDEFINED,
                                new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                image: real_img,
                                subresource_range,
                            };

                            let barriers = [proxy_to_src, real_to_dst];
                            (dev_ctx.pfn_cmd_pipeline_barrier)(
                                cmd_buf,
                                vk::PipelineStageFlags::ALL_COMMANDS,
                                vk::PipelineStageFlags::TRANSFER,
                                vk::DependencyFlags::empty(),
                                0, std::ptr::null(),
                                0, std::ptr::null(),
                                2, barriers.as_ptr(),
                            );

                            // 5. Blit (Kopiowanie z SDR -> HDR)
                            let blit_region = vk::ImageBlit {
                                src_subresource: vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: 0, base_array_layer: 0, layer_count: 1 },
                                src_offsets: [
                                    vk::Offset3D { x: 0, y: 0, z: 0 },
                                    vk::Offset3D { x: state.width as i32, y: state.height as i32, z: 1 },
                                ],
                                dst_subresource: vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: 0, base_array_layer: 0, layer_count: 1 },
                                dst_offsets: [
                                    vk::Offset3D { x: 0, y: 0, z: 0 },
                                    vk::Offset3D { x: state.width as i32, y: state.height as i32, z: 1 },
                                ],
                            };

                            (dev_ctx.pfn_cmd_blit_image)(
                                cmd_buf,
                                proxy_img, vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                                real_img, vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                1, &blit_region,
                                vk::Filter::LINEAR,
                            );

                            // 6. Powrót barier
                            let proxy_to_present = vk::ImageMemoryBarrier {
                                s_type: vk::StructureType::IMAGE_MEMORY_BARRIER,
                                p_next: std::ptr::null(),
                                src_access_mask: vk::AccessFlags::TRANSFER_READ,
                                dst_access_mask: vk::AccessFlags::MEMORY_READ,
                                old_layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                                new_layout: vk::ImageLayout::PRESENT_SRC_KHR,
                                src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                image: proxy_img,
                                subresource_range,
                            };

                            let real_to_present = vk::ImageMemoryBarrier {
                                s_type: vk::StructureType::IMAGE_MEMORY_BARRIER,
                                p_next: std::ptr::null(),
                                src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                                dst_access_mask: vk::AccessFlags::MEMORY_READ,
                                old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                new_layout: vk::ImageLayout::PRESENT_SRC_KHR,
                                src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                image: real_img,
                                subresource_range,
                            };

                            let end_barriers = [proxy_to_present, real_to_present];
                            (dev_ctx.pfn_cmd_pipeline_barrier)(
                                cmd_buf,
                                vk::PipelineStageFlags::TRANSFER,
                                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                                vk::DependencyFlags::empty(),
                                0, std::ptr::null(),
                                0, std::ptr::null(),
                                2, end_barriers.as_ptr(),
                            );

                            (dev_ctx.pfn_end_command_buffer)(cmd_buf);

                            // 7. Submission
                            let submit_info = vk::SubmitInfo {
                                s_type: vk::StructureType::SUBMIT_INFO,
                                p_next: std::ptr::null(),
                                wait_semaphore_count: 0,
                                p_wait_semaphores: std::ptr::null(),
                                p_wait_dst_stage_mask: std::ptr::null(),
                                command_buffer_count: 1,
                                p_command_buffers: &cmd_buf,
                                signal_semaphore_count: 0,
                                p_signal_semaphores: std::ptr::null(),
                            };

                            let _ = (dev_ctx.pfn_queue_submit)(queue, 1, &submit_info, vk::Fence::null());
                            let _ = (dev_ctx.pfn_queue_wait_idle)(queue);
                        }
                    }
                }
            }
        }

        // Zlecamy Prawdziwą Prezentację (HDR) dla oryginalnej funkcji sterownika
        let map = REAL_QUEUE_PRESENT.read().unwrap();
        if let Some(real_func_opt) = map.get(&device) {
            if let Some(real_func) = real_func_opt {
                let real_queue_present: vk::PFN_vkQueuePresentKHR = std::mem::transmute(*real_func);
                let res = real_queue_present(queue, p_present_info);
                if res != vk::Result::SUCCESS && res != vk::Result::SUBOPTIMAL_KHR {
                    eprintln!("[Vulkan HDR Layer] real_queue_present zwróciło błąd: {:?}", res);
                }
                return res;
            }
        }
    } else {
        eprintln!("[Vulkan HDR Layer] OSTRZEŻENIE: Nie znaleziono urządzenia dla kolejki w hook_queue_present_khr!");
    }
    
    vk::Result::ERROR_INITIALIZATION_FAILED
}
