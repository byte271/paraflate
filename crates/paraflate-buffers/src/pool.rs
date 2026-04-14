use std::sync::Arc;

use paraflate_core::ParaflateResult;
use parking_lot::Mutex;

#[derive(Clone, Debug)]
pub struct BufferPoolConfig {
    pub buffer_len: usize,
    pub max_buffers: usize,
}

impl Default for BufferPoolConfig {
    fn default() -> Self {
        Self {
            buffer_len: 1024 * 1024,
            max_buffers: 64,
        }
    }
}

pub struct BufferHandle {
    pool: Arc<InnerPool>,
    data: Vec<u8>,
}

impl BufferHandle {
    pub fn as_slice(&self) -> &[u8] {
        self.data.as_slice()
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.data.as_mut_slice()
    }

    pub fn clear(&mut self) {
        self.data.clear();
    }

    pub fn capacity(&self) -> usize {
        self.data.capacity()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
    }
}

impl Drop for BufferHandle {
    fn drop(&mut self) {
        let mut inner = self.pool.free.lock();
        if inner.len() < self.pool.config.max_buffers {
            self.data.clear();
            let mut v = Vec::new();
            std::mem::swap(&mut v, &mut self.data);
            if v.capacity() >= self.pool.config.buffer_len {
                inner.push(v);
            }
        }
    }
}

struct InnerPool {
    config: BufferPoolConfig,
    free: Mutex<Vec<Vec<u8>>>,
}

pub struct BufferPool {
    inner: Arc<InnerPool>,
}

impl BufferPool {
    pub fn new(config: BufferPoolConfig) -> ParaflateResult<Self> {
        if config.buffer_len == 0 || config.max_buffers == 0 {
            return Err(paraflate_core::ParaflateError::InvariantViolated(
                "buffer pool dimensions".to_string(),
            ));
        }
        Ok(Self {
            inner: Arc::new(InnerPool {
                config,
                free: Mutex::new(Vec::new()),
            }),
        })
    }

    pub fn acquire(&self) -> BufferHandle {
        let mut v = self
            .inner
            .free
            .lock()
            .pop()
            .unwrap_or_else(|| Vec::with_capacity(self.inner.config.buffer_len.max(4096)));
        v.clear();
        BufferHandle {
            pool: Arc::clone(&self.inner),
            data: v,
        }
    }

    pub fn config(&self) -> &BufferPoolConfig {
        &self.inner.config
    }
}
