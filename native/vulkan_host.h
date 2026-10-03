/* SPDX-License-Identifier: MIT
 * Headless libretro Vulkan context for cores that render their own images.
 */
#pragma once

#define VK_NO_PROTOTYPES
#include "vendor/libretro_vulkan.h"
#include <cstdint>
#include <mutex>
#include <string>
#include <vector>

class VulkanHost {
public:
    VulkanHost();
    ~VulkanHost();
    VulkanHost(const VulkanHost&) = delete;
    VulkanHost& operator=(const VulkanHost&) = delete;

    static bool available();
    bool set_negotiation(const retro_hw_render_context_negotiation_interface *requested);
    bool initialize(std::string &error);
    bool capture(unsigned width, unsigned height, std::vector<uint32_t> &pixels, std::string &error);
    const retro_hw_render_interface_vulkan *interface() const { return &interface_; }

private:
    bool load_loader(std::string &error);
    void close();
    bool staging(size_t bytes, std::string &error);

    static void set_image(void *handle, const retro_vulkan_image *image, uint32_t count,
                          const VkSemaphore *semaphores, uint32_t family);
    static uint32_t sync_index(void *) { return 0; }
    static uint32_t sync_mask(void *) { return 1; }
    static void set_command_buffers(void *handle, uint32_t count, const VkCommandBuffer *commands);
    static void wait_sync(void *handle);
    static void lock_queue(void *handle);
    static void unlock_queue(void *handle);
    static void set_signal_semaphore(void *handle, VkSemaphore semaphore);

    void *loader_ = nullptr;
    PFN_vkGetInstanceProcAddr get_instance_proc_ = nullptr;
    PFN_vkGetDeviceProcAddr get_device_proc_ = nullptr;
    PFN_vkCreateInstance create_instance_ = nullptr;
    PFN_vkDestroyInstance destroy_instance_ = nullptr;
    PFN_vkEnumeratePhysicalDevices enumerate_devices_ = nullptr;
    PFN_vkGetPhysicalDeviceQueueFamilyProperties queue_families_ = nullptr;
    PFN_vkGetPhysicalDeviceMemoryProperties memory_properties_ = nullptr;
    PFN_vkCreateDevice create_device_ = nullptr;
    PFN_vkDestroyDevice destroy_device_ = nullptr;
    PFN_vkGetDeviceQueue get_queue_ = nullptr;
    PFN_vkCreateCommandPool create_pool_ = nullptr;
    PFN_vkDestroyCommandPool destroy_pool_ = nullptr;
    PFN_vkAllocateCommandBuffers allocate_commands_ = nullptr;
    PFN_vkFreeCommandBuffers free_commands_ = nullptr;
    PFN_vkBeginCommandBuffer begin_command_ = nullptr;
    PFN_vkEndCommandBuffer end_command_ = nullptr;
    PFN_vkCmdPipelineBarrier barrier_ = nullptr;
    PFN_vkCmdCopyImageToBuffer copy_image_ = nullptr;
    PFN_vkQueueSubmit submit_ = nullptr;
    PFN_vkQueueWaitIdle wait_idle_ = nullptr;
    PFN_vkCreateBuffer create_buffer_ = nullptr;
    PFN_vkDestroyBuffer destroy_buffer_ = nullptr;
    PFN_vkGetBufferMemoryRequirements buffer_requirements_ = nullptr;
    PFN_vkAllocateMemory allocate_memory_ = nullptr;
    PFN_vkFreeMemory free_memory_ = nullptr;
    PFN_vkBindBufferMemory bind_buffer_ = nullptr;
    PFN_vkMapMemory map_memory_ = nullptr;
    PFN_vkUnmapMemory unmap_memory_ = nullptr;
    PFN_vkInvalidateMappedMemoryRanges invalidate_memory_ = nullptr;

    retro_hw_render_context_negotiation_interface_vulkan negotiation_{};
    retro_hw_render_interface_vulkan interface_{};
    VkInstance instance_ = VK_NULL_HANDLE;
    VkPhysicalDevice gpu_ = VK_NULL_HANDLE;
    VkDevice device_ = VK_NULL_HANDLE;
    VkQueue queue_ = VK_NULL_HANDLE;
    uint32_t queue_family_ = 0;
    VkCommandPool pool_ = VK_NULL_HANDLE;
    VkBuffer buffer_ = VK_NULL_HANDLE;
    VkDeviceMemory memory_ = VK_NULL_HANDLE;
    VkDeviceSize buffer_size_ = 0;
    VkDeviceSize allocation_size_ = 0;
    bool coherent_ = false;
    bool core_created_device_ = false;
    bool have_image_ = false;
    VkFormat last_format_ = VK_FORMAT_UNDEFINED;
    retro_vulkan_image image_{};
    std::vector<VkSemaphore> semaphores_;
    std::vector<VkCommandBuffer> commands_;
    VkSemaphore signal_semaphore_ = VK_NULL_HANDLE;
    uint32_t source_family_ = VK_QUEUE_FAMILY_IGNORED;
    std::recursive_mutex queue_mutex_;
};
