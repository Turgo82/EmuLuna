/* SPDX-License-Identifier: MIT */
#include "vulkan_host.h"
#include <dlfcn.h>
#include <algorithm>
#include <cstdio>
#include <cstring>
#include <type_traits>

namespace {
std::string failure(const char *operation, VkResult result) {
    return std::string(operation) + " failed (Vulkan error " + std::to_string(result) + ").";
}
}

VulkanHost::VulkanHost() {
    interface_.interface_type = RETRO_HW_RENDER_INTERFACE_VULKAN;
    interface_.interface_version = RETRO_HW_RENDER_INTERFACE_VULKAN_VERSION;
    interface_.handle = this;
    interface_.set_image = set_image;
    interface_.get_sync_index = sync_index;
    interface_.get_sync_index_mask = sync_mask;
    interface_.set_command_buffers = set_command_buffers;
    interface_.wait_sync_index = wait_sync;
    interface_.lock_queue = lock_queue;
    interface_.unlock_queue = unlock_queue;
    interface_.set_signal_semaphore = set_signal_semaphore;
}

VulkanHost::~VulkanHost() { close(); }

bool VulkanHost::available() {
    VulkanHost probe;
    std::string error;
    return probe.initialize(error);
}

bool VulkanHost::set_negotiation(const retro_hw_render_context_negotiation_interface *requested) {
    if (!requested || requested->interface_type != RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN ||
        requested->interface_version < 1) return false;
    const auto *vulkan = reinterpret_cast<const retro_hw_render_context_negotiation_interface_vulkan*>(requested);
    negotiation_.interface_type = vulkan->interface_type;
    negotiation_.interface_version = 1; // The v1 device callback is sufficient for Beetle PSX HW.
    negotiation_.get_application_info = vulkan->get_application_info;
    negotiation_.create_device = vulkan->create_device;
    negotiation_.destroy_device = vulkan->destroy_device;
    return true;
}

bool VulkanHost::load_loader(std::string &error) {
    loader_ = dlopen("libvulkan.so.1", RTLD_NOW | RTLD_LOCAL);
    if (!loader_) { error = "The Vulkan loader is unavailable."; return false; }
    get_instance_proc_ = reinterpret_cast<PFN_vkGetInstanceProcAddr>(dlsym(loader_, "vkGetInstanceProcAddr"));
    if (!get_instance_proc_) { error = "The Vulkan loader has no instance entry point."; return false; }
    create_instance_ = reinterpret_cast<PFN_vkCreateInstance>(get_instance_proc_(VK_NULL_HANDLE, "vkCreateInstance"));
    if (!create_instance_) { error = "The Vulkan loader cannot create an instance."; return false; }
    return true;
}

bool VulkanHost::initialize(std::string &error) {
    if (instance_ || !load_loader(error)) return instance_ != VK_NULL_HANDLE;
    const VkApplicationInfo fallback{VK_STRUCTURE_TYPE_APPLICATION_INFO, nullptr,
        "EmuLuna", 1, "EmuLuna libretro host", 1, VK_API_VERSION_1_1};
    const VkApplicationInfo *app = negotiation_.get_application_info ?
        negotiation_.get_application_info() : nullptr;
    if (!app) app = &fallback;
    VkInstanceCreateInfo instance_info{VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO};
    instance_info.pApplicationInfo = app;
    VkResult result = create_instance_(&instance_info, nullptr, &instance_);
    if (result != VK_SUCCESS) { error = failure("Creating the Vulkan instance", result); close(); return false; }

    auto load_instance = [&](auto &entry, const char *name) {
        using Type = typename std::remove_reference<decltype(entry)>::type;
        entry = reinterpret_cast<Type>(get_instance_proc_(instance_, name));
        if (!entry && error.empty()) error = std::string("Missing Vulkan function: ") + name;
    };
    load_instance(destroy_instance_, "vkDestroyInstance");
    load_instance(enumerate_devices_, "vkEnumeratePhysicalDevices");
    load_instance(queue_families_, "vkGetPhysicalDeviceQueueFamilyProperties");
    load_instance(memory_properties_, "vkGetPhysicalDeviceMemoryProperties");
    load_instance(create_device_, "vkCreateDevice");
    load_instance(get_device_proc_, "vkGetDeviceProcAddr");
    if (!error.empty()) { close(); return false; }

    uint32_t device_count = 0;
    result = enumerate_devices_(instance_, &device_count, nullptr);
    if (result != VK_SUCCESS || !device_count) {
        error = result == VK_SUCCESS ? "No Vulkan graphics device is available." :
            failure("Enumerating Vulkan devices", result);
        close(); return false;
    }
    std::vector<VkPhysicalDevice> devices(device_count);
    result = enumerate_devices_(instance_, &device_count, devices.data());
    if (result != VK_SUCCESS) { error = failure("Enumerating Vulkan devices", result); close(); return false; }
    for (auto device : devices) {
        uint32_t family_count = 0;
        queue_families_(device, &family_count, nullptr);
        std::vector<VkQueueFamilyProperties> families(family_count);
        queue_families_(device, &family_count, families.data());
        for (uint32_t i = 0; i < family_count; ++i) {
            if ((families[i].queueFlags & (VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT)) ==
                (VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT) && families[i].queueCount) {
                gpu_ = device; queue_family_ = i; break;
            }
        }
        if (gpu_) break;
    }
    if (!gpu_) { error = "No Vulkan graphics and compute queue is available."; close(); return false; }

    if (negotiation_.create_device) {
        retro_vulkan_context context{};
        if (negotiation_.create_device(&context, instance_, gpu_, VK_NULL_HANDLE,
                                       get_instance_proc_, nullptr, 0, nullptr, 0, nullptr)) {
            gpu_ = context.gpu;
            device_ = context.device;
            queue_ = context.queue;
            queue_family_ = context.queue_family_index;
            core_created_device_ = true;
        }
    }
    if (!device_) {
        float priority = 1.0f;
        VkDeviceQueueCreateInfo queue_info{VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO};
        queue_info.queueFamilyIndex = queue_family_;
        queue_info.queueCount = 1;
        queue_info.pQueuePriorities = &priority;
        VkDeviceCreateInfo device_info{VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO};
        device_info.queueCreateInfoCount = 1;
        device_info.pQueueCreateInfos = &queue_info;
        result = create_device_(gpu_, &device_info, nullptr, &device_);
        if (result != VK_SUCCESS) { error = failure("Creating the Vulkan device", result); close(); return false; }
    }

    auto load_device = [&](auto &entry, const char *name) {
        using Type = typename std::remove_reference<decltype(entry)>::type;
        entry = reinterpret_cast<Type>(get_device_proc_(device_, name));
        if (!entry && error.empty()) error = std::string("Missing Vulkan function: ") + name;
    };
    load_device(destroy_device_, "vkDestroyDevice");
    load_device(get_queue_, "vkGetDeviceQueue");
    load_device(create_pool_, "vkCreateCommandPool");
    load_device(destroy_pool_, "vkDestroyCommandPool");
    load_device(allocate_commands_, "vkAllocateCommandBuffers");
    load_device(free_commands_, "vkFreeCommandBuffers");
    load_device(begin_command_, "vkBeginCommandBuffer");
    load_device(end_command_, "vkEndCommandBuffer");
    load_device(barrier_, "vkCmdPipelineBarrier");
    load_device(copy_image_, "vkCmdCopyImageToBuffer");
    load_device(submit_, "vkQueueSubmit");
    load_device(wait_idle_, "vkQueueWaitIdle");
    load_device(create_buffer_, "vkCreateBuffer");
    load_device(destroy_buffer_, "vkDestroyBuffer");
    load_device(buffer_requirements_, "vkGetBufferMemoryRequirements");
    load_device(allocate_memory_, "vkAllocateMemory");
    load_device(free_memory_, "vkFreeMemory");
    load_device(bind_buffer_, "vkBindBufferMemory");
    load_device(map_memory_, "vkMapMemory");
    load_device(unmap_memory_, "vkUnmapMemory");
    load_device(invalidate_memory_, "vkInvalidateMappedMemoryRanges");
    if (!error.empty()) { close(); return false; }
    if (!queue_) get_queue_(device_, queue_family_, 0, &queue_);
    if (!queue_) { error = "The Vulkan device did not provide a graphics queue."; close(); return false; }
    VkCommandPoolCreateInfo pool_info{VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO};
    pool_info.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
    pool_info.queueFamilyIndex = queue_family_;
    result = create_pool_(device_, &pool_info, nullptr, &pool_);
    if (result != VK_SUCCESS) { error = failure("Creating the Vulkan command pool", result); close(); return false; }

    interface_.instance = instance_;
    interface_.gpu = gpu_;
    interface_.device = device_;
    interface_.get_instance_proc_addr = get_instance_proc_;
    interface_.get_device_proc_addr = get_device_proc_;
    interface_.queue = queue_;
    interface_.queue_index = queue_family_;
    return true;
}

bool VulkanHost::staging(size_t bytes, std::string &error) {
    if (buffer_ && buffer_size_ >= bytes) return true;
    if (buffer_) { destroy_buffer_(device_, buffer_, nullptr); buffer_ = VK_NULL_HANDLE; }
    if (memory_) { free_memory_(device_, memory_, nullptr); memory_ = VK_NULL_HANDLE; }
    buffer_size_ = 0;
    VkBufferCreateInfo buffer_info{VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO};
    buffer_info.size = bytes;
    buffer_info.usage = VK_BUFFER_USAGE_TRANSFER_DST_BIT;
    buffer_info.sharingMode = VK_SHARING_MODE_EXCLUSIVE;
    VkResult result = create_buffer_(device_, &buffer_info, nullptr, &buffer_);
    if (result != VK_SUCCESS) { error = failure("Creating the Vulkan readback buffer", result); return false; }
    VkMemoryRequirements requirements{};
    buffer_requirements_(device_, buffer_, &requirements);
    VkPhysicalDeviceMemoryProperties properties{};
    memory_properties_(gpu_, &properties);
    uint32_t type = UINT32_MAX;
    int best_score = -1;
    for (uint32_t i = 0; i < properties.memoryTypeCount; ++i) {
        auto flags = properties.memoryTypes[i].propertyFlags;
        if ((requirements.memoryTypeBits & (1u << i)) && (flags & VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT)) {
            // This buffer is read by the CPU every frame. Coherent VRAM is
            // often uncached on discrete GPUs, making even small frames slow
            // to unpack. Prefer cached host memory and invalidate it below
            // when it is not coherent.
            const int score = ((flags & VK_MEMORY_PROPERTY_HOST_CACHED_BIT) ? 2 : 0) +
                              ((flags & VK_MEMORY_PROPERTY_HOST_COHERENT_BIT) ? 1 : 0);
            if (score > best_score) { type = i; best_score = score; }
        }
    }
    if (type == UINT32_MAX) { error = "The Vulkan GPU has no readable staging memory."; return false; }
    coherent_ = (properties.memoryTypes[type].propertyFlags & VK_MEMORY_PROPERTY_HOST_COHERENT_BIT) != 0;
    VkMemoryAllocateInfo allocation{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO};
    allocation.allocationSize = requirements.size;
    allocation.memoryTypeIndex = type;
    result = allocate_memory_(device_, &allocation, nullptr, &memory_);
    if (result != VK_SUCCESS) { error = failure("Allocating Vulkan readback memory", result); return false; }
    result = bind_buffer_(device_, buffer_, memory_, 0);
    if (result != VK_SUCCESS) { error = failure("Binding Vulkan readback memory", result); return false; }
    buffer_size_ = bytes;
    allocation_size_ = requirements.size;
    return true;
}

bool VulkanHost::capture(unsigned width, unsigned height, std::vector<uint32_t> &pixels, std::string &error) {
    std::lock_guard<std::recursive_mutex> guard(queue_mutex_);
    // Read the core's image synchronously into EmuLuna's existing CPU frame path.
    // This keeps filters, aspect handling, HUD and the frontend display API independent.
    if (!have_image_ || !width || !height || width > 4096 || height > 4096) {
        error = "The Vulkan core did not provide a valid image."; return false;
    }
    if (source_family_ != VK_QUEUE_FAMILY_IGNORED && source_family_ != queue_family_) {
        error = "The Vulkan core used a separate queue family that EmuLuna cannot read."; return false;
    }
    const VkFormat format = image_.create_info.format;
    if (format != last_format_) {
        std::fprintf(stderr, "Vulkan core image format: VkFormat %d\n", static_cast<int>(format));
        last_format_ = format;
    }
    const bool rgba = format == VK_FORMAT_R8G8B8A8_UNORM || format == VK_FORMAT_R8G8B8A8_SRGB;
    const bool bgra = format == VK_FORMAT_B8G8R8A8_UNORM || format == VK_FORMAT_B8G8R8A8_SRGB;
    const bool rgb555 = format == VK_FORMAT_A1R5G5B5_UNORM_PACK16;
    if (!rgba && !bgra && !rgb555) {
        error = "The Vulkan core returned an unsupported image format (VkFormat " +
                std::to_string(static_cast<int>(format)) + ").";
        return false;
    }
    if (!staging(static_cast<size_t>(width) * height * (rgb555 ? 2 : 4), error)) return false;

    VkCommandBufferAllocateInfo allocate{VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};
    allocate.commandPool = pool_;
    allocate.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    allocate.commandBufferCount = 1;
    VkCommandBuffer command = VK_NULL_HANDLE;
    VkResult result = allocate_commands_(device_, &allocate, &command);
    if (result != VK_SUCCESS) { error = failure("Allocating a Vulkan readback command", result); return false; }
    VkCommandBufferBeginInfo begin{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};
    begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    result = begin_command_(command, &begin);
    if (result != VK_SUCCESS) {
        error = failure("Starting Vulkan readback", result);
        free_commands_(device_, pool_, 1, &command); return false;
    }
    VkImageMemoryBarrier to_copy{VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER};
    to_copy.srcAccessMask = VK_ACCESS_MEMORY_WRITE_BIT;
    to_copy.dstAccessMask = VK_ACCESS_TRANSFER_READ_BIT;
    to_copy.oldLayout = image_.image_layout;
    to_copy.newLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL;
    to_copy.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    to_copy.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    to_copy.image = image_.create_info.image;
    to_copy.subresourceRange = image_.create_info.subresourceRange;
    barrier_(command, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT,
             0, 0, nullptr, 0, nullptr, 1, &to_copy);
    VkBufferImageCopy region{};
    region.imageSubresource.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
    region.imageSubresource.mipLevel = image_.create_info.subresourceRange.baseMipLevel;
    region.imageSubresource.baseArrayLayer = image_.create_info.subresourceRange.baseArrayLayer;
    region.imageSubresource.layerCount = 1;
    region.imageExtent = {width, height, 1};
    copy_image_(command, image_.create_info.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                buffer_, 1, &region);
    VkImageMemoryBarrier restore = to_copy;
    restore.srcAccessMask = VK_ACCESS_TRANSFER_READ_BIT;
    restore.dstAccessMask = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    restore.oldLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL;
    restore.newLayout = image_.image_layout;
    barrier_(command, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT,
             0, 0, nullptr, 0, nullptr, 1, &restore);
    result = end_command_(command);
    if (result != VK_SUCCESS) {
        error = failure("Finishing Vulkan readback", result);
        free_commands_(device_, pool_, 1, &command); return false;
    }
    std::vector<VkCommandBuffer> submitted = commands_;
    submitted.push_back(command);
    std::vector<VkPipelineStageFlags> wait_stages(semaphores_.size(), VK_PIPELINE_STAGE_ALL_COMMANDS_BIT);
    VkSubmitInfo submit{VK_STRUCTURE_TYPE_SUBMIT_INFO};
    submit.waitSemaphoreCount = static_cast<uint32_t>(semaphores_.size());
    submit.pWaitSemaphores = semaphores_.data();
    submit.pWaitDstStageMask = wait_stages.data();
    submit.commandBufferCount = static_cast<uint32_t>(submitted.size());
    submit.pCommandBuffers = submitted.data();
    if (signal_semaphore_) { submit.signalSemaphoreCount = 1; submit.pSignalSemaphores = &signal_semaphore_; }
    result = submit_(queue_, 1, &submit, VK_NULL_HANDLE);
    if (result == VK_SUCCESS) result = wait_idle_(queue_);
    free_commands_(device_, pool_, 1, &command);
    commands_.clear(); semaphores_.clear(); signal_semaphore_ = VK_NULL_HANDLE;
    have_image_ = false;
    if (result != VK_SUCCESS) { error = failure("Reading the Vulkan frame", result); return false; }
    void *mapped = nullptr;
    result = map_memory_(device_, memory_, 0, allocation_size_, 0, &mapped);
    if (result != VK_SUCCESS) { error = failure("Mapping the Vulkan frame", result); return false; }
    if (!coherent_) {
        VkMappedMemoryRange range{VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE};
        range.memory = memory_;
        range.size = VK_WHOLE_SIZE;
        result = invalidate_memory_(device_, 1, &range);
    }
    if (result == VK_SUCCESS) {
        pixels.resize(static_cast<size_t>(width) * height);
        const auto *source = static_cast<const uint8_t*>(mapped);
        for (size_t i = 0; i < pixels.size(); ++i) {
            if (rgb555) {
                // Beetle PSX HW uses the PlayStation's 15-bit scanout for its
                // default dither mode. Vulkan packs B:G:R:A into 5:5:5:1 bits.
                const uint16_t value = static_cast<uint16_t>(source[i * 2]) |
                                       (static_cast<uint16_t>(source[i * 2 + 1]) << 8);
                const uint32_t red = (value >> 10) & 31;
                const uint32_t green = (value >> 5) & 31;
                const uint32_t blue = value & 31;
                pixels[i] = 0xff000000u |
                            ((red * 255 / 31) << 16) |
                            ((green * 255 / 31) << 8) |
                            (blue * 255 / 31);
                continue;
            }
            const auto *p = source + i * 4;
            uint32_t red = rgba ? p[0] : p[2];
            uint32_t green = p[1];
            uint32_t blue = rgba ? p[2] : p[0];
            pixels[i] = 0xff000000u | (red << 16) | (green << 8) | blue;
        }
    }
    unmap_memory_(device_, memory_);
    if (result != VK_SUCCESS) { error = failure("Reading Vulkan staging memory", result); return false; }
    return true;
}

void VulkanHost::set_image(void *handle, const retro_vulkan_image *image, uint32_t count,
                           const VkSemaphore *semaphores, uint32_t family) {
    auto *self = static_cast<VulkanHost*>(handle);
    std::lock_guard<std::recursive_mutex> guard(self->queue_mutex_);
    self->have_image_ = image != nullptr;
    if (image) self->image_ = *image;
    if (semaphores && count) self->semaphores_.assign(semaphores, semaphores + count);
    else self->semaphores_.clear();
    self->source_family_ = family;
}
void VulkanHost::set_command_buffers(void *handle, uint32_t count, const VkCommandBuffer *commands) {
    auto *self = static_cast<VulkanHost*>(handle);
    std::lock_guard<std::recursive_mutex> guard(self->queue_mutex_);
    if (commands && count) self->commands_.assign(commands, commands + count);
    else self->commands_.clear();
}
void VulkanHost::wait_sync(void *handle) {
    auto *self = static_cast<VulkanHost*>(handle);
    std::lock_guard<std::recursive_mutex> guard(self->queue_mutex_);
    if (self->queue_) self->wait_idle_(self->queue_);
}
void VulkanHost::lock_queue(void *handle) { static_cast<VulkanHost*>(handle)->queue_mutex_.lock(); }
void VulkanHost::unlock_queue(void *handle) { static_cast<VulkanHost*>(handle)->queue_mutex_.unlock(); }
void VulkanHost::set_signal_semaphore(void *handle, VkSemaphore semaphore) {
    auto *self = static_cast<VulkanHost*>(handle);
    std::lock_guard<std::recursive_mutex> guard(self->queue_mutex_);
    self->signal_semaphore_ = semaphore;
}

void VulkanHost::close() {
    if (queue_ && wait_idle_) wait_idle_(queue_);
    if (buffer_ && destroy_buffer_) destroy_buffer_(device_, buffer_, nullptr);
    if (memory_ && free_memory_) free_memory_(device_, memory_, nullptr);
    if (pool_ && destroy_pool_) destroy_pool_(device_, pool_, nullptr);
    if (core_created_device_ && negotiation_.destroy_device) negotiation_.destroy_device();
    if (device_ && destroy_device_) destroy_device_(device_, nullptr);
    if (instance_ && destroy_instance_) destroy_instance_(instance_, nullptr);
    if (loader_) dlclose(loader_);
    loader_ = nullptr; instance_ = VK_NULL_HANDLE; device_ = VK_NULL_HANDLE;
    queue_ = VK_NULL_HANDLE; pool_ = VK_NULL_HANDLE;
    buffer_ = VK_NULL_HANDLE; memory_ = VK_NULL_HANDLE;
    core_created_device_ = false;
    interface_.instance = VK_NULL_HANDLE;
    interface_.device = VK_NULL_HANDLE;
}
