#[derive(Copy, Clone)]
pub struct BufferResource {
    state_key: DefaultKey,
    buffer: VkBuffer,
    memory: VkMemory,
    size: usize,
}

pub struct ImageResource {

}